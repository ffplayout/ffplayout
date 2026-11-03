//! One tracked decoder per asset, coalesced per-presentation requests and ready pixels.
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::{Result, ensure};
use arc_swap::{ArcSwap, ArcSwapOption};
use ffmpeg_next::{Dictionary, format};

use crate::ffmpeg::{
    compositor::{metadata::yuva_descriptor, overlay::OverlayFrame},
    reference_video_frame,
    sources::decoder::LogoDecoder,
};
use crate::surface::ColorEncoding;
use crate::{
    preparation::{Cancellation, MediaTask, PreparationService, ResourceBudget},
    source::{FrameTiming, LoopDuration},
    style::LogoStyle,
};

use super::AnimationOptions;

type Instance = (u64, u64, i64);
type Frames = VecDeque<(FrameTiming, Arc<OverlayFrame>)>;
const LIMIT: usize = 64 * 1024 * 1024;
const LOOKAHEAD: i64 = 500_000;

struct Request {
    elapsed: AtomicI64,
    frame_epoch: AtomicI64,
    ready: ArcSwapOption<OverlayFrame>,
    ready_elapsed: AtomicI64,
}

struct Shared {
    requests: ArcSwap<BTreeMap<Instance, Arc<Request>>>,
    admission: Mutex<()>,
    duration: OnceLock<LoopDuration>,
    failed: AtomicBool,
    inactive: AtomicBool,
    frame_epoch: AtomicI64,
    #[cfg(test)]
    decoded: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    cached_bytes: std::sync::atomic::AtomicUsize,
}

pub(super) struct StreamSource {
    shared: Arc<Shared>,
    poster: Arc<OverlayFrame>,
    poster_timing: FrameTiming,
    options: AnimationOptions,
    task: MediaTask,
    wanted: AtomicBool,
}

impl StreamSource {
    pub fn start(
        path: &Path,
        style: LogoStyle,
        options: AnimationOptions,
        poster: Arc<OverlayFrame>,
        poster_timing: FrameTiming,
        service: &PreparationService,
    ) -> Result<Self> {
        let shared = Arc::new(Shared {
            requests: ArcSwap::from_pointee(BTreeMap::new()),
            admission: Mutex::new(()),
            duration: OnceLock::new(),
            failed: AtomicBool::new(false),
            inactive: AtomicBool::new(true),
            frame_epoch: AtomicI64::new(0),
            #[cfg(test)]
            decoded: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            cached_bytes: std::sync::atomic::AtomicUsize::new(0),
        });
        let state = shared.clone();
        let path = path.to_path_buf();
        let budget = service.budget();
        let frame_bytes = bytes(&poster);
        let task = service.spawn_media(move |stop| {
            if let Err(error) = run(path, style, options, &state, &budget, &stop, frame_bytes)
                && !stop.is_cancelled()
            {
                state.failed.store(true, Ordering::Release);
                log::warn!("Animated graphic decoder stopped; retaining ready pixels: {error:#}");
            }
        })?;

        Ok(Self {
            shared,
            poster,
            poster_timing,
            options,
            task,
            wanted: AtomicBool::new(false),
        })
    }

    #[cfg(test)]
    pub fn ready_at(&self, elapsed: i64, instance: Instance) -> bool {
        let elapsed = local_time(&self.shared, elapsed, self.options).unwrap_or(-1);

        self.shared
            .requests
            .load()
            .get(&instance)
            .is_some_and(|request| request.ready_elapsed.load(Ordering::Acquire) == elapsed)
    }

    #[cfg(test)]
    pub fn duration_known(&self) -> bool {
        self.shared.duration.get().is_some()
    }

    #[cfg(test)]
    pub fn stats(&self) -> (usize, usize) {
        (
            self.shared.decoded.load(Ordering::Acquire),
            self.shared.cached_bytes.load(Ordering::Acquire),
        )
    }

    pub fn begin_frame(&self, epoch: i64) {
        self.wanted.store(false, Ordering::Release);
        self.shared.frame_epoch.store(epoch, Ordering::Release);
    }

    pub fn end_frame(&self) {
        if !self.wanted.load(Ordering::Acquire) {
            self.suspend();
        }
    }

    pub fn suspend(&self) {
        self.begin_frame(i64::MIN);
        if !self.shared.inactive.swap(true, Ordering::AcqRel) {
            self.task.wake();
        }
    }

    pub fn stop(&self) {
        self.shared.requests.store(Arc::new(BTreeMap::new()));
        self.task.cancel();
    }

