//! Bounded, static PNG preparation. Call on a preparation worker, not per frame.
use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use ffmpeg_next::{format::Pixel, frame, software::scaling};
use image::{DynamicImage, ImageDecoder, Limits, codecs::png::PngDecoder};

use crate::{
    bitmap::StraightRgbaBitmap,
    surface::{ColorEncoding, Resource, ResourceKey},
};

use super::{scene::CpuScene, text::rgba_to_yuva420p};

impl CpuScene {
    /// Odd native sizes are padded transparently at the right/bottom for YUVA420.
    /// Explicit dimensions refer to the prepared surface and must be even.
    pub fn prepare_png(
        &mut self,
        key: ResourceKey,
        path: &Path,
        size: Option<(u32, u32)>,
    ) -> Result<Arc<Resource>> {
        if let Some((width, height)) = size {
            ensure!(
                width > 0
                    && height > 0
                    && width <= 8192
                    && height <= 8192
                    && width.is_multiple_of(2)
                    && height.is_multiple_of(2),
                "PNG size must be even and within 2..8192 pixels"
            );
            ensure!(
                u64::from(width) * u64::from(height) * 3 <= 64 * 1024 * 1024,
                "prepared PNG exceeds 64 MiB"
            );
        }
        const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
        let metadata = path.metadata()?;
        ensure!(metadata.is_file(), "graphics source must be a regular file");
        ensure!(
            metadata.len() <= MAX_FILE_BYTES,
            "graphic PNG exceeds 16 MiB"
        );
        let file = File::open(path).context("opening graphic PNG")?;
        ensure!(
            file.metadata()?.is_file(),
            "graphics source must be a regular file"
        );
        let mut encoded = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut encoded)?;
        ensure!(
            encoded.len() as u64 <= MAX_FILE_BYTES,
            "graphic PNG exceeds 16 MiB"
        );
        let mut limits = Limits::default();
        limits.max_alloc = Some(64 * 1024 * 1024);
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        let decoder = PngDecoder::with_limits(Cursor::new(encoded), limits)?;
        ensure!(
            !decoder.is_apng()?,
            "animated PNG is not supported for playlist graphics yet"
        );
        let (width, height) = decoder.dimensions();
        let width = (width + 1) & !1;
        let height = (height + 1) & !1;
        ensure!(
            u64::from(width) * u64::from(height) * 4 <= 64 * 1024 * 1024,
            "decoded PNG exceeds 64 MiB"
        );
        let rgba = DynamicImage::from_decoder(decoder)?.into_rgba8();
        let mut pixels = vec![0; width as usize * height as usize * 4];
        for (y, row) in rgba.rows().enumerate() {
            let offset = y * width as usize * 4;
            for (x, pixel) in row.enumerate() {
                pixels[offset + x * 4..offset + x * 4 + 4].copy_from_slice(&pixel.0);
            }
        }
        let bitmap = StraightRgbaBitmap {
            width,
            height,
            pixels,
        };
        let mut prepared = rgba_to_yuva420p(&bitmap)?;
        if let Some((width, height)) = size
            && (width, height) != (bitmap.width, bitmap.height)
        {
            let mut scaler = scaling::Context::get(
                Pixel::YUVA420P,
                bitmap.width,
                bitmap.height,
                Pixel::YUVA420P,
                width,
                height,
                scaling::flag::Flags::BILINEAR,
            )?;
            let mut scaled = frame::Video::empty();
            scaler.run(&prepared, &mut scaled)?;
            prepared = scaled;
        }

        self.prepare_yuva(key, prepared, ColorEncoding::Bt601Limited)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use image::{Rgba, RgbaImage};

    use super::*;
    use crate::{
        playback::{Presentation, Transition},
        scene::{Layer, LayerId, LayerSpec, SceneRevision, Scope},
    };

    struct TestPng(std::path::PathBuf);

    impl TestPng {
        fn new(width: u32, height: u32) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ffplayout-graphic-png-{}-{}.png",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 180]))
                .save(&path)
                .unwrap();

            Self(path)
        }
    }

    impl Drop for TestPng {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn odd_native_png_is_padded_transparently_and_composited_without_alpha_duplication() {
        let file = TestPng::new(3, 3);
        let mut renderer = CpuScene::new(1, 64 * 1024);
        let key = ResourceKey { id: 1, revision: 1 };
        let resource = renderer.prepare_png(key, &file.0, None).unwrap();
        assert_eq!(
            (resource.descriptor.width, resource.descriptor.height),
            (4, 4)
        );
        let scene = SceneRevision {
            number: 1,
            layers: vec![Layer {
                spec: LayerSpec {
                    id: LayerId(1),
                    resource: key,
                    z: 0,
                    scope: Scope::default(),
                    canvas: (4, 4),
                    position: (0, 0),
                    opacity: 1.0,
                },
                resource,
                presentation: Presentation::new(0, Transition::default()).unwrap(),
                visible: true,
            }]
            .into(),
        };
        let mut target = frame::Video::new(Pixel::YUV420P, 4, 4);
        target.data_mut(0).fill(16);
        target.data_mut(1).fill(128);
        target.data_mut(2).fill(128);
        assert_eq!(
            renderer
                .blend(
                    &scene,
                    0,
                    Scope::default(),
                    &mut target,
                    ColorEncoding::Bt601Limited
                )
                .blended,
            1
        );
        for y in 0..4 {
            for x in 0..4 {
                let expected = if x < 3 && y < 3 { 171 } else { 16 };
                assert_eq!(target.data(0)[y * target.stride(0) + x], expected);
            }
        }
    }

    #[test]
    fn png_sizes_formats_and_resource_budgets_are_checked() {
        let file = TestPng::new(4, 4);
        let key = ResourceKey { id: 1, revision: 1 };
        let mut renderer = CpuScene::new(1, 64 * 1024);
        let prepared = renderer.prepare_png(key, &file.0, Some((8, 4))).unwrap();
        assert_eq!(
            (prepared.descriptor.width, prepared.descriptor.height),
            (8, 4)
        );
        for size in [(0, 0), (3, 4), (8194, 4), (8192, 8192)] {
            assert!(
                CpuScene::new(1, 64 * 1024)
                    .prepare_png(key, &file.0, Some(size))
                    .is_err()
            );
        }
        let huge = TestPng::new(8193, 1);
        assert!(
            CpuScene::new(1, 64 * 1024)
                .prepare_png(key, &huge.0, None)
                .is_err()
        );
        assert!(CpuScene::new(1, 1).prepare_png(key, &file.0, None).is_err());
        let gif = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_images/animated-logo.gif");
        assert!(
            CpuScene::new(1, 64 * 1024)
                .prepare_png(key, &gif, None)
                .is_err()
        );
        assert!(
            renderer
                .prepare_png(key, file.0.parent().unwrap(), None)
                .is_err()
        );
    }
}
