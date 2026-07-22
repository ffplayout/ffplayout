#[cfg(not(feature = "desktop-cpu"))]
use std::sync::Arc;

use anyhow::{Result, anyhow};
use ffmpeg_next::util::color;
use pixels::{Pixels, SurfaceTexture, wgpu};
use winit::window::Window;

use super::{
    DESKTOP_VOLUME_MAX,
    graphics::RgbaBitmap,
    render::{Rect, WindowFrame, fit_rect, help_panel_rect, logo_rect, subtitle_rect},
    video::VideoSurface,
};

pub(super) type WindowRenderer = PixelsRenderer;

#[cfg(not(feature = "desktop-cpu"))]
pub(super) struct PixelsRenderer {
    pixels: Pixels<'static>,
    window: Arc<Window>,
    yuv: GpuYuvRenderer,
    sprites: GpuSpriteRenderer,
}

#[cfg(not(feature = "desktop-cpu"))]
impl PixelsRenderer {
    pub(super) fn new(window: Arc<Window>, _width: u32, _height: u32) -> Result<Self> {
        let size = window.inner_size();
        let surface =
            SurfaceTexture::new(size.width.max(1), size.height.max(1), Arc::clone(&window));
        let pixels = Pixels::new(1, 1, surface).map_err(|error| anyhow!("{error}"))?;
        let yuv = GpuYuvRenderer::new(pixels.context(), pixels.surface_texture_format());
        let sprites = GpuSpriteRenderer::new(pixels.context(), pixels.surface_texture_format());
        Ok(Self {
            pixels,
            window,
            yuv,
            sprites,
        })
    }

    pub(super) fn resize_surface(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.pixels
            .resize_surface(width, height)
            .map_err(|error| anyhow!("{error}"))
    }

    pub(super) fn resize_buffer(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let _ = (width, height);
        Ok(())
    }

    pub(super) fn render(&mut self, frame: &WindowFrame, size: (u32, u32)) -> Result<()> {
        if let Some(video) = &frame.video {
            self.yuv.upload(video)?;
        }
        // Winit needs this notification before presenting on Wayland. It
        // requests the compositor frame callback that releases the next
        // redraw, including after a surface resize.
        self.window.pre_present_notify();
        self.pixels
            .render_with(|encoder, target, _| {
                self.yuv.render(encoder, target, size);
                self.sprites.render(encoder, target, frame, size);
                Ok(())
            })
            .map_err(|error| anyhow!("{error}"))
    }
}

#[cfg(not(feature = "desktop-cpu"))]
struct GpuYuvRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    parameters: wgpu::Buffer,
    parameter_values: [f32; 20],
    output_is_srgb: bool,
    textures: Option<GpuYuvTextures>,
    last_pts: Option<i64>,
}

#[cfg(not(feature = "desktop-cpu"))]
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

