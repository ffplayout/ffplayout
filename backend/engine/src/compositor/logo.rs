use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{codec, format, frame, media, software::scaling, util::format::pixel::Pixel};

use crate::utils::{config::LogoConfig, helper::even};

pub struct LogoOverlay {
    pub frame: frame::Video, // YUVA420P
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub opacity: u8, // 0..255
}

impl LogoOverlay {
    pub fn load(config: &LogoConfig, output_width: u32, output_height: u32) -> Result<Self> {
        if !(0.0..=1.0).contains(&config.opacity) || !config.opacity.is_finite() {
            return Err(anyhow!("logo opacity must be between 0.0 and 1.0"));
        }

        let mut ictx = format::input(&config.path)
            .with_context(|| format!("failed to open logo {}", config.path))?;

        let stream = ictx
            .streams()
            .best(media::Type::Video)
            .ok_or_else(|| anyhow!("logo {} contains no video/image stream", config.path))?;

        let stream_index = stream.index();
        let ctx = codec::context::Context::from_parameters(stream.parameters())?;
        let mut decoder = ctx.decoder().video()?;

        let input_width = decoder.width();
        let input_height = decoder.height();

        let (width, height) = logo_dimensions(
            config.scale.as_deref(),
            input_width,
            input_height,
            output_width,
            output_height,
        )?;

        // Convert the logo directly to YUVA420P.
        // Plane 0 = Y
        // Plane 1 = U
        // Plane 2 = V
        // Plane 3 = Alpha
        let mut scaler = scaling::Context::get(
            decoder.format(),
            input_width,
            input_height,
            Pixel::YUVA420P,
            width,
            height,
            scaling::flag::Flags::BILINEAR,
        )?;

        let mut decoded = frame::Video::empty();
        let mut yuva = None;

        for (packet_stream, packet) in ictx.packets() {
            if packet_stream.index() != stream_index {
                continue;
            }

            decoder.send_packet(&packet)?;

            if decoder.receive_frame(&mut decoded).is_ok() {
                let mut scaled = frame::Video::empty();
                scaler.run(&decoded, &mut scaled)?;
                yuva = Some(scaled);
                break;
            }
        }

        if yuva.is_none() {
            decoder.send_eof()?;
            if decoder.receive_frame(&mut decoded).is_ok() {
                let mut scaled = frame::Video::empty();
                scaler.run(&decoded, &mut scaled)?;
                yuva = Some(scaled);
            }
        }

        let frame = yuva.ok_or_else(|| anyhow!("logo {} produced no frame", config.path))?;
        let (x, y) = logo_position(&config.position, output_width, output_height, width, height)?;

        Ok(Self {
            frame,
            x: even(x),
            y: even(y),
            width,
            height,
            opacity: (config.opacity * 255.0).round().clamp(0.0, 255.0) as u8,
        })
    }
}

fn logo_dimensions(
    scale: Option<&str>,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
) -> Result<(u32, u32)> {
    let Some(scale) = scale.filter(|scale| !scale.trim().is_empty()) else {
        return Ok((even(input_width).max(2), even(input_height).max(2)));
    };
    let (width, height) = scale
        .split_once(':')
        .or_else(|| scale.split_once('x'))
        .ok_or_else(|| anyhow!("logo scale must use WIDTH:HEIGHT or WIDTHxHEIGHT"))?;
    let width = parse_logo_dimension(width, input_width, output_width)?;
    let height = parse_logo_dimension(height, input_height, output_height)?;

    let (width, height) = match (width, height) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (
            width,
            ((u64::from(width) * u64::from(input_height)) / u64::from(input_width)) as u32,
        ),
        (None, Some(height)) => (
            ((u64::from(height) * u64::from(input_width)) / u64::from(input_height)) as u32,
            height,
        ),
        (None, None) => (input_width, input_height),
    };

    Ok((even(width).max(2), even(height).max(2)))
}

fn parse_logo_dimension(value: &str, input: u32, output: u32) -> Result<Option<u32>> {
    let value = value.trim();
    if value == "-1" {
        return Ok(None);
    }
    if value == "iw" || value == "ih" {
        return Ok(Some(input));
    }
    if value == "W" || value == "H" || value == "main_w" || value == "main_h" {
        return Ok(Some(output));
    }
    value
        .parse::<u32>()
        .map(Some)
        .map_err(|_| anyhow!("unsupported logo scale expression {value:?}"))
}

fn logo_position(
    position: &str,
    output_width: u32,
    output_height: u32,
    logo_width: u32,
    logo_height: u32,
) -> Result<(u32, u32)> {
    let (x, y) = position
        .split_once(':')
        .ok_or_else(|| anyhow!("logo position must use X:Y"))?;
    let x = eval_position_expr(x, output_width, logo_width)?;
    let y = eval_position_expr(y, output_height, logo_height)?;
    Ok((
        x.clamp(0, i64::from(output_width.saturating_sub(logo_width))) as u32,
        y.clamp(0, i64::from(output_height.saturating_sub(logo_height))) as u32,
    ))
}

