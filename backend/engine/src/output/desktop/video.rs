use std::{
    mem,
    ops::Deref,
    sync::{Arc, Mutex, PoisonError, Weak},
};

use anyhow::{Result, anyhow};
#[cfg(feature = "desktop-gpu")]
use ffmpeg_next::util::color;
use ffmpeg_next::{frame, software::scaling, util::format::pixel::Pixel};

const VIDEO_BUFFER_POOL_CAPACITY: usize = 6;

pub(super) struct RecyclableBuffer<T> {
    data: Vec<T>,
    pool: Weak<Mutex<Vec<Vec<T>>>>,
}

impl<T> RecyclableBuffer<T> {
    fn pooled(data: Vec<T>, pool: &Arc<Mutex<Vec<Vec<T>>>>) -> Self {
        Self {
            data,
            pool: Arc::downgrade(pool),
        }
    }

    #[cfg(all(test, feature = "desktop-cpu", not(feature = "desktop-gpu")))]
    pub(super) fn unpooled(data: Vec<T>) -> Self {
        Self {
            data,
            pool: Weak::new(),
        }
    }
}

impl<T> Deref for RecyclableBuffer<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T> Drop for RecyclableBuffer<T> {
    fn drop(&mut self) {
        let Some(pool) = self.pool.upgrade() else {
            return;
        };
        let mut pool = pool.lock().unwrap_or_else(PoisonError::into_inner);

        if pool.len() < VIDEO_BUFFER_POOL_CAPACITY {
            pool.push(mem::take(&mut self.data));
        }
    }
}

fn take_buffer<T: Clone>(pool: &Arc<Mutex<Vec<Vec<T>>>>, len: usize, value: T) -> Vec<T> {
    let mut pool = pool.lock().unwrap_or_else(PoisonError::into_inner);
    let best_fit = pool
        .iter()
        .enumerate()
        .filter(|(_, buffer)| buffer.capacity() >= len)
        .min_by_key(|(_, buffer)| buffer.capacity())
        .map(|(index, _)| index)
        .or_else(|| {
            pool.iter()
                .enumerate()
                .max_by_key(|(_, buffer)| buffer.capacity())
                .map(|(index, _)| index)
        });
    let mut buffer = best_fit
        .map(|index| pool.swap_remove(index))
        .unwrap_or_default();
    drop(pool);
    buffer.resize(len, value);
    buffer
}

#[derive(Clone)]
pub(super) struct VideoSurface {
    pub(super) width: u32,
    pub(super) height: u32,
    #[cfg(feature = "desktop-gpu")]
    pub(super) y: Arc<RecyclableBuffer<u8>>,
    #[cfg(feature = "desktop-gpu")]
    pub(super) u: Arc<RecyclableBuffer<u8>>,
    #[cfg(feature = "desktop-gpu")]
    pub(super) v: Arc<RecyclableBuffer<u8>>,
    #[cfg(feature = "desktop-gpu")]
    pub(super) color_space: color::Space,
    #[cfg(feature = "desktop-gpu")]
    pub(super) color_range: color::Range,
    #[cfg(feature = "desktop-gpu")]
    pub(super) color_primaries: color::Primaries,
    #[cfg(feature = "desktop-gpu")]
    pub(super) color_transfer: color::TransferCharacteristic,
    #[cfg(all(feature = "desktop-cpu", not(feature = "desktop-gpu")))]
    pub(super) pixels: Arc<RecyclableBuffer<u32>>,
    pub(super) pts: i64,
}

pub(super) struct DesktopFrameConverter {
    scaler: Option<scaling::Context>,
    converted: frame::Video,
    #[cfg(feature = "desktop-gpu")]
    buffer_pool: Arc<Mutex<Vec<Vec<u8>>>>,
    #[cfg(all(feature = "desktop-cpu", not(feature = "desktop-gpu")))]
    buffer_pool: Arc<Mutex<Vec<Vec<u32>>>>,
}

impl Default for DesktopFrameConverter {
    fn default() -> Self {
        Self {
            scaler: None,
            converted: frame::Video::empty(),
            buffer_pool: Arc::new(Mutex::new(Vec::with_capacity(VIDEO_BUFFER_POOL_CAPACITY))),
        }
    }
}

