#[cfg(feature = "desktop-gpu")]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Result, anyhow};
use ffmpeg_next::util::color;
use winit::{event_loop::OwnedDisplayHandle, window::Window};

use super::{
    DESKTOP_VOLUME_MAX,
    graphics::RgbaBitmap,
    render::{Rect, WindowFrame, fit_rect, help_panel_rect, logo_rect, subtitle_rect},
    video::VideoSurface,
};

pub(super) type WindowRenderer = WgpuRenderer;

#[cfg(feature = "desktop-gpu")]
pub(super) struct WgpuRenderer {
    window: Arc<Window>,
    instance: wgpu::Instance,
    state: WgpuState,
}

#[cfg(feature = "desktop-gpu")]
struct WgpuState {
    surface: wgpu::Surface<'static>,
    _adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    device_lost: Arc<AtomicBool>,
    yuv: GpuYuvRenderer,
    sprites: GpuSpriteRenderer,
}

#[cfg(feature = "desktop-gpu")]
impl WgpuRenderer {
    pub(super) fn new(
        window: Arc<Window>,
        display_handle: OwnedDisplayHandle,
        _width: u32,
        _height: u32,
    ) -> Result<Self> {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display_handle)).with_env(),
        );
        let size = window.inner_size();
        let state = WgpuState::new(&instance, &window, (size.width, size.height))?;
        Ok(Self {
            window,
            instance,
            state,
        })
    }

    fn rebuild_state(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        self.state = WgpuState::new(&self.instance, &self.window, (size.width, size.height))?;
        Ok(())
    }

    fn acquire_surface_texture(&mut self) -> Result<Option<(wgpu::SurfaceTexture, bool)>> {
        for _ in 0..2 {
            if self.state.device_lost.swap(false, Ordering::AcqRel) {
                self.rebuild_state()?;
            }
            match self.state.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(texture) => {
                    return Ok(Some((texture, false)));
                }
                wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                    return Ok(Some((texture, true)));
                }
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    return Ok(None);
                }
                wgpu::CurrentSurfaceTexture::Outdated => {
                    self.state.configure_surface();
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    log::warn!("WGPU desktop surface lost; rebuilding renderer state");
                    self.rebuild_state()?;
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    return Err(anyhow!("validating WGPU desktop surface texture"));
                }
            }
        }
        // Recovery succeeded but the compositor did not provide a usable image yet.
        // The next scheduled video frame retries with the refreshed state.
        Ok(None)
    }

    pub(super) fn resize_surface(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.state.config.width = width;
        self.state.config.height = height;
        self.state.configure_surface();
        Ok(())
    }

    pub(super) fn resize_buffer(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let _ = (width, height);
        Ok(())
    }

    pub(super) fn reset_frame_cache(&mut self) {
        self.state.yuv.reset_frame_cache();
    }

    pub(super) fn release_frame_resources(&mut self) {
        self.state.yuv.release_frame_resources();
        self.state.sprites.release_frame_resources();
    }

    pub(super) fn render(&mut self, frame: &WindowFrame, size: (u32, u32)) -> Result<()> {
        let Some((surface_texture, reconfigure_after_present)) = self.acquire_surface_texture()?
        else {
            return Ok(());
        };
        if let Some(video) = &frame.video {
            self.state.yuv.upload(video)?;
        }
        let target = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("ffplayout_desktop_encoder"),
                });
        self.state
            .yuv
            .render(&mut encoder, &target, size, frame.video.is_some());
        self.state
            .sprites
            .render(&mut encoder, &target, frame, size);
        self.state.queue.submit(Some(encoder.finish()));
        // On Wayland this must describe the buffer submission that immediately follows.
        self.window.pre_present_notify();
        surface_texture.present();
        if reconfigure_after_present {
            self.state.configure_surface();
        }
        Ok(())
    }
}

