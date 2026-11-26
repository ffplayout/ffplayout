//! Experimental local HLS orchestration with a caller-provided container writer.
//! One destination worker owns all renditions and publishes only closed files.
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt::Write,
    fs, mem,
    path::PathBuf,
};

use anyhow::{Context, Result, ensure};
use jiff::{RoundMode, SignedDuration, Timestamp, TimestampRound, Unit};

use crate::manifest::{Playlist, Segment, atomic_write, load_codecs, safe_name};

use super::{Config, Cue, MediaPacket, RenditionConfig, SegmentWriter, Subtitle};

const MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;
const MAX_PACKETS: usize = 32_768;
const MPEGTS_WRAP: i64 = 1 << 33;

struct Rendition {
    playlist: Playlist,
    name: String,
    first_key: Option<i64>,
    keyframes: VecDeque<(i64, i64)>,
    latest_audio: Option<i64>,
    codecs: String,
    resumed_codecs: Option<String>,
    resumed_last_sequence: Option<u64>,
    width: u32,
    height: u32,
    bandwidth: u64,
}

pub struct Packager<T, W> {
    writer: W,
    renditions: Vec<Rendition>,
    subtitles: Option<(Subtitle, Playlist)>,
    master: Option<PathBuf>,
    packets: Vec<MediaPacket<T>>,
    completed: Vec<MediaPacket<T>>,
    bytes: usize,
    cues: VecDeque<Cue>,
    cue_bytes: usize,
    segment_us: i64,
    list_size: usize,
    start: Option<i64>,
    next_target: Option<i64>,
    sequence: u64,
    file_number: u64,
    discontinuity: bool,
    wall_anchor: Option<(i64, Timestamp)>,
    program_date_time: bool,
    finished: bool,
    published: bool,
}

impl<T, W: SegmentWriter<T>> Packager<T, W> {
    pub fn new(config: Config, writer: W) -> Result<Self> {
        Self::open(config, writer, None)
    }

    pub fn new_replacing(
        config: Config,
        writer: W,
        next_sequence: u64,
        discontinuity_sequence: u64,
    ) -> Result<Self> {
        Self::open(
            config,
            writer,
            Some((next_sequence, discontinuity_sequence)),
        )
    }