impl DesktopFrameConverter {
    pub(super) fn convert(&mut self, frame: &frame::Video) -> Result<VideoSurface> {
        let width = frame.width();
        let height = frame.height();

        if width == 0 || height == 0 {
            return Err(anyhow!("desktop video frame has zero dimensions"));
        }

        let reconfigure = self.scaler.as_ref().is_none_or(|scaler| {
            let input = scaler.input();
            input.format != frame.format() || input.width != width || input.height != height
        });

        if reconfigure {
            if let Some(scaler) = &mut self.scaler {
                scaler.cached(
                    frame.format(),
                    width,
                    height,
                    output_pixel_format(),
                    width,
                    height,
                    scaling::flag::Flags::FAST_BILINEAR,
                );
            } else {
                self.scaler = Some(scaling::Context::get(
                    frame.format(),
                    width,
                    height,
                    output_pixel_format(),
                    width,
                    height,
                    scaling::flag::Flags::FAST_BILINEAR,
                )?);
            }

            self.converted = frame::Video::empty();
        }
        self.scaler
            .as_mut()
            .expect("desktop scaler must be initialized")
            .run(frame, &mut self.converted)?;

        #[cfg(all(feature = "desktop-cpu", not(feature = "desktop-gpu")))]
        {
            let mut pixels =
                take_buffer(&self.buffer_pool, width as usize * height as usize, 0_u32);
            let stride = self.converted.stride(0) / 4;

            for (target_row, source_row) in pixels
                .chunks_exact_mut(width as usize)
                .zip(self.converted.plane::<[u8; 4]>(0).chunks_exact(stride))
            {
                for (target, source) in target_row.iter_mut().zip(source_row) {
                    *target = bgrz_to_rgb_pixel(*source);
                }
            }

            Ok(VideoSurface {
                width,
                height,
                pixels: Arc::new(RecyclableBuffer::pooled(pixels, &self.buffer_pool)),
                pts: frame.pts().unwrap_or_default(),
            })
        }

        #[cfg(feature = "desktop-gpu")]
        {
            let chroma_width = width.div_ceil(2) as usize;
            let chroma_height = height.div_ceil(2) as usize;

            Ok(VideoSurface {
                width,
                height,
                y: copy_frame_plane(
                    &self.converted,
                    0,
                    width as usize,
                    height as usize,
                    &self.buffer_pool,
                ),
                u: copy_frame_plane(
                    &self.converted,
                    1,
                    chroma_width,
                    chroma_height,
                    &self.buffer_pool,
                ),
                v: copy_frame_plane(
                    &self.converted,
                    2,
                    chroma_width,
                    chroma_height,
                    &self.buffer_pool,
                ),
                color_space: frame.color_space(),
                color_range: frame.color_range(),
                color_primaries: frame.color_primaries(),
                color_transfer: frame.color_transfer_characteristic(),
                pts: frame.pts().unwrap_or_default(),
            })
        }
    }
}

#[cfg(all(feature = "desktop-cpu", not(feature = "desktop-gpu")))]
fn output_pixel_format() -> Pixel {
    Pixel::BGRZ
}

#[cfg(feature = "desktop-gpu")]
fn output_pixel_format() -> Pixel {
    Pixel::YUV420P
}

#[cfg(feature = "desktop-gpu")]
fn copy_frame_plane(
    frame: &frame::Video,
    plane: usize,
    width: usize,
    height: usize,
    pool: &Arc<Mutex<Vec<Vec<u8>>>>,
) -> Arc<RecyclableBuffer<u8>> {
    let stride = frame.stride(plane);
    let source = frame.data(plane);
    let mut pixels = take_buffer(pool, width * height, 0);

    for (target, source) in pixels
        .chunks_exact_mut(width)
        .zip(source.chunks_exact(stride))
    {
        target.copy_from_slice(&source[..width]);
    }
    Arc::new(RecyclableBuffer::pooled(pixels, pool))
}

#[cfg(all(feature = "desktop-cpu", not(feature = "desktop-gpu")))]
pub(super) fn bgrz_to_rgb_pixel([blue, green, red, _]: [u8; 4]) -> u32 {
    (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_returns_to_pool_after_last_reference_is_dropped() {
        let pool = Arc::new(Mutex::new(Vec::new()));
        let buffer = Arc::new(RecyclableBuffer::pooled(vec![1_u8; 16], &pool));
        let second_reference = Arc::clone(&buffer);

        drop(buffer);
        assert!(pool.lock().expect("video buffer pool lock").is_empty());

        drop(second_reference);
        let pool = pool.lock().expect("video buffer pool lock");
        assert_eq!(pool.len(), 1);
        assert_eq!(pool[0].len(), 16);
    }

    #[test]
    fn buffer_pool_uses_smallest_sufficient_buffer() {
        let pool = Arc::new(Mutex::new(vec![
            Vec::<u8>::with_capacity(64),
            Vec::<u8>::with_capacity(32),
        ]));

        let buffer = take_buffer(&pool, 24, 0_u8);

        assert_eq!(buffer.capacity(), 32);
        assert_eq!(buffer.len(), 24);
    }

    #[test]
    fn buffer_pool_is_bounded() {
        let pool = Arc::new(Mutex::new(Vec::new()));

        for _ in 0..VIDEO_BUFFER_POOL_CAPACITY + 2 {
            drop(RecyclableBuffer::pooled(vec![0_u8; 4], &pool));
        }

        assert_eq!(
            pool.lock().expect("video buffer pool lock").len(),
            VIDEO_BUFFER_POOL_CAPACITY
        );
    }
}