#[cfg(feature = "desktop-gpu")]
impl WgpuState {
    fn new(instance: &wgpu::Instance, window: &Arc<Window>, size: (u32, u32)) -> Result<Self> {
        let surface = instance
            .create_surface(Arc::clone(window))
            .map_err(|error| anyhow!("creating WGPU surface: {error}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .map_err(|error| anyhow!("requesting WGPU adapter: {error}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ffplayout_desktop_device"),
            ..Default::default()
        }))
        .map_err(|error| anyhow!("requesting WGPU device: {error}"))?;
        let device_lost = Arc::new(AtomicBool::new(false));
        let device_lost_callback = Arc::clone(&device_lost);
        device.set_device_lost_callback(move |reason, message| {
            log::error!("WGPU desktop device lost ({reason:?}): {message}");
            device_lost_callback.store(true, Ordering::Release);
        });
        device.on_uncaptured_error(Arc::new(|error| {
            log::error!("uncaptured WGPU desktop error: {error}");
        }));
        let config = surface_config(&surface, &adapter, size.0, size.1)?;
        surface.configure(&device, &config);
        let yuv = GpuYuvRenderer::new(&device, &queue, config.format);
        let sprites = GpuSpriteRenderer::new(&device, &queue, config.format);
        Ok(Self {
            surface,
            _adapter: adapter,
            device,
            queue,
            config,
            device_lost,
            yuv,
            sprites,
        })
    }

    fn configure_surface(&self) {
        self.surface.configure(&self.device, &self.config);
    }
}

#[cfg(feature = "desktop-gpu")]
fn surface_config(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
) -> Result<wgpu::SurfaceConfiguration> {
    let capabilities = surface.get_capabilities(adapter);
    let format = capabilities
        .formats
        .iter()
        .copied()
        .find(wgpu::TextureFormat::is_srgb)
        .or_else(|| capabilities.formats.first().copied())
        .ok_or_else(|| anyhow!("WGPU adapter does not support the desktop surface"))?;
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: width.max(1),
        height: height.max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        desired_maximum_frame_latency: 2,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: Vec::new(),
    })
}

#[cfg(feature = "desktop-gpu")]
const YUV_MATRIX_PARAMETER_END: usize = 16;
#[cfg(feature = "desktop-gpu")]
const YUV_SCALE_PARAMETER_END: usize = 20;
#[cfg(feature = "desktop-gpu")]
const YUV_PARAMETER_COUNT: usize = 24;
#[cfg(feature = "desktop-gpu")]
const YUV_PARAMETER_BYTES: u64 = (YUV_PARAMETER_COUNT * std::mem::size_of::<f32>()) as u64;

#[cfg(feature = "desktop-gpu")]
const _: () = {
    assert!(YUV_MATRIX_PARAMETER_END + 4 == YUV_SCALE_PARAMETER_END);
    assert!(YUV_SCALE_PARAMETER_END + 4 == YUV_PARAMETER_COUNT);
};

#[cfg(feature = "desktop-gpu")]
struct GpuYuvRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    parameters: wgpu::Buffer,
    parameter_values: [f32; YUV_PARAMETER_COUNT],
    output_is_srgb: bool,
    textures: Option<GpuYuvTextures>,
    last_pts: Option<i64>,
    last_y_plane: Option<Arc<[u8]>>,
}

#[cfg(feature = "desktop-gpu")]
struct GpuYuvTextures {
    width: u32,
    height: u32,
    y: wgpu::Texture,
    u: wgpu::Texture,
    v: wgpu::Texture,
    _y_view: wgpu::TextureView,
    _u_view: wgpu::TextureView,
    _v_view: wgpu::TextureView,
    _sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
}