#[cfg(not(feature = "desktop-cpu"))]
impl GpuYuvRenderer {
    fn new(context: &pixels::PixelsContext<'_>, surface_format: wgpu::TextureFormat) -> Self {
        let device = context.device.clone();
        let queue = context.queue.clone();
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
                        min_binding_size: wgpu::BufferSize::new(80),
                    },
                    count: None,
                },
            ],
        });
        let parameters = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ffplayout_yuv_parameters"),
            size: 80,
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
            parameter_values: [0.0; 20],
            output_is_srgb: surface_format.is_srgb(),
            textures: None,
            last_pts: None,
        }
    }

    fn upload(&mut self, video: &VideoSurface) -> Result<()> {
        let recreate = self.textures.as_ref().is_none_or(|textures| {
            textures.width != video.width || textures.height != video.height
        });
        if recreate {
            self.textures = Some(self.create_textures(video.width, video.height));
            self.last_pts = None;
        }
        if self.last_pts == Some(video.pts) {
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
        self.parameter_values[..16]
            .copy_from_slice(&yuv_color_parameters(video.color_space, video.color_range));
        self.last_pts = Some(video.pts);
        Ok(())
    }

    fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        surface_size: (u32, u32),
    ) {
        if let Some(textures) = &self.textures {
            let rect = fit_rect(
                textures.width,
                textures.height,
                surface_size.0,
                surface_size.1,
            );
            self.parameter_values[16..].copy_from_slice(&[
                rect.width as f32 / surface_size.0.max(1) as f32,
                rect.height as f32 / surface_size.1.max(1) as f32,
                0.0,
                if self.output_is_srgb { 1.0 } else { 0.0 },
            ]);
            self.queue
                .write_buffer(&self.parameters, 0, &f32_bytes(&self.parameter_values));
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
        if let Some(textures) = &self.textures {
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

#[cfg(not(feature = "desktop-cpu"))]
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

#[cfg(not(feature = "desktop-cpu"))]
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

#[cfg(not(feature = "desktop-cpu"))]
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

#[cfg(not(feature = "desktop-cpu"))]
pub(super) fn yuv_color_parameters(space: color::Space, range: color::Range) -> [f32; 16] {
    let (matrix, offset) = match (space, range) {
        (color::Space::BT709 | color::Space::BT2020NCL, color::Range::JPEG) => (
            [
                1.0, 1.0, 1.0, 0.0, 0.0, -0.1873, 1.8556, 0.0, 1.5748, -0.4681, 0.0, 0.0,
            ],
            [0.0, -0.5, -0.5, 0.0],
        ),
        (color::Space::BT709 | color::Space::BT2020NCL, _) => (
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

#[cfg(not(feature = "desktop-cpu"))]
fn f32_bytes(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_ne_bytes())
        .collect()
}

#[cfg(not(feature = "desktop-cpu"))]
const YUV_SHADER: &str = r#"
struct Parameters {
    column0: vec4<f32>,
    column1: vec4<f32>,
    column2: vec4<f32>,
    offset: vec4<f32>,
    scale: vec4<f32>,
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
    // Y'CbCr conversion produces display-encoded RGB. An sRGB surface
    // encodes fragment output on write, so convert it back to linear light
    // first to avoid applying gamma twice.
    let linear = select(
        rgb,
        vec3<f32>(
            select(pow((rgb.r + 0.099) / 1.099, 1.0 / 0.45), rgb.r / 4.5, rgb.r < 0.081),
            select(pow((rgb.g + 0.099) / 1.099, 1.0 / 0.45), rgb.g / 4.5, rgb.g < 0.081),
            select(pow((rgb.b + 0.099) / 1.099, 1.0 / 0.45), rgb.b / 4.5, rgb.b < 0.081),
        ),
        parameters.scale.w > 0.5,
    );
    return vec4<f32>(linear, 1.0);
}
"#;

#[cfg(not(feature = "desktop-cpu"))]
struct GpuSpriteRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

#[cfg(not(feature = "desktop-cpu"))]
impl GpuSpriteRenderer {
    fn new(context: &pixels::PixelsContext<'_>, surface_format: wgpu::TextureFormat) -> Self {
        let device = context.device.clone();
        let queue = context.queue.clone();
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
        }
    }

    fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        frame: &WindowFrame,
        size: (u32, u32),
    ) {
        if size.0 == 0 || size.1 == 0 {
            return;
        }
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
            self.draw_bitmap(&mut pass, &logo.bitmap, rect, logo.opacity, size);
        }
        if let Some(subtitle) = &frame.subtitle {
            self.draw_bitmap(
                &mut pass,
                subtitle,
                subtitle_rect(subtitle, size),
                255,
                size,
            );
        }
        if frame.volume_overlay {
            let bitmap = volume_bitmap(frame.volume);
            self.draw_bitmap(
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
            self.draw_bitmap(
                &mut pass,
                &solid_bitmap([16, 18, 20, 205]),
                panel,
                255,
                size,
            );
            self.draw_bitmap(
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

    fn draw_bitmap(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        bitmap: &RgbaBitmap,
        rect: Rect,
        opacity: u8,
        surface_size: (u32, u32),
    ) {
        if bitmap.width == 0 || bitmap.height == 0 || rect.width == 0 || rect.height == 0 {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
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
        write_rgba_texture(&self.queue, &texture, bitmap);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let values = [
            rect.width as f32 / surface_size.0 as f32,
            rect.height as f32 / surface_size.1 as f32,
            (rect.x as f32 + rect.width as f32 * 0.5) * 2.0 / surface_size.0 as f32 - 1.0,
            1.0 - (rect.y as f32 + rect.height as f32 * 0.5) * 2.0 / surface_size.1 as f32,
            opacity as f32 / 255.0,
            0.0,
            0.0,
            0.0,
        ];
        let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ffplayout_sprite_parameters"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&uniform, 0, &f32_bytes(&values));
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ffplayout_sprite_bind_group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..6, 0..1);
    }
}

#[cfg(not(feature = "desktop-cpu"))]
fn solid_bitmap(color: [u8; 4]) -> RgbaBitmap {
    RgbaBitmap {
        pixels: color.to_vec(),
        width: 1,
        height: 1,
    }
}

#[cfg(not(feature = "desktop-cpu"))]
fn volume_bitmap(volume: f64) -> RgbaBitmap {
    let width = 240_u32;
    let height = 28_u32;
    let mut pixels = vec![0; width as usize * height as usize * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[16, 18, 20, 220]);
    }
    let fill = ((volume / DESKTOP_VOLUME_MAX).clamp(0.0, 1.0) * f64::from(width - 16)) as u32;
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
        pixels,
        width,
        height,
    }
}

#[cfg(not(feature = "desktop-cpu"))]
fn write_rgba_texture(queue: &wgpu::Queue, texture: &wgpu::Texture, bitmap: &RgbaBitmap) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bitmap.pixels,
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

#[cfg(not(feature = "desktop-cpu"))]
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
    return vec4<f32>(color.rgb, color.a * parameters.opacity.x);
}
"#;
