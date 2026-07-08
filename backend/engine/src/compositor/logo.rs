use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{codec, frame, media, software::scaling, util::format::pixel::Pixel};

use crate::{
    compositor::overlay::{OverlayRef, blend_overlay},
    utils::{
        config::LogoConfig,
        helper::{even, open_media_input},
    },
};

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

        let mut ictx = open_media_input(&config.path)
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

    fn as_overlay(&self) -> OverlayRef<'_> {
        OverlayRef {
            frame: &self.frame,
            x: self.x as i32,
            y: self.y as i32,
            width: self.width,
            height: self.height,
            opacity: self.opacity,
        }
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
    blend_overlay(target, logo.as_overlay(), opacity_factor);
}
