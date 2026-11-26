use std::{fs, path::Path, time::SystemTime};

use anyhow::{Context, Result, ensure};
use ff_hls::{MediaPacket, SegmentWriter};
use ffmpeg_next::{Dictionary, Packet, Rational, Rescale, codec, ffi, format, rescale};

use super::{super::hls::HlsReplacement, fmp4};
use crate::utils::ffmpeg::reference_packet;

pub(super) struct Track {
    pub parameters: codec::Parameters,
    pub time_base: Rational,
}

pub(super) struct ContainerWriter {
    tracks: Vec<[Track; 2]>,
    fragments: Option<Vec<fmp4::Initialization>>,
    offset_us: Option<i64>,
    shared_audio: bool,
    replacement: Option<HlsReplacement>,
}

impl ContainerWriter {
    pub fn new(
        tracks: Vec<[Track; 2]>,
        fragmented: bool,
        shared_audio: bool,
        replacement: Option<HlsReplacement>,
    ) -> Self {
        let fragments = fragmented.then(|| {
            (0..tracks.len() + usize::from(shared_audio))
                .map(|_| fmp4::Initialization::default())
                .collect()
        });
        Self {
            tracks,
            fragments,
            offset_us: None,
            shared_audio,
            replacement,
        }
    }
}

impl SegmentWriter<Packet> for ContainerWriter {
    fn first_file_number(&self) -> u64 {
        // A fresh fMP4 playlist can replace an earlier codec or configuration
        // at the same URL. Keep its init and fragments distinct in caches.
        if self.fragments.is_some() {
            u64::try_from(jiff::Timestamp::now().as_microsecond()).unwrap_or(0)
        } else {
            0
        }
    }

    fn extension(&self) -> &'static str {
        if self.fragments.is_some() {
            "m4s"
        } else {
            "ts"
        }
    }

    fn initialization(&self, rendition: usize) -> Option<&str> {
        self.fragments.as_ref()?.get(rendition)?.uri()
    }

    fn initialization_matches(&self, rendition: usize, previous: &Path) -> Result<bool> {
        self.fragments
            .as_ref()
            .context("missing native HLS initialization")?[rendition]
            .matches(previous)
    }

    fn codecs(&self, rendition: usize) -> Option<&str> {
        self.fragments.as_ref()?.get(rendition)?.codecs()
    }

    fn presentation_offset_us(&self) -> i64 {
        self.offset_us.unwrap_or_default()
    }

    fn before_first_publication(&mut self) -> Result<()> {
        if let Some(replacement) = &self.replacement {
            replacement.protect(SystemTime::now())?;
        }
        Ok(())
    }

    fn after_publication(&mut self) -> Result<()> {
        if let Some(replacement) = &mut self.replacement
            && replacement.retire(SystemTime::now())?
        {
            self.replacement = None;
        }
        Ok(())
    }

    fn write_segment(
        &mut self,
        path: &Path,
        packets: &[MediaPacket<Packet>],
        rendition: usize,
    ) -> Result<()> {
        let fragmented = self.fragments.is_some();
        let offset_us = if fragmented {
            *self.offset_us.get_or_insert_with(|| {
                let earliest = packets
                    .iter()
                    .map(|packet| packet.dts_us)
                    .min()
                    .unwrap_or_default();
                // tfdt is unsigned. If an encoder emits negative DTS, move
                // every track by the same offset, rounded up to a millisecond
                // so the WebVTT mapping can describe it on the same clock.
                if earliest < 0 {
                    earliest
                        .saturating_neg()
                        .div_euclid(1000)
                        .saturating_add(1)
                        .saturating_mul(1000)
                } else {
                    0
                }
            })
        } else {
            0
        };
        let audio_only = self.shared_audio && rendition == self.tracks.len();
        let tracks = if audio_only {
            &self.tracks[0][1..]
        } else if self.shared_audio {
            &self.tracks[rendition][..1]
        } else {
            &self.tracks[rendition][..]
        };
        ensure!(
            !path.exists(),
            "refusing to overwrite HLS segment {}",
            path.display()
        );

        let temp = path.with_extension(format!("{}.native.tmp", self.extension()));
        let result = (|| -> Result<()> {
            let mut output = format::output_as(&temp, if fragmented { "mp4" } else { "mpegts" })?;

            // Preserve the shared presentation clock exactly, including the 33-bit
            // wrap in PES headers. WebVTT maps to this same clock, without muxdelay.
            // SAFETY: this exclusively owned context has not written its header yet.
            unsafe {
                (*output.as_mut_ptr()).avoid_negative_ts = 0;
                (*output.as_mut_ptr()).max_interleave_delta = 100_000;
                (*output.as_mut_ptr()).max_delay = 0;
            }

            for track in tracks {
                let mut stream = output.add_stream(codec::Id::None)?;

                let mut parameters = track.parameters.clone();
                if fragmented {
                    // SAFETY: parameters is an exclusively owned clone.
                    unsafe {
                        (*parameters.as_mut_ptr()).codec_tag = if parameters.id() == codec::Id::HEVC
                        {
                            u32::from_le_bytes(*b"hvc1")
                        } else {
                            0
                        };
                    }
                }
                stream.set_parameters(parameters);
                stream.set_time_base(track.time_base);
            }

            let mut options = Dictionary::new();

            if fragmented {
                options.set(
                    "movflags",
                    "delay_moov+default_base_moof+frag_custom+frag_discont+skip_trailer",
                );
                options.set("use_editlist", "0");
            } else {
                options.set("mpegts_copyts", "1");
                options.set("mpegts_flags", "resend_headers+initial_discontinuity");
            }

            let unused = output.write_header_with(options)?;

            ensure!(
                unused.iter().next().is_none(),
                "unused native HLS container options"
            );
            drop(unused);

            for packet in packets.iter().filter(|packet| {
                if audio_only {
                    packet.stream == 1
                } else {
                    packet.stream / 2 == rendition
                        && (!self.shared_audio || packet.stream.is_multiple_of(2))
                }
            }) {
                let index = if self.shared_audio {
                    0
                } else {
                    packet.stream % 2
                };
                let mut packet = reference_packet(&packet.payload)?;
                let time_base = output
                    .stream(index)
                    .context("HLS container stream missing")?
                    .time_base();

                packet.set_stream(index);
                packet.set_position(-1);
                packet.rescale_ts(tracks[index].time_base, time_base);
                if fragmented {
                    let offset = offset_us.rescale(rescale::TIME_BASE, time_base);
                    packet.set_pts(packet.pts().map(|pts| pts.saturating_add(offset)));
                    packet.set_dts(packet.dts().map(|dts| dts.saturating_add(offset)));
                }
                packet.write_interleaved(&mut output)?;
            }

            output.write_trailer()?;
            // SAFETY: the writer exclusively owns this live context. Closing via
            // the pointer also clears it, so Output's destructor cannot close twice.
            let closed = unsafe { ffi::avio_closep(&mut (*output.as_mut_ptr()).pb) };

            if closed < 0 {
                return Err(ffmpeg_next::Error::from(closed)).context("closing native HLS segment");
            }

            drop(output);
            if let Some(initializations) = &mut self.fragments {
                let audio = if audio_only {
                    Some(&tracks[0].parameters)
                } else {
                    tracks.get(1).map(|track| &track.parameters)
                };
                initializations[rendition].publish(&temp, path, !audio_only, audio)?;
            } else {
                fs::rename(&temp, path)
                    .with_context(|| format!("publishing {}", path.display()))?;
            }

            Ok(())
        })();

        if fragmented || result.is_err() {
            let _ = fs::remove_file(temp);
        }

        result
    }
}
