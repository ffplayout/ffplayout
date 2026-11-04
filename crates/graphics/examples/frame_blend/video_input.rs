//! Prepare benchmark video input outside the measured compositing path.
use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};

use super::color::{Color, Rect, Transfer, compose_blocks};

pub fn load_video(path: &Path, width: usize, height: usize, color: Color) -> Result<Vec<u8>> {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("yuv"))
    {
        let video = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        ensure!(
            video.len() == width * height * 3 / 2,
            "expected exactly one tightly packed YUV420p frame ({} bytes), got {}",
            width * height * 3 / 2,
            video.len()
        );

        return Ok(video);
    }

    let image = image::open(path)
        .with_context(|| format!("decoding video image {}", path.display()))?
        .into_rgb8();
    ensure!(
        image.dimensions() == (width as u32, height as u32),
        "video image dimensions must match the requested frame; no implicit rescaling"
    );
    let mut video = vec![0; width * height * 3 / 2];
    compose_blocks(
        &mut video,
        width,
        height,
        Rect::full(width, height),
        color,
        |x, y| {
            image.get_pixel(x as u32, y as u32).0.map(|value| {
                color
                    .transfer
                    .encode(Transfer::Srgb.decode(f64::from(value) / 255.0))
            })
        },
    );

    Ok(video)
}