    fn open(config: Config, writer: W, replacement_sequence: Option<(u64, u64)>) -> Result<Self> {
        let replace = replacement_sequence.is_some();
        let extension = writer.extension();
        ensure!(
            matches!(extension, "ts" | "m4s"),
            "unsupported native HLS video segment extension"
        );
        ensure!(
            (1..=60).contains(&config.segment_seconds),
            "native HLS segment duration must be 1–60 seconds"
        );
        ensure!(
            (1..=10_000).contains(&config.list_size),
            "native HLS playlist size must be 1–10000"
        );
        ensure!(
            !config.renditions.is_empty() && config.renditions.len() <= 16,
            "native HLS requires 1–16 renditions"
        );

        if let Some(name) = &config.master_playlist {
            ensure!(
                safe_name(name) && name.ends_with(".m3u8"),
                "invalid native HLS master name"
            );
        }

        ensure!(
            (1..=i32::MAX as u32).contains(&config.delete_threshold),
            "native HLS delete threshold must be 1–2147483647"
        );

        let parent = &config.directory;
        let previous_codecs = config
            .master_playlist
            .as_ref()
            .map(|name| load_codecs(&parent.join(name)))
            .unwrap_or_default();
        let mut renditions = Vec::new();
        let mut names = HashSet::new();

        for RenditionConfig {
            name,
            width,
            height,
            bandwidth,
            codecs,
        } in config.renditions
        {
            ensure!(
                safe_name(&name) && name != "master" && names.insert(name.clone()),
                "invalid or duplicate native HLS variant name"
            );
            ensure!(
                !codecs.contains(['"', '\r', '\n']),
                "invalid HLS codec declaration"
            );

            let uri = format!("{name}.m3u8");
            let path = parent.join(&uri);
            let playlist = if replace {
                Playlist::empty(path)
            } else {
                Playlist::load(path)?
            };
            let resumed_last_sequence = playlist.segments.back().map(|segment| segment.sequence);

            renditions.push(Rendition {
                resumed_codecs: previous_codecs.get(&uri).cloned(),
                resumed_last_sequence,
                playlist,
                name,
                first_key: None,
                keyframes: VecDeque::new(),
                latest_audio: None,
                codecs,
                width,
                height,
                bandwidth,
            });
        }

        ensure!(
            !config
                .master_playlist
                .as_ref()
                .is_some_and(|master| names.iter().any(|name| *master == format!("{name}.m3u8"))),
            "HLS master must not overwrite a media playlist"
        );
        ensure!(
            config.subtitle.is_none() || !names.contains(&format!("{}_vtt", renditions[0].name)),
            "HLS subtitle and rendition playlist names collide"
        );

        if let Some(subtitle) = &config.subtitle {
            ensure!(
                !subtitle.name.contains(['"', '\r', '\n'])
                    && !subtitle.language.contains(['"', '\r', '\n']),
                "invalid native HLS subtitle attributes"
            );
        }

        for rendition in &mut renditions {
            rendition.playlist.configure(
                config.segment_seconds,
                config.delete_threshold,
                &format!("{}_", rendition.name),
                extension,
                replace,
            )?;
        }

        let mut subtitles = config
            .subtitle
            .map(|subtitle| {
                Ok::<_, anyhow::Error>((subtitle, {
                    let path = parent.join(format!("{}_vtt.m3u8", renditions[0].name));
                    if replace {
                        Playlist::empty(path)
                    } else {
                        Playlist::load(path)?
                    }
                }))
            })
            .transpose()?;
        if let Some((_, playlist)) = &mut subtitles {
            playlist.configure(
                config.segment_seconds,
                config.delete_threshold,
                &renditions[0].name,
                "vtt",
                replace,
            )?;
        }

        // Native variants share one file counter. Two manifests assigning
        // different media sequences to that same last set indicate corruption.
        let mut last_files = HashMap::new();
        for rendition in &renditions {
            if let Some(last) = rendition.playlist.segments.back()
                && let Some((_, suffix)) = last.uri.rsplit_once('_')
                && let Some(previous) = last_files.insert(suffix, last.sequence)
            {
                ensure!(
                    previous == last.sequence,
                    "inconsistent native HLS media sequence for the same segment set"
                );
            }
        }
        let histories = renditions
            .iter()
            .map(|r| &r.playlist)
            .chain(subtitles.iter().map(|(_, playlist)| playlist));
        let mut latest = None;
        let (mut sequence, mut discontinuity_sequence) = replacement_sequence.unwrap_or_default();
        for playlist in histories {
            if let Some(last) = playlist.segments.back() {
                latest = Some(latest.unwrap_or(0).max(last.sequence));
                discontinuity_sequence =
                    discontinuity_sequence.max(playlist.last_discontinuity_sequence()?);
            }
        }
        let has_history = latest.is_some();
        if let Some(last) = latest {
            sequence = last.checked_add(1).context("HLS sequence overflow")?;
        }
        let discontinuity = has_history || sequence > 0;
        // A process may have published only part of the last rendition set.
        // Keep the advanced windows and let lagging tracks rejoin at the next
        // unused sequence. Their on-disk playlists stay live until publication.
        for rendition in &mut renditions {
            let playlist = &mut rendition.playlist;
            if playlist
                .segments
                .back()
                .is_none_or(|s| Some(s.sequence) != latest)
            {
                playlist.reset_history(discontinuity_sequence);
                rendition.resumed_last_sequence = None;
            }
        }
        if let Some((_, playlist)) = &mut subtitles
            && playlist
                .segments
                .back()
                .is_none_or(|s| Some(s.sequence) != latest)
        {
            playlist.reset_history(discontinuity_sequence);
        }

        let mut file_number = if has_history {
            sequence
        } else {
            writer.first_file_number().max(sequence)
        };

        // Orphaned files can remain after a crash, or numbering may differ from
        // MEDIA-SEQUENCE in playlists produced by the FFmpeg backend.
        let prefixes: Vec<_> = renditions
            .iter()
            .map(|rendition| format!("{}_", rendition.name))
            .collect();

        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();

            for prefix in &prefixes {
                if let Some(number) = name
                    .strip_prefix(prefix)
                    .and_then(|suffix| {
                        suffix
                            .strip_suffix(if extension == "m4s" { ".m4s" } else { ".ts" })
                            .or_else(|| {
                                (extension == "m4s")
                                    .then(|| suffix.strip_suffix(".init.mp4"))
                                    .flatten()
                            })
                    })
                    .and_then(|number| number.parse::<u64>().ok())
                {
                    file_number = file_number.max(
                        number
                            .checked_add(1)
                            .context("HLS filename sequence overflow")?,
                    );
                }
            }
        }