#[cfg(feature = "desktop-gpu")]
impl GpuYuvRenderer {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let device = device.clone();
        let queue = queue.clone();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ffplayout_yuv_bind_group_layout"),
            entries: &[
                texture_binding(0),
                texture_binding(1),
                texture_binding(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(YUV_PARAMETER_BYTES),
                    },
                    count: None,
                },
            ],
        });
        let parameters = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ffplayout_yuv_parameters"),
            size: YUV_PARAMETER_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ffplayout_yuv_shader"),
            source: wgpu::ShaderSource::Wgsl(YUV_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ffplayout_yuv_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ffplayout_yuv_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            device,
            queue,
            pipeline,
            layout,
            parameters,
            parameter_values: [0.0; YUV_PARAMETER_COUNT],
            output_is_srgb: surface_format.is_srgb(),
            textures: None,
            last_pts: None,
            last_y_plane: None,
        }
    }

    fn reset_frame_cache(&mut self) {
        self.last_pts = None;
        self.last_y_plane = None;
    }

    fn release_frame_resources(&mut self) {
        self.reset_frame_cache();
        self.textures = None;
    }

    fn upload(&mut self, video: &VideoSurface) -> Result<()> {
        let recreate = self.textures.as_ref().is_none_or(|textures| {
            textures.width != video.width || textures.height != video.height
        });
        if recreate {
            self.textures = Some(self.create_textures(video.width, video.height));
            self.reset_frame_cache();
        }
        if self.last_pts == Some(video.pts)
            && self
                .last_y_plane
                .as_ref()
                .is_some_and(|plane| Arc::ptr_eq(plane, &video.y))
        {
            return Ok(());
        }
        let textures = self.textures.as_ref().expect("YUV textures initialized");
        write_plane(
            &self.queue,
            &textures.y,
            video.width,
            video.height,
            &video.y,
        );
        write_plane(
            &self.queue,
            &textures.u,
            video.width.div_ceil(2),
            video.height.div_ceil(2),
            &video.u,
        );
        write_plane(
            &self.queue,
            &textures.v,
            video.width.div_ceil(2),
            video.height.div_ceil(2),
            &video.v,
        );
        self.parameter_values[..YUV_MATRIX_PARAMETER_END]
            .copy_from_slice(&yuv_color_parameters(video.color_space, video.color_range));
        self.parameter_values[YUV_SCALE_PARAMETER_END..]
            .copy_from_slice(&color_render_parameters(video));
        self.last_pts = Some(video.pts);
        self.last_y_plane = Some(Arc::clone(&video.y));
        Ok(())
    }

    fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        surface_size: (u32, u32),
        show_video: bool,
    ) {
        if let Some(textures) = &self.textures {
            let rect = fit_rect(
                textures.width,
                textures.height,
                surface_size.0,
                surface_size.1,
            );
            set_yuv_scale_parameters(
                &mut self.parameter_values,
                [
                    rect.width as f32 / surface_size.0.max(1) as f32,
                    rect.height as f32 / surface_size.1.max(1) as f32,
                    0.0,
                    if self.output_is_srgb { 1.0 } else { 0.0 },
                ],
            );
            self.queue.write_buffer(
                &self.parameters,
                0,
                bytemuck::cast_slice(&self.parameter_values),
            );
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ffplayout_yuv_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if show_video && let Some(textures) = &self.textures {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &textures.bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }

    fn create_textures(&self, width: u32, height: u32) -> GpuYuvTextures {
        let y = create_plane_texture(&self.device, width, height, "y");
        let u = create_plane_texture(&self.device, width.div_ceil(2), height.div_ceil(2), "u");
        let v = create_plane_texture(&self.device, width.div_ceil(2), height.div_ceil(2), "v");
        let y_view = y.create_view(&wgpu::TextureViewDescriptor::default());
        let u_view = u.create_view(&wgpu::TextureViewDescriptor::default());
        let v_view = v.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ffplayout_yuv_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ffplayout_yuv_bind_group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&y_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&u_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&v_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.parameters.as_entire_binding(),
                },
            ],
        });
        GpuYuvTextures {
            width,
            height,
            y,
            u,
            v,
            _y_view: y_view,
            _u_view: u_view,
            _v_view: v_view,
            _sampler: sampler,
            bind_group,
        }
    }
}

#[cfg(feature = "desktop-gpu")]
fn texture_binding(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

#[cfg(feature = "desktop-gpu")]
fn create_plane_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    plane: &str,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("ffplayout_yuv_{plane}_texture")),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

