//! Detect changes before publishing new packets to the session's workers.
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use ffmpeg_next::{Packet, Rescale, codec, codec::packet::side_data, ffi, format, media, rescale};

use super::{Route, Track};

const DISCONTINUITY_US: i64 = 2_000_000;
const MAX_INITIALIZATION_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct Source {
    tracks: Vec<Track>,
    dts: BTreeMap<usize, i64>,
    failed: BTreeMap<i32, Weak<Route>>,
    demux_headers: BTreeMap<usize, Vec<u8>>,
    in_band_headers: BTreeMap<usize, Vec<u8>>,
}

impl Source {
    pub(super) fn tracks(&self) -> Vec<Track> {
        self.tracks.clone()
    }

    pub(super) fn contains(&self, stream: usize) -> bool {
        self.tracks.iter().any(|track| track.input == stream)
    }

    pub(super) fn ready(&self) -> bool {
        !self.tracks.is_empty()
            && self.tracks.iter().all(|track| {
                // These formats require decoder initialization for FLV. Annex B
                // parameters can be collected from a later keyframe if the demuxer
                // did not find them while opening the input.
                let codec = track.parameters.id();

                !matches!(codec, codec::Id::H264 | codec::Id::HEVC)
                    || !extradata(&track.parameters).is_empty()
            })
    }

    pub(super) fn is_failed(&self, route: &Arc<Route>) -> bool {
        self.failed
            .get(&route.id)
            .and_then(Weak::upgrade)
            .is_some_and(|failed| Arc::ptr_eq(&failed, route))
    }

    pub(super) fn failed(&mut self, id: i32, route: Arc<Route>) {
        self.failed.insert(id, Arc::downgrade(&route));
    }

    pub(super) fn update(&mut self, input: &format::context::Input, packet: &Packet) -> bool {
        let mut changed = false;

        if self.tracks.is_empty() || packet.is_key() {
            let selected: Vec<_> = [media::Type::Video, media::Type::Audio]
                .into_iter()
                .filter_map(|kind| input.streams().best(kind))
                .map(|stream| stream.index())
                .collect();

            if !self
                .tracks
                .iter()
                .map(|track| track.input)
                .eq(selected.iter().copied())
            {
                self.tracks = selected
                    .into_iter()
                    .filter_map(|index| input.stream(index))
                    .map(|stream| {
                        let video = stream.parameters().medium() == media::Type::Video;

                        Track::from_stream(stream, video)
                    })
                    .collect();
                self.demux_headers = self
                    .tracks
                    .iter()
                    .map(|track| (track.input, extradata(&track.parameters).to_vec()))
                    .collect();
                self.in_band_headers.clear();
                changed = true;
            }
        }

        if let Some(track) = self
            .tracks
            .iter_mut()
            .find(|track| track.input == packet.stream())
        {
            if let Some(stream) = input.stream(track.input) {
                let parameters = stream.parameters();
                let signature_changed = signature(&track.parameters) != signature(&parameters);
                let headers = packet
                    .side_data()
                    .find(|entry| entry.kind() == side_data::Type::NewExtraData)
                    .map(|entry| entry.data());
                let discovered = (packet.is_key() && headers.is_none() && track.video)
                    .then(|| annex_b_headers(packet.data().unwrap_or_default(), parameters.id()))
                    .flatten();
                let demux_headers = extradata(&parameters);
                let previous = self.demux_headers.entry(track.input).or_default();
                let headers_changed = !demux_headers.is_empty() && demux_headers != previous;

                if headers_changed {
                    *previous = demux_headers.to_vec();
                }

                if signature_changed || headers_changed || track.time_base != stream.time_base() {
                    track.parameters = parameters.clone();
                    track.time_base = stream.time_base();
                    changed = true;
                }

                let new_headers = headers.or_else(|| {
                    {
                        let discovered = discovered.as_ref()?;
                        let previous = self.in_band_headers.entry(track.input).or_default();
                        let changed = !previous.is_empty() && previous != discovered;

                        if previous != discovered {
                            previous.clone_from(discovered);
                        }

                        (changed || extradata(&track.parameters).is_empty()).then_some(discovered)
                    }
                    .map(Vec::as_slice)
                });

                if let Some(headers) = new_headers
                    && headers != extradata(&track.parameters)
                    && replace_extradata(&mut track.parameters, headers)
                {
                    changed = true;
                }
            }

            if let Some(dts) = packet.dts().or_else(|| packet.pts())
                && track.time_base.numerator() > 0
                && track.time_base.denominator() > 0
            {
                let dts = dts.rescale(track.time_base, rescale::TIME_BASE);
                if self.dts.insert(track.input, dts).is_some_and(|last| {
                    dts < last.saturating_sub(DISCONTINUITY_US)
                        || dts > last.saturating_add(10_000_000)
                }) {
                    changed = true;
                }
            }
        }

        if changed {
            self.failed.clear();
            self.dts.clear();
        }

        changed
    }
}