        if subtitles.is_some() {
            for entry in fs::read_dir(parent)? {
                let name = entry?.file_name();
                let name = name.to_string_lossy();

                if let Some(number) = name
                    .strip_prefix(&renditions[0].name)
                    .and_then(|value| value.strip_suffix(".vtt"))
                    .and_then(|value| value.parse::<u64>().ok())
                {
                    file_number = file_number.max(
                        number
                            .checked_add(1)
                            .context("HLS subtitle filename overflow")?,
                    );
                }
            }
        }

        Ok(Self {
            writer,
            renditions,
            subtitles,
            master: config.master_playlist.map(|name| parent.join(name)),
            packets: Vec::new(),
            completed: Vec::new(),
            bytes: 0,
            cues: VecDeque::new(),
            cue_bytes: 0,
            segment_us: i64::from(config.segment_seconds) * 1_000_000,
            list_size: config.list_size as usize,
            start: None,
            next_target: None,
            sequence,
            file_number,
            discontinuity,
            wall_anchor: None,
            program_date_time: config.program_date_time,
            finished: false,
            published: false,
        })
    }

    /// Supply codec declarations discovered before the next playlist publication.
    pub fn set_codecs(&mut self, rendition: usize, codecs: String) -> Result<()> {
        ensure!(
            !codecs.contains(['"', '\r', '\n']),
            "invalid HLS codec declaration"
        );

        let rendition = self
            .renditions
            .get_mut(rendition)
            .context("HLS rendition missing")?;
        rendition.codecs = codecs;

        Ok(())
    }

    pub fn write_cue(&mut self, cue: Cue) -> Result<()> {
        ensure!(!self.finished, "native HLS output has finished");
        ensure!(
            cue.start_ms >= 0 && cue.end_ms > cue.start_ms,
            "invalid HLS cue times"
        );
        ensure!(
            self.cues.len() < 10_000
                && self.cue_bytes.saturating_add(cue.text.len()) <= 8 * 1024 * 1024,
            "native HLS subtitle queue exceeded capacity"
        );

        self.cue_bytes += cue.text.len();
        self.cues.push_back(cue);

        Ok(())
    }

    fn anchor_program_date_time(&mut self, media_start: i64, date: Timestamp) -> Result<()> {
        let published_date = date.round(
            TimestampRound::new()
                .smallest(Unit::Millisecond)
                .mode(RoundMode::Floor),
        )?;
        let mut overlaps = false;

        for playlist in self.playlists() {
            overlaps |= playlist.overlaps_program_date_time(published_date)?;
        }

        if overlaps {
            self.reset_history()?;
        }

        self.wall_anchor = Some((media_start, date));

        Ok(())
    }

    fn playlists(&self) -> impl Iterator<Item = &Playlist> {
        self.renditions
            .iter()
            .map(|rendition| &rendition.playlist)
            .chain(self.subtitles.iter().map(|(_, playlist)| playlist))
    }

    fn reset_history(&mut self) -> Result<()> {
        let mut sequence = 0;

        for playlist in self.playlists() {
            sequence = sequence.max(playlist.last_discontinuity_sequence()?);
        }

        for rendition in &mut self.renditions {
            rendition.playlist.reset_history(sequence);
            rendition.resumed_last_sequence = None;
        }
        if let Some((_, playlist)) = &mut self.subtitles {
            playlist.reset_history(sequence);
        }
        self.discontinuity = true;

        Ok(())
    }

    pub fn write(&mut self, packet: MediaPacket<T>) -> Result<()> {
        self.write_with_clock(packet, Timestamp::now)
    }

    pub(super) fn write_with_clock(
        &mut self,
        packet: MediaPacket<T>,
        now: impl FnOnce() -> Timestamp,
    ) -> Result<()> {
        ensure!(!self.finished, "native HLS output has finished");

        let stream = packet.stream;
        let pts = packet.pts_us;
        let dts = packet.dts_us;
        let packet_keyframe = packet.keyframe;

        ensure!(
            stream < self.renditions.len() * 2,
            "native HLS packet stream is out of range"
        );
        ensure!(packet.duration_us >= 0, "negative packet duration");

        let rendition = &mut self.renditions[stream / 2];

        if stream.is_multiple_of(2) {
            if packet.keyframe {
                rendition.first_key.get_or_insert(pts);
                rendition.keyframes.push_back((pts, dts));
            } else if rendition.first_key.is_none() {
                return Ok(());
            }
        } else {
            rendition.latest_audio = Some(pts);
        }

        self.bytes = self.bytes.saturating_add(packet.bytes);
        ensure!(
            self.bytes <= MAX_BUFFER_BYTES && self.packets.len() < MAX_PACKETS,
            "native HLS segment buffer exceeded bounded capacity"
        );
        self.packets.push(packet);

        if self.start.is_none() {
            let common = self.renditions[0].keyframes.iter().find_map(|&(pts, _)| {
                let cuts = self
                    .renditions
                    .iter()
                    .map(|rendition| {
                        rendition
                            .keyframes
                            .iter()
                            .find(|(key, _)| *key == pts)
                            .map(|(_, dts)| *dts)
                    })
                    .collect::<Option<Vec<_>>>();

                cuts.map(|cuts| (pts, cuts))
            });

            if let Some((start, cuts)) = common {
                let next_target = start
                    .div_euclid(self.segment_us)
                    .checked_add(1)
                    .and_then(|slot| slot.checked_mul(self.segment_us))
                    .context("HLS target timestamp overflow")?;

                if self.program_date_time {
                    self.anchor_program_date_time(start, now())?;
                }

                // Reconnects may first encounter different scene-cut keyframes.
                // Join the first common keyframe and discard preceding media.
                self.packets.retain(|packet| {
                    if packet.stream.is_multiple_of(2) {
                        packet.dts_us >= cuts[packet.stream / 2]
                    } else {
                        packet.pts_us >= start
                    }
                });
                self.bytes = self.packets.iter().map(|packet| packet.bytes).sum();
                self.start = Some(start);
                self.next_target = Some(next_target);
            }
        }

        if let Some(start) = self.start {
            ensure!(
                pts.saturating_sub(start) <= self.segment_us * 3 + 5_000_000,
                "native HLS could not find aligned segment boundaries"
            );

            // Only keyframes and audio progress can make a boundary ready.
            if (stream % 2 == 1 || packet_keyframe)
                && let Some((end, cuts)) = self.next_boundary(start)
            {
                let next_target = self
                    .next_target
                    .context("HLS segment target missing")?
                    .checked_add(self.segment_us)
                    .context("HLS target timestamp overflow")?;

                self.publish(end, Some(&cuts))?;
                self.start = Some(end);
                self.next_target = Some(next_target);

                for rendition in &mut self.renditions {
                    rendition.keyframes.retain(|(pts, _)| *pts >= end);
                }
            }
        }

        Ok(())
    }

    fn next_boundary(&self, start: i64) -> Option<(i64, Vec<i64>)> {
        let target = self.next_target?;

        for &(pts, _) in &self.renditions[0].keyframes {
            // Keep cuts on the shared program grid. A late cut must not
            // move every following target further out.
            if pts < target || pts <= start {
                continue;
            }

            if !self
                .renditions
                .iter()
                .all(|rendition| rendition.latest_audio.is_some_and(|audio| audio >= pts))
            {
                continue;
            }

            let cuts = self
                .renditions
                .iter()
                .map(|rendition| {
                    rendition
                        .keyframes
                        .iter()
                        .find(|(key, _)| *key == pts)
                        .map(|(_, dts)| *dts)
                })
                .collect::<Option<Vec<_>>>();

            if let Some(cuts) = cuts {
                return Some((pts, cuts));
            }
        }

        None
    }

    fn publish(&mut self, end: i64, cuts: Option<&[i64]>) -> Result<()> {
        let start = self.start.context("native HLS segment start missing")?;

        ensure!(end > start, "native HLS segment duration must be positive");

        let duration = (end - start) as f64 / 1_000_000.0;

        // Reject an out-of-bound cut before writing any media or manifests.
        for rendition in &self.renditions {
            rendition.playlist.validate_duration(duration)?;
        }

        if let Some((_, playlist)) = &self.subtitles {
            playlist.validate_duration(duration)?;
        }

        let date = if self.program_date_time {
            let (anchor, date) = self
                .wall_anchor
                .context("native HLS wall clock anchor missing")?;
            let offset = SignedDuration::from_micros_i128(i128::from(start) - i128::from(anchor));
            let date = date.checked_add(offset)?;

            Some(date.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        } else {
            None
        };

        // Keep the completed segment in its existing storage. Move only
        // the packets beyond the cut into the buffer for the next segment.
        mem::swap(&mut self.packets, &mut self.completed);
        let pending = self.completed.extract_if(.., |packet| {
            let before_cut = cuts.is_none_or(|cuts| {
                if packet.stream.is_multiple_of(2) {
                    packet.dts_us < cuts[packet.stream / 2]
                } else {
                    packet.pts_us < end
                }
            });

            !before_cut || (!packet.stream.is_multiple_of(2) && packet.pts_us < start)
        });

        self.packets.extend(
            pending.filter(|packet| packet.stream.is_multiple_of(2) || packet.pts_us >= end),
        );
        self.completed.sort_by_key(|packet| packet.dts_us);
        self.bytes = self.packets.iter().map(|packet| packet.bytes).sum();

        // All media resources are complete before any playlist can expose them.
        let media_result = self
            .renditions
            .iter()
            .enumerate()
            .try_for_each(|(index, rendition)| {
                let uri = format!(
                    "{}_{}.{}",
                    rendition.name,
                    self.file_number,
                    self.writer.extension()
                );

                self.writer.write_segment(
                    &rendition.playlist.parent().join(&uri),
                    &self.completed,
                    index,
                )
            });

        self.completed.clear();
        media_result?;

        if let Some((_, playlist)) = &self.subtitles {
            let uri = format!("{}{}.vtt", self.renditions[0].name, self.file_number);
            let mapping = (start.saturating_add(self.writer.presentation_offset_us()) / 1000 * 90)
                .rem_euclid(MPEGTS_WRAP);
            let mut text = format!(
                "WEBVTT\nX-TIMESTAMP-MAP=LOCAL:{},MPEGTS:{mapping}\n\n",
                timestamp(start / 1000)
            );

            for cue in self
                .cues
                .iter()
                .filter(|cue| cue.start_ms < (end + 999) / 1000 && cue.end_ms > start / 1000)
            {
                write!(
                    text,
                    "{} --> {}\n{}\n\n",
                    timestamp(cue.start_ms),
                    timestamp(cue.end_ms),
                    cue.text
                )?;
            }

            let path = playlist.parent().join(uri);

            ensure!(
                !path.exists(),
                "refusing to overwrite HLS subtitle segment {}",
                path.display()
            );
            atomic_write(&path, text.as_bytes())?;
        }

        if !self.published {
            self.writer.before_first_publication()?;
            let mut replace = false;
            for (index, rendition) in self.renditions.iter().enumerate() {
                if let Some(uri) = rendition
                    .playlist
                    .segments
                    .back()
                    .and_then(|segment| segment.initialization.as_ref())
                {
                    let previous = rendition.playlist.parent().join(uri);
                    replace |= !self.writer.initialization_matches(index, &previous)?;
                }
            }

            if replace {
                self.reset_history()?;
            }
        }

        for (index, rendition) in self.renditions.iter_mut().enumerate() {
            if let Some(codecs) = self.writer.codecs(index)
                && rendition.codecs != codecs
            {
                rendition.codecs = codecs.to_owned();
            }
            rendition.playlist.append(
                Segment {
                    sequence: self.sequence,
                    uri: format!(
                        "{}_{}.{}",
                        rendition.name,
                        self.file_number,
                        self.writer.extension()
                    ),
                    initialization: self.writer.initialization(index).map(str::to_owned),
                    duration,
                    discontinuity: self.discontinuity,
                    date: date.clone(),
                },
                self.list_size,
            )?;
        }

        if let Some((_, playlist)) = &mut self.subtitles {
            playlist.append(
                Segment {
                    sequence: self.sequence,
                    uri: format!("{}{}.vtt", self.renditions[0].name, self.file_number),
                    initialization: None,
                    duration,
                    discontinuity: self.discontinuity,
                    date,
                },
                self.list_size,
            )?;
            playlist.publish()?;
        }

        for rendition in &mut self.renditions {
            rendition.playlist.publish()?;
        }

        self.publish_master()?;
        self.published = true;
        self.writer.after_publication()?;
        for rendition in &mut self.renditions {
            rendition.playlist.retire()?;
        }
        if let Some((_, playlist)) = &mut self.subtitles {
            playlist.retire()?;
        }
        self.cues.retain(|cue| cue.end_ms > end / 1000);
        self.cue_bytes = self.cues.iter().map(|cue| cue.text.len()).sum();
        self.sequence = self
            .sequence
            .checked_add(1)
            .context("native HLS sequence overflow")?;
        self.file_number = self
            .file_number
            .checked_add(1)
            .context("native HLS filename overflow")?;
        self.discontinuity = false;

        Ok(())
    }

    fn publish_master(&self) -> Result<()> {
        let Some(path) = &self.master else {
            return Ok(());
        };
        let mut text = "#EXTM3U\n#EXT-X-VERSION:3\n".to_owned();

        if let Some((subtitle, playlist)) = &self.subtitles {
            ensure!(
                !subtitle.name.contains(['"', '\r', '\n'])
                    && !subtitle.language.contains(['"', '\r', '\n']),
                "invalid native HLS subtitle attributes"
            );

            writeln!(
                text,
                "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"{}\",LANGUAGE=\"{}\",DEFAULT={},AUTOSELECT=YES,URI=\"{}\"",
                subtitle.name,
                subtitle.language,
                if subtitle.default { "YES" } else { "NO" },
                playlist
                    .path
                    .file_name()
                    .context("subtitle playlist filename missing")?
                    .to_string_lossy()
            )?;
        }

        for rendition in &self.renditions {
            write!(
                text,
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}",
                rendition.bandwidth, rendition.width, rendition.height
            )?;

            // A discontinuity preserves old media, which may require a different
            // codec. Advertise the new declaration only once it describes the
            // entire window, including after delayed header discovery.
            let retained_history = rendition.resumed_last_sequence.is_some_and(|last| {
                rendition
                    .playlist
                    .segments
                    .front()
                    .is_some_and(|segment| segment.sequence <= last)
            });
            let compatible_history =
                rendition.resumed_codecs.as_deref() == Some(rendition.codecs.as_str());

            if !rendition.codecs.is_empty() && (!retained_history || compatible_history) {
                write!(text, ",CODECS=\"{}\"", rendition.codecs)?;
            }

            if self.subtitles.is_some() {
                text.push_str(",SUBTITLES=\"subs\"");
            }

            write!(text, "\n{}.m3u8\n", rendition.name)?;
        }

        atomic_write(path, text.as_bytes())
    }

    pub fn finish(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }

        if let Some(start) = self.start {
            let end = self
                .packets
                .iter()
                .filter(|packet| packet.stream.is_multiple_of(2))
                .map(|packet| packet.pts_us.saturating_add(packet.duration_us))
                .max()
                .unwrap_or(start);

            if end > start {
                self.publish(end, None)?;
            }
        }

        self.finished = true;

        Ok(())
    }
}

fn timestamp(milliseconds: i64) -> String {
    let milliseconds = milliseconds.max(0);

    format!(
        "{:02}:{:02}:{:02}.{:03}",
        milliseconds / 3_600_000,
        milliseconds / 60_000 % 60,
        milliseconds / 1000 % 60,
        milliseconds % 1000
    )
}