    pub fn at(&self, mut elapsed: i64, instance: Instance) -> Option<Arc<OverlayFrame>> {
        if elapsed < 0 {
            return None;
        }
        if let Some(duration) = self.shared.duration.get() {
            elapsed = duration.playback_position(
                elapsed,
                self.options.repeat,
                self.options.completion,
            )?;
        }
        self.wanted.store(true, Ordering::Release);
        if self.shared.inactive.swap(false, Ordering::AcqRel) {
            self.task.wake();
        }
        let requests = self.shared.requests.load();
        if let Some(request) = requests.get(&instance) {
            let epoch = self.shared.frame_epoch.load(Ordering::Acquire);
            if request.frame_epoch.swap(epoch, Ordering::AcqRel) != epoch
                && request.ready_elapsed.load(Ordering::Acquire) != elapsed
            {
                self.task.wake();
            }
            if request.elapsed.swap(elapsed, Ordering::AcqRel) != elapsed
                && !self.shared.failed.load(Ordering::Acquire)
            {
                self.task.wake();
            }

            return request.ready.load_full().or_else(|| {
                self.poster_timing
                    .contains(elapsed)
                    .then(|| self.poster.clone())
            });
        }
        // Only initial presentation admission allocates. The per-frame path
        // above consists of atomic access and ready-surface references.
        let Ok(_guard) = self.shared.admission.try_lock() else {
            return self
                .poster_timing
                .contains(elapsed)
                .then(|| self.poster.clone());
        };
        let mut requests = (**self.shared.requests.load()).clone();
        requests.retain(|key, _| (key.0, key.1) != (instance.0, instance.1));
        if requests.len() >= 32 {
            let epoch = self.shared.frame_epoch.load(Ordering::Acquire);
            let retired = requests
                .iter()
                .filter(|(_, request)| request.frame_epoch.load(Ordering::Acquire) != epoch)
                .min_by_key(|(_, request)| request.frame_epoch.load(Ordering::Acquire))
                .map(|(key, _)| *key);
            if let Some(retired) = retired {
                requests.remove(&retired);
            }
        }
        if requests.len() < 32 {
            requests.insert(
                instance,
                Arc::new(Request {
                    elapsed: AtomicI64::new(elapsed),
                    frame_epoch: AtomicI64::new(self.shared.frame_epoch.load(Ordering::Acquire)),
                    ready: ArcSwapOption::empty(),
                    ready_elapsed: AtomicI64::new(-1),
                }),
            );
            self.shared.requests.store(Arc::new(requests));
            self.task.wake();
        }

        self.poster_timing
            .contains(elapsed)
            .then(|| self.poster.clone())
    }
}

fn bytes(frame: &OverlayFrame) -> usize {
    (0..4)
        .map(|plane| frame.frame.data(plane).len())
        .sum::<usize>()
        + frame.chroma_alpha.len()
}

fn local_time(shared: &Shared, elapsed: i64, options: AnimationOptions) -> Option<i64> {
    shared.duration.get().map_or(Some(elapsed), |duration| {
        duration.playback_position(elapsed, options.repeat, options.completion)
    })
}