fn eval_position_expr(expr: &str, main: u32, overlay: u32) -> Result<i64> {
    let normalized = expr
        .replace("main_w", "M")
        .replace("main_h", "M")
        .replace("overlay_w", "O")
        .replace("overlay_h", "O")
        .replace(['W', 'H'], "M")
        .replace(['w', 'h'], "O")
        .replace('-', "+-");
    let mut total = 0_i64;
    for part in normalized.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (sign, part) = part
            .strip_prefix('-')
            .map_or((1_i64, part), |part| (-1_i64, part));
        let value = match part {
            "M" => i64::from(main),
            "O" => i64::from(overlay),
            _ => part
                .parse::<i64>()
                .map_err(|_| anyhow!("unsupported logo position expression {expr:?}"))?,
        };
        total += sign * value;
    }
    Ok(total)
}

pub fn blend_logo(target: &mut frame::Video, logo: &LogoOverlay, opacity_factor: f64) {
    // Expected formats:
    // target: YUV420P
    // logo.frame: YUVA420P
    if opacity_factor <= 0.0 {
        return;
    }
    let opacity = ((logo.opacity as f64) * opacity_factor.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8;

    let logo_y = logo.frame.data(0);
    let logo_u = logo.frame.data(1);
    let logo_v = logo.frame.data(2);
    let logo_a = logo.frame.data(3);

    let logo_y_stride = logo.frame.stride(0);
    let logo_u_stride = logo.frame.stride(1);
    let logo_v_stride = logo.frame.stride(2);
    let logo_a_stride = logo.frame.stride(3);

    let target_y_stride = target.stride(0);
    let target_u_stride = target.stride(1);
    let target_v_stride = target.stride(2);

    let target_y_len = target.data(0).len();
    let target_u_len = target.data(1).len();
    let target_v_len = target.data(2).len();

    let width = logo.width as usize;
    let height = logo.height as usize;
    let logo_x = logo.x as usize;
    let logo_y_pos = logo.y as usize;

    // Luma: blend each pixel.
    for y in 0..height {
        for x in 0..width {
            let src_y_idx = y * logo_y_stride + x;
            let src_a_idx = y * logo_a_stride + x;

            if src_y_idx >= logo_y.len() || src_a_idx >= logo_a.len() {
                continue;
            }

            let alpha = mul_alpha(logo_a[src_a_idx], opacity);
            if alpha == 0 {
                continue;
            }

            let tx = logo_x + x;
            let ty = logo_y_pos + y;
            let dst_idx = ty * target_y_stride + tx;

            if dst_idx >= target_y_len {
                continue;
            }

            let dst = target.data(0)[dst_idx];
            let src = logo_y[src_y_idx];

            target.data_mut(0)[dst_idx] = blend_u8(dst, src, alpha);
        }
    }

    // Chroma: blend one sample per 2x2 block for YUV420P.
    let uv_width = width / 2;
    let uv_height = height / 2;
    let target_uv_x = logo_x / 2;
    let target_uv_y = logo_y_pos / 2;

    for y in 0..uv_height {
        for x in 0..uv_width {
            let src_u_idx = y * logo_u_stride + x;
            let src_v_idx = y * logo_v_stride + x;

            if src_u_idx >= logo_u.len() || src_v_idx >= logo_v.len() {
                continue;
            }

            // Average alpha for the 2x2 luma block.
            let ax = x * 2;
            let ay = y * 2;

            let alpha = avg_alpha_2x2(logo_a, logo_a_stride, ax, ay, width, height, opacity);

            if alpha == 0 {
                continue;
            }

            let dst_x = target_uv_x + x;
            let dst_y = target_uv_y + y;

            let dst_u_idx = dst_y * target_u_stride + dst_x;
            let dst_v_idx = dst_y * target_v_stride + dst_x;

            if dst_u_idx >= target_u_len || dst_v_idx >= target_v_len {
                continue;
            }

            let dst_u = target.data(1)[dst_u_idx];
            let dst_v = target.data(2)[dst_v_idx];

            target.data_mut(1)[dst_u_idx] = blend_u8(dst_u, logo_u[src_u_idx], alpha);
            target.data_mut(2)[dst_v_idx] = blend_u8(dst_v, logo_v[src_v_idx], alpha);
        }
    }
}

#[inline]
fn blend_u8(dst: u8, src: u8, alpha: u8) -> u8 {
    let a = alpha as u16;
    (((dst as u16 * (255 - a)) + (src as u16 * a) + 127) / 255) as u8
}

#[inline]
fn mul_alpha(src_alpha: u8, opacity: u8) -> u8 {
    ((src_alpha as u16 * opacity as u16 + 127) / 255) as u8
}

fn avg_alpha_2x2(
    alpha_plane: &[u8],
    alpha_stride: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    opacity: u8,
) -> u8 {
    let mut sum = 0_u16;
    let mut count = 0_u16;

    for dy in 0..2 {
        for dx in 0..2 {
            let px = x + dx;
            let py = y + dy;

            if px >= width || py >= height {
                continue;
            }

            let idx = py * alpha_stride + px;
            if idx >= alpha_plane.len() {
                continue;
            }

            sum += alpha_plane[idx] as u16;
            count += 1;
        }
    }

    if count == 0 {
        return 0;
    }

    let avg = (sum / count) as u8;
    mul_alpha(avg, opacity)
}