#[cfg(feature = "desktop-gpu")]
fn write_plane(queue: &wgpu::Queue, texture: &wgpu::Texture, width: u32, height: u32, data: &[u8]) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(feature = "desktop-gpu")]
pub(super) fn yuv_color_parameters(space: color::Space, range: color::Range) -> [f32; 16] {
    let (matrix, offset) = match (space, range) {
        (color::Space::BT2020NCL | color::Space::BT2020CL, color::Range::JPEG) => (
            [
                1.0, 1.0, 1.0, 0.0, 0.0, -0.1646, 1.8814, 0.0, 1.4746, -0.5714, 0.0, 0.0,
            ],
            [0.0, -0.5, -0.5, 0.0],
        ),
        (color::Space::BT2020NCL | color::Space::BT2020CL, _) => (
            [
                1.1644, 1.1644, 1.1644, 0.0, 0.0, -0.1873, 2.1418, 0.0, 1.6787, -0.6504, 0.0, 0.0,
            ],
            [-16.0 / 255.0, -0.5, -0.5, 0.0],
        ),
        (color::Space::BT709, color::Range::JPEG) => (
            [
                1.0, 1.0, 1.0, 0.0, 0.0, -0.1873, 1.8556, 0.0, 1.5748, -0.4681, 0.0, 0.0,
            ],
            [0.0, -0.5, -0.5, 0.0],
        ),
        (color::Space::BT709, _) => (
            [
                1.1644, 1.1644, 1.1644, 0.0, 0.0, -0.2132, 2.1124, 0.0, 1.7927, -0.5329, 0.0, 0.0,
            ],
            [-16.0 / 255.0, -0.5, -0.5, 0.0],
        ),
        (_, color::Range::JPEG) => (
            [
                1.0, 1.0, 1.0, 0.0, 0.0, -0.3441, 1.7720, 0.0, 1.4020, -0.7141, 0.0, 0.0,
            ],
            [0.0, -0.5, -0.5, 0.0],
        ),
        _ => (
            [
                1.1644, 1.1644, 1.1644, 0.0, 0.0, -0.3918, 2.0172, 0.0, 1.5960, -0.8130, 0.0, 0.0,
            ],
            [-16.0 / 255.0, -0.5, -0.5, 0.0],
        ),
    };
    let mut parameters = [0.0; 16];
    parameters[..12].copy_from_slice(&matrix);
    parameters[12..].copy_from_slice(&offset);
    parameters
}

#[cfg(feature = "desktop-gpu")]
fn color_render_parameters(video: &VideoSurface) -> [f32; 4] {
    let transfer = match video.color_transfer {
        color::TransferCharacteristic::SMPTE2084 => 1.0,
        color::TransferCharacteristic::ARIB_STD_B67 => 2.0,
        color::TransferCharacteristic::Linear => 3.0,
        color::TransferCharacteristic::IEC61966_2_1 => 4.0,
        _ => 0.0,
    };
    let bt2020 = matches!(video.color_primaries, color::Primaries::BT2020)
        || matches!(
            video.color_space,
            color::Space::BT2020NCL | color::Space::BT2020CL
        );
    [
        transfer,
        if bt2020 { 1.0 } else { 0.0 },
        if transfer == 1.0 || transfer == 2.0 {
            1.0
        } else {
            0.0
        },
        0.0,
    ]
}

#[cfg(feature = "desktop-gpu")]
fn set_yuv_scale_parameters(parameters: &mut [f32; YUV_PARAMETER_COUNT], scale: [f32; 4]) {
    parameters[YUV_MATRIX_PARAMETER_END..YUV_SCALE_PARAMETER_END].copy_from_slice(&scale);
}

#[cfg(feature = "desktop-gpu")]
const YUV_SHADER: &str = r#"
struct Parameters {
    column0: vec4<f32>,
    column1: vec4<f32>,
    column2: vec4<f32>,
    offset: vec4<f32>,
    scale: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var y_plane: texture_2d<f32>;
@group(0) @binding(1) var u_plane: texture_2d<f32>;
@group(0) @binding(2) var v_plane: texture_2d<f32>;
@group(0) @binding(3) var plane_sampler: sampler;
@group(0) @binding(4) var<uniform> parameters: Parameters;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let position = positions[index];
    var output: VertexOutput;
    output.position = vec4<f32>(position * parameters.scale.xy, 0.0, 1.0);
    output.uv = vec2<f32>((position.x + 1.0) * 0.5, (1.0 - position.y) * 0.5);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let yuv = vec3<f32>(
        textureSample(y_plane, plane_sampler, input.uv).r,
        textureSample(u_plane, plane_sampler, input.uv).r,
        textureSample(v_plane, plane_sampler, input.uv).r,
    ) + parameters.offset.xyz;
    let matrix = mat3x3<f32>(
        parameters.column0.xyz,
        parameters.column1.xyz,
        parameters.column2.xyz,
    );
    let rgb = clamp(matrix * yuv, vec3<f32>(0.0), vec3<f32>(1.0));
    var linear: vec3<f32>;
    if parameters.color.x > 3.5 {
        linear = inverse_srgb(rgb);
    } else if parameters.color.x > 2.5 {
        linear = rgb;
    } else if parameters.color.x > 1.5 {
        linear = hlg_eotf(rgb);
    } else if parameters.color.x > 0.5 {
        linear = pq_eotf(rgb) * 100.0;
    } else {
        linear = inverse_bt709(rgb);
    }
    if parameters.color.y > 0.5 {
        linear = mat3x3<f32>(
            vec3<f32>(1.6605, -0.1246, -0.0182),
            vec3<f32>(-0.5876, 1.1329, -0.1006),
            vec3<f32>(-0.0728, -0.0083, 1.1187),
        ) * linear;
    }
    if parameters.color.z > 0.5 {
        linear = aces_tonemap(max(linear, vec3<f32>(0.0)));
    }
    linear = clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
    let output = select(linear_to_srgb(linear), linear, parameters.scale.w > 0.5);
    return vec4<f32>(output, 1.0);
}