fn signature(parameters: &codec::Parameters) -> (codec::Id, i32, i32, i32, i32, i32, i32, i32) {
    // SAFETY: these fields belong to the borrowed codec parameters.
    let raw = unsafe { &*parameters.as_ptr() };

    (
        parameters.id(),
        raw.width,
        raw.height,
        raw.sample_rate,
        raw.ch_layout.nb_channels,
        raw.format,
        raw.profile,
        raw.level,
    )
}

fn extradata(parameters: &codec::Parameters) -> &[u8] {
    // SAFETY: FFmpeg owns the codec header with the parameters.
    unsafe {
        let raw = &*parameters.as_ptr();
        if raw.extradata.is_null() || raw.extradata_size <= 0 {
            return &[];
        }

        std::slice::from_raw_parts(raw.extradata, raw.extradata_size as usize)
    }
}

fn replace_extradata(parameters: &mut codec::Parameters, data: &[u8]) -> bool {
    if data.is_empty() || data.len() > MAX_INITIALIZATION_BYTES {
        return false;
    }

    // SAFETY: allocate FFmpeg-owned storage with required input padding. The
    // previous pointer belongs exclusively to these cloned parameters.
    unsafe {
        let buffer =
            ffi::av_mallocz(data.len() + ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize).cast::<u8>();
        if buffer.is_null() {
            return false;
        }

        std::ptr::copy_nonoverlapping(data.as_ptr(), buffer, data.len());
        let raw = &mut *parameters.as_mut_ptr();
        ffi::av_free(raw.extradata.cast());
        raw.extradata = buffer;
        raw.extradata_size = data.len() as i32;
    }

    true
}

fn annex_b_headers(data: &[u8], codec: codec::Id) -> Option<Vec<u8>> {
    if !data.starts_with(&[0, 0, 1]) && !data.starts_with(&[0, 0, 0, 1]) {
        return None;
    }

    let needed: &[u8] = match codec {
        codec::Id::H264 => &[7, 8],
        codec::Id::HEVC => &[32, 33, 34],
        _ => return None,
    };
    let mut starts = data
        .windows(3)
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == [0, 0, 1]).then_some(index))
        .peekable();
    let mut found = BTreeMap::new();

    while let Some(start) = starts.next() {
        let end = starts.peek().copied().unwrap_or(data.len());
        let mut nal = data.get(start + 3..end)?;

        while nal.last() == Some(&0) {
            nal = &nal[..nal.len() - 1];
        }
        let Some(header) = nal.first() else { continue };
        let kind = if codec == codec::Id::H264 {
            header & 0x1f
        } else {
            (header >> 1) & 0x3f
        };

        if needed.contains(&kind) {
            found.insert(kind, nal);
        }
    }

    if found.len() != needed.len() {
        return None;
    }

    let mut bytes = Vec::new();

    for nal in found.into_values() {
        if bytes.len() + nal.len() + 4 > MAX_INITIALIZATION_BYTES {
            return None;
        }

        bytes.extend_from_slice(&[0, 0, 0, 1]);
        bytes.extend_from_slice(nal);
    }

    Some(bytes)
}