fn run(
    path: PathBuf,
    style: LogoStyle,
    options: AnimationOptions,
    shared: &Shared,
    budget: &ResourceBudget,
    stop: &Cancellation,
    frame_bytes: usize,
) -> Result<()> {
    let mut input_options = Dictionary::new();
    input_options.set("protocol_whitelist", "file");
    input_options.set("format_whitelist", "gif,mov,matroska,webm,avi,mpegts");
    let input = format::input_with_dictionary(&path, input_options)?;
    let mut decoder = LogoDecoder::new(input, style, 2, 2)?;
    decoder.initialize_origin(|| stop.is_cancelled())?;
    let mut cache = Frames::new();
    let mut processed = BTreeMap::<Instance, i64>::new();
    let mut last_served = None;
    let mut end = 0;
    let mut loop_end = LoopDuration::from_ticks(1, 1, 1_000_000);
    'requests: while !stop.is_cancelled() {
        if shared.inactive.load(Ordering::Acquire) {
            for request in shared.requests.load().values() {
                request.ready.store(None);
                request.ready_elapsed.store(-1, Ordering::Release);
            }
            cache.clear();
            #[cfg(test)]
            shared.cached_bytes.store(0, Ordering::Release);
            processed.clear();
            thread::park();
            continue;
        }
        let requests = shared.requests.load_full();
        processed.retain(|key, _| requests.contains_key(key));
        let next = requests
            .iter()
            .filter_map(|(key, request)| {
                let elapsed = request.elapsed.load(Ordering::Acquire);

                (request.frame_epoch.load(Ordering::Acquire)
                    == shared.frame_epoch.load(Ordering::Acquire)
                    && processed.get(key) != Some(&elapsed))
                .then_some((*key, request.clone(), elapsed))
            })
            .min_by_key(|(key, _, _)| (last_served.is_some_and(|last| *key <= last), *key));
        let Some((instance, request, elapsed)) = next else {
            thread::park();
            continue;
        };
        last_served = Some(instance);
        let Some(mut local) = local_time(shared, elapsed, options) else {
            processed.insert(instance, elapsed);
            continue;
        };
        if (cache.is_empty() && end > local)
            || cache
                .front()
                .is_some_and(|(timing, _)| local < timing.start_us)
            || local > end + 1_000_000
        {
            // Preserve the original timestamp origin; GIF demuxers may require
            // a rewind rather than arbitrary seeking.
            let hint = decoder.duration_hint_us();
            let target = if hint > 0 {
                local.min(hint.saturating_sub(1))
            } else {
                local
            };
            if decoder.seek_to(target).is_err() {
                decoder.rewind()?;
            }
            cache.clear();
            end = 0;
        }
        'decode: while !stop.is_cancelled() && end <= local.saturating_add(LOOKAHEAD) {
            if request.elapsed.load(Ordering::Acquire) != elapsed {
                break 'decode;
            }
            if request.frame_epoch.load(Ordering::Acquire)
                != shared.frame_epoch.load(Ordering::Acquire)
                || shared
                    .requests
                    .load()
                    .get(&instance)
                    .is_none_or(|current| !Arc::ptr_eq(current, &request))
            {
                continue 'requests;
            }
            while cache.len() > 1
                && cache
                    .get(1)
                    .is_some_and(|(timing, _)| timing.start_us <= local)
            {
                cache.pop_front();
            }
            if let Some((_, last)) = cache.back() {
                let full = cache.len() >= 16
                    || cache
                        .iter()
                        .map(|(_, frame)| bytes(frame))
                        .sum::<usize>()
                        .saturating_add(bytes(last))
                        > LIMIT;
                if full {
                    if cache
                        .front()
                        .is_some_and(|(timing, _)| timing.end_us() <= local)
                    {
                        cache.pop_front();
                    } else {
                        break;
                    }
                }
            }
            let lease = loop {
                if stop.is_cancelled() {
                    return Ok(());
                }
                if request.elapsed.load(Ordering::Acquire) != elapsed {
                    break 'decode;
                }
                if request.frame_epoch.load(Ordering::Acquire)
                    != shared.frame_epoch.load(Ordering::Acquire)
                {
                    continue 'requests;
                }
                match budget.reserve(frame_bytes) {
                    Ok(lease) => break lease,
                    Err(_) if cache.iter().any(|(timing, _)| timing.contains(local)) => {
                        break 'decode;
                    }
                    Err(_) => thread::park_timeout(Duration::from_millis(25)),
                }
            };
            let decoded = decoder.next_frame_with_cancel(|| stop.is_cancelled())?;
            let Some(decoded) = decoded else {
                ensure!(
                    end > 0,
                    "animation has no decodable frames at requested position"
                );
                let _ = shared.duration.set(loop_end);
                let Some(mapped) = local_time(shared, elapsed, options) else {
                    break;
                };
                local = mapped;
                if local
                    < cache
                        .front()
                        .map_or(i64::MAX, |(timing, _)| timing.start_us)
                {
                    decoder.rewind()?;
                    cache.clear();
                    end = 0;
                    continue;
                }
                break;
            };
            let logo = decoded.surface;
            let size = logo.storage_bytes();
            ensure!(
                size <= LIMIT,
                "one animation frame exceeds 64 MiB; reduce size"
            );
            // Eviction and accounting happen on the decoder thread. Queued
            // video snapshots keep their own channel-budgeted leases.
            while !cache.is_empty()
                && (cache.len() >= 16
                    || cache
                        .iter()
                        .map(|(_, frame)| bytes(frame))
                        .sum::<usize>()
                        .saturating_add(size)
                        > LIMIT)
            {
                cache.pop_front();
            }
            ensure!(
                size <= frame_bytes,
                "animation changes prepared storage layout"
            );
            let frame = reference_video_frame(&logo.frame)?;
            let bounds = logo.as_overlay().bounds;
            let alpha = logo.as_overlay().chroma_alpha.to_vec();
            yuva_descriptor(&frame, bounds, alpha.len(), ColorEncoding::Bt601Limited)?;
            let (width, height) = (frame.width(), frame.height());
            end = decoded.timing.end_us();
            loop_end = decoded.loop_end;
            cache.push_back((
                decoded.timing,
                Arc::new(OverlayFrame {
                    lease: Some(lease),
                    frame,
                    chroma_alpha: alpha,
                    bounds,
                    width,
                    height,
                    x: 0,
                    y: 0,
                    opacity: 255,
                }),
            ));
            #[cfg(test)]
            {
                shared.decoded.fetch_add(1, Ordering::AcqRel);
                shared.cached_bytes.store(
                    cache.iter().map(|(_, frame)| bytes(frame)).sum(),
                    Ordering::Release,
                );
            }
        }
        let current = request.elapsed.load(Ordering::Acquire);
        if current >= elapsed
            && let Some(current_local) = local_time(shared, current, options)
            && current_local >= local
        {
            // A slow decoder may finish after the next forward request. Publish
            // that last-good image rather than starving forever under load.
            // Backward seeks, loop wrap and retired presentations cannot use it.
            let candidate = cache
                .iter()
                .find(|(timing, _)| timing.contains(current_local))
                .map(|(_, frame)| (frame, current))
                .or_else(|| {
                    cache
                        .iter()
                        .find(|(timing, _)| timing.contains(local))
                        .map(|(_, frame)| (frame, elapsed))
                });
            if let Some((frame, completed)) = candidate {
                request.ready.store(Some(frame.clone()));
                request.ready_elapsed.store(completed, Ordering::Release);
            }
        }
        if current == elapsed {
            processed.insert(instance, elapsed);
        }
    }

    Ok(())
}