fn inverse_bt709(value: vec3<f32>) -> vec3<f32> {
    return select(
        pow((value + vec3<f32>(0.099)) / 1.099, vec3<f32>(1.0 / 0.45)),
        value / 4.5,
        value < vec3<f32>(0.081),
    );
}

fn inverse_srgb(value: vec3<f32>) -> vec3<f32> {
    return select(
        pow((value + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)),
        value / 12.92,
        value <= vec3<f32>(0.04045),
    );
}

fn linear_to_srgb(value: vec3<f32>) -> vec3<f32> {
    return select(
        1.055 * pow(value, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055),
        value * 12.92,
        value <= vec3<f32>(0.0031308),
    );
}

fn pq_eotf(value: vec3<f32>) -> vec3<f32> {
    let m1 = 2610.0 / 16384.0;
    let m2 = 2523.0 / 32.0;
    let c1 = 3424.0 / 4096.0;
    let c2 = 2413.0 / 128.0;
    let c3 = 2392.0 / 128.0;
    let power = pow(value, vec3<f32>(1.0 / m2));
    return pow(
        max(power - vec3<f32>(c1), vec3<f32>(0.0)) /
            max(vec3<f32>(c2) - c3 * power, vec3<f32>(0.000001)),
        vec3<f32>(1.0 / m1),
    );
}

fn hlg_eotf(value: vec3<f32>) -> vec3<f32> {
    let a = 0.17883277;
    let b = 0.28466892;
    let c = 0.55991073;
    let linear = select(
        (value * value) / 3.0,
        (exp((value - vec3<f32>(c)) / a) + vec3<f32>(b)) / 12.0,
        value > vec3<f32>(0.5),
    );
    return linear * 3.0;
}

fn aces_tonemap(value: vec3<f32>) -> vec3<f32> {
    return clamp(
        value * (2.51 * value + vec3<f32>(0.03)) /
            (value * (2.43 * value + vec3<f32>(0.59)) + vec3<f32>(0.14)),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
}
"#;

#[cfg(feature = "desktop-gpu")]
struct GpuSpriteRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    output_is_srgb: bool,
    logo: Option<CachedSprite>,
    subtitle: Option<CachedSprite>,
    volume: Option<CachedSprite>,
    help_panel: Option<CachedSprite>,
    help: Option<CachedSprite>,
    volume_bitmap: Option<(u32, RgbaBitmap)>,
    help_panel_bitmap: RgbaBitmap,
}

#[cfg(feature = "desktop-gpu")]
struct CachedSprite {
    pixels: Arc<[u8]>,
    width: u32,
    height: u32,
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

#[cfg(feature = "desktop-gpu")]
struct SpriteRenderContext<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    pipeline: &'a wgpu::RenderPipeline,
    layout: &'a wgpu::BindGroupLayout,
    sampler: &'a wgpu::Sampler,
    output_is_srgb: bool,
}

#[cfg(feature = "desktop-gpu")]
impl GpuSpriteRenderer {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let device = device.clone();
        let queue = queue.clone();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ffplayout_sprite_bind_group_layout"),
            entries: &[
                texture_binding(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(32),
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ffplayout_sprite_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ffplayout_sprite_shader"),
            source: wgpu::ShaderSource::Wgsl(SPRITE_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ffplayout_sprite_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ffplayout_sprite_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            device,
            queue,
            pipeline,
            layout,
            sampler,
            output_is_srgb: surface_format.is_srgb(),
            logo: None,
            subtitle: None,
            volume: None,
            help_panel: None,
            help: None,
            volume_bitmap: None,
            help_panel_bitmap: solid_bitmap([16, 18, 20, 205]),
        }
    }

    fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        frame: &WindowFrame,
        size: (u32, u32),
    ) {
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        let context = SpriteRenderContext {
            device: &self.device,
            queue: &self.queue,
            pipeline: &self.pipeline,
            layout: &self.layout,
            sampler: &self.sampler,
            output_is_srgb: self.output_is_srgb,
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ffplayout_sprite_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(logo) = &frame.logo
            && let Some(rect) = logo_rect(logo, size)
        {
            context.draw_bitmap(
                &mut self.logo,
                &mut pass,
                &logo.bitmap,
                rect,
                logo.opacity,
                size,
            );
        }
        if let Some(subtitle) = &frame.subtitle {
            context.draw_bitmap(
                &mut self.subtitle,
                &mut pass,
                subtitle,
                subtitle_rect(subtitle, size),
                255,
                size,
            );
        }
        if frame.volume_overlay {
            let fill = volume_fill(frame.volume);
            if self
                .volume_bitmap
                .as_ref()
                .is_none_or(|(cached_fill, _)| *cached_fill != fill)
            {
                self.volume_bitmap = Some((fill, volume_bitmap(fill)));
            }
            let bitmap = self
                .volume_bitmap
                .as_ref()
                .expect("volume bitmap initialized")
                .1
                .clone();
            context.draw_bitmap(
                &mut self.volume,
                &mut pass,
                &bitmap,
                Rect {
                    x: (size.0.saturating_sub(bitmap.width)) / 2,
                    y: size.1.saturating_sub(bitmap.height + 24),
                    width: bitmap.width,
                    height: bitmap.height,
                },
                255,
                size,
            );
        }
        if let Some(help) = &frame.help {
            let panel = help_panel_rect(help, size);
            context.draw_bitmap(
                &mut self.help_panel,
                &mut pass,
                &self.help_panel_bitmap,
                panel,
                255,
                size,
            );
            context.draw_bitmap(
                &mut self.help,
                &mut pass,
                help,
                Rect {
                    x: panel.x + (panel.width.saturating_sub(help.width)) / 2,
                    y: panel.y + (panel.height.saturating_sub(help.height)) / 2,
                    width: help.width.min(panel.width),
                    height: help.height.min(panel.height),
                },
                255,
                size,
            );
        }
    }

    fn release_frame_resources(&mut self) {
        self.logo = None;
        self.subtitle = None;
        self.volume = None;
        self.help_panel = None;
        self.help = None;
        self.volume_bitmap = None;
    }
}

#[cfg(feature = "desktop-gpu")]
impl SpriteRenderContext<'_> {
    fn draw_bitmap(
        &self,
        cache: &mut Option<CachedSprite>,
        pass: &mut wgpu::RenderPass<'_>,
        bitmap: &RgbaBitmap,
        rect: Rect,
        opacity: u8,
        surface_size: (u32, u32),
    ) {
        if bitmap.width == 0 || bitmap.height == 0 || rect.width == 0 || rect.height == 0 {
            return;
        }
        if cache.as_ref().is_none_or(|cached| {
            cached.width != bitmap.width
                || cached.height != bitmap.height
                || !Arc::ptr_eq(&cached.pixels, &bitmap.pixels)
        }) {
            *cache = Some(CachedSprite::new(
                self.device,
                self.queue,
                self.layout,
                self.sampler,
                bitmap,
            ));
        }
        let cached = cache.as_ref().expect("sprite cache initialized");
        let values = [
            rect.width as f32 / surface_size.0 as f32,
            rect.height as f32 / surface_size.1 as f32,
            (rect.x as f32 + rect.width as f32 * 0.5) * 2.0 / surface_size.0 as f32 - 1.0,
            1.0 - (rect.y as f32 + rect.height as f32 * 0.5) * 2.0 / surface_size.1 as f32,
            opacity as f32 / 255.0,
            if self.output_is_srgb { 1.0 } else { 0.0 },
            0.0,
            0.0,
        ];
        self.queue
            .write_buffer(&cached.uniform, 0, bytemuck::cast_slice(&values));
        pass.set_pipeline(self.pipeline);
        pass.set_bind_group(0, &cached.bind_group, &[]);
        pass.draw(0..6, 0..1);
    }
}

#[cfg(feature = "desktop-gpu")]
impl CachedSprite {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        bitmap: &RgbaBitmap,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ffplayout_sprite_texture"),
            size: wgpu::Extent3d {
                width: bitmap.width,
                height: bitmap.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        write_rgba_texture(queue, &texture, bitmap);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ffplayout_sprite_parameters"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ffplayout_sprite_bind_group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        Self {
            pixels: Arc::clone(&bitmap.pixels),
            width: bitmap.width,
            height: bitmap.height,
            _texture: texture,
            _view: view,
            uniform,
            bind_group,
        }
    }
}

#[cfg(feature = "desktop-gpu")]
fn solid_bitmap(color: [u8; 4]) -> RgbaBitmap {
    RgbaBitmap {
        pixels: Arc::from(color),
        width: 1,
        height: 1,
    }
}

#[cfg(feature = "desktop-gpu")]
fn volume_fill(volume: f64) -> u32 {
    ((volume / DESKTOP_VOLUME_MAX).clamp(0.0, 1.0) * 224.0) as u32
}

#[cfg(feature = "desktop-gpu")]
fn volume_bitmap(fill: u32) -> RgbaBitmap {
    let width = 240_u32;
    let height = 28_u32;
    let mut pixels = vec![0; width as usize * height as usize * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[16, 18, 20, 220]);
    }
    for y in 11..17 {
        for x in 8..width - 8 {
            let offset = ((y * width + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(if x - 8 < fill {
                &[238, 238, 238, 255]
            } else {
                &[78, 86, 96, 255]
            });
        }
    }
    RgbaBitmap {
        pixels: pixels.into(),
        width,
        height,
    }
}

#[cfg(feature = "desktop-gpu")]
fn write_rgba_texture(queue: &wgpu::Queue, texture: &wgpu::Texture, bitmap: &RgbaBitmap) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bitmap.pixels.as_ref(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bitmap.width * 4),
            rows_per_image: Some(bitmap.height),
        },
        wgpu::Extent3d {
            width: bitmap.width,
            height: bitmap.height,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(feature = "desktop-gpu")]
const SPRITE_SHADER: &str = r#"
struct Parameters { transform: vec4<f32>, opacity: vec4<f32> };
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;
@group(0) @binding(2) var<uniform> parameters: Parameters;
struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let position = positions[index];
    var output: VertexOutput;
    output.position = vec4<f32>(position * parameters.transform.xy + parameters.transform.zw, 0.0, 1.0);
    output.uv = vec2<f32>((position.x + 1.0) * 0.5, (1.0 - position.y) * 0.5);
    return output;
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(image, image_sampler, input.uv);
    let encoded = select(linear_to_srgb(color.rgb), color.rgb, parameters.opacity.y > 0.5);
    return vec4<f32>(encoded, color.a * parameters.opacity.x);
}
fn linear_to_srgb(value: vec3<f32>) -> vec3<f32> {
    return select(
        1.055 * pow(value, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055),
        value * 12.92,
        value <= vec3<f32>(0.0031308),
    );
}
"#;

#[cfg(test)]
mod tests {
    use naga::{
        front::wgsl,
        valid::{Capabilities, ValidationFlags, Validator},
    };

    use super::{
        SPRITE_SHADER, YUV_MATRIX_PARAMETER_END, YUV_PARAMETER_COUNT, YUV_SCALE_PARAMETER_END,
        YUV_SHADER, set_yuv_scale_parameters,
    };

    fn validate_wgsl(source: &str) {
        let module = wgsl::parse_str(source).expect("WGSL shader parses");
        Validator::new(ValidationFlags::all(), Capabilities::all())
            .validate(&module)
            .expect("WGSL shader validates");
    }

    #[test]
    fn yuv_shader_is_valid_wgsl() {
        validate_wgsl(YUV_SHADER);
    }

    #[test]
    fn sprite_shader_is_valid_wgsl() {
        validate_wgsl(SPRITE_SHADER);
    }

    #[test]
    fn scale_parameters_only_fill_the_scale_uniform_slot() {
        let mut parameters = [-1.0; YUV_PARAMETER_COUNT];
        set_yuv_scale_parameters(&mut parameters, [1.0, 2.0, 3.0, 4.0]);

        assert!(
            parameters[..YUV_MATRIX_PARAMETER_END]
                .iter()
                .all(|value| *value == -1.0)
        );
        assert_eq!(
            parameters[YUV_MATRIX_PARAMETER_END..YUV_SCALE_PARAMETER_END],
            [1.0, 2.0, 3.0, 4.0]
        );
        assert!(
            parameters[YUV_SCALE_PARAMETER_END..]
                .iter()
                .all(|value| *value == -1.0)
        );
    }
}
