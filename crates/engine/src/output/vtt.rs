use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};

pub(super) struct VttCue {
    pub(super) start_ms: i64,
    pub(super) end_ms: i64,
    pub(super) text: String,
}

impl VttCue {
    /// Intersect with the played source interval before mapping to output time.
    pub(super) fn on_timeline(
        mut self,
        output_start_ms: i64,
        source_start_ms: i64,
        source_end_ms: Option<i64>,
    ) -> Option<Self> {
        self.start_ms = self.start_ms.max(source_start_ms);
        if let Some(end_ms) = source_end_ms {
            self.end_ms = self.end_ms.min(end_ms);
        }
        if self.end_ms <= self.start_ms {
            return None;
        }
        self.start_ms =
            output_start_ms.saturating_add(self.start_ms.saturating_sub(source_start_ms));
        self.end_ms = output_start_ms.saturating_add(self.end_ms.saturating_sub(source_start_ms));

        (self.end_ms > self.start_ms).then_some(self)
    }
}

pub(super) fn sidecar_path(media_path: &str) -> PathBuf {
    Path::new(media_path).with_extension("vtt")
}

/// Optional subtitle data must never disconnect the audio/video destination.
pub(super) fn load_sidecar(media_path: &str, channel_id: i32) -> Vec<VttCue> {
    let path = sidecar_path(media_path);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            log::warn!(channel = channel_id; "Ignoring VTT sidecar {}: {error}", path.display());

            return Vec::new();
        }
    };
    let parsed = parse(&content);

    if let Some((line, error)) = parsed.first_error {
        log::warn!(channel = channel_id;
            "Skipped {} invalid VTT block(s) in {}; first error at line {line}: {error}",
            parsed.invalid_blocks, path.display()
        );
    }

    parsed.cues
}

#[derive(Default)]
struct ParsedVtt {
    cues: Vec<VttCue>,
    invalid_blocks: usize,
    first_error: Option<(usize, String)>,
}

fn parse(content: &str) -> ParsedVtt {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut parsed = ParsedVtt::default();
    let mut block = Vec::new();
    let mut block_start = 1;

    for (index, line) in normalized.lines().chain(std::iter::once("")).enumerate() {
        if line.trim().is_empty() {
            if let Err(error) = parse_block(&block, &mut parsed.cues) {
                parsed.invalid_blocks += 1;
                parsed
                    .first_error
                    .get_or_insert_with(|| (block_start, error.to_string()));
            }
            block.clear();
        } else {
            if block.is_empty() {
                block_start = index + 1;
            }
            block.push(line);
        }
    }

    parsed
}

fn parse_block(lines: &[&str], cues: &mut Vec<VttCue>) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }

    let first = lines[0].trim_start_matches('\u{feff}').trim();

    if first.starts_with("WEBVTT")
        || first.starts_with("NOTE")
        || first == "STYLE"
        || first == "REGION"
    {
        return Ok(());
    }

    let timing_index = lines
        .iter()
        .position(|line| line.contains("-->"))
        .ok_or_else(|| anyhow!("VTT cue is missing a timing line"))?;
    let (start_ms, end_ms) = parse_timing(lines[timing_index])?;

    if end_ms <= start_ms {
        return Err(anyhow!("VTT cue end must be after start"));
    }

    let text = lines[timing_index + 1..]
        .iter()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        return Ok(());
    }

    cues.push(VttCue {
        start_ms,
        end_ms,
        text,
    });

    Ok(())
}

fn parse_timing(line: &str) -> Result<(i64, i64)> {
    let (start, rest) = line
        .split_once("-->")
        .ok_or_else(|| anyhow!("invalid VTT timing line"))?;
    let end = rest
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("missing VTT cue end timestamp"))?;
    Ok((parse_timestamp(start.trim())?, parse_timestamp(end)?))
}

fn parse_timestamp(value: &str) -> Result<i64> {
    let value = value.replace(',', ".");
    let parts = value.split(':').collect::<Vec<_>>();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0_i64, minutes.parse::<i64>()?, *seconds),
        [hours, minutes, seconds] => (hours.parse::<i64>()?, minutes.parse::<i64>()?, *seconds),
        _ => return Err(anyhow!("invalid VTT timestamp {value:?}")),
    };
    let (seconds, millis) = seconds
        .split_once('.')
        .ok_or_else(|| anyhow!("VTT timestamp must include milliseconds"))?;
    let seconds = seconds.parse::<i64>()?;
    let millis = parse_milliseconds(millis)?;

    if hours < 0 || !(0..60).contains(&minutes) || !(0..60).contains(&seconds) {
        return Err(anyhow!("VTT timestamp components are out of range"));
    }

    hours
        .checked_mul(60)
        .and_then(|value| value.checked_add(minutes))
        .and_then(|value| value.checked_mul(60))
        .and_then(|value| value.checked_add(seconds))
        .and_then(|value| value.checked_mul(1_000))
        .and_then(|value| value.checked_add(millis))
        .ok_or_else(|| anyhow!("VTT timestamp is too large"))
}

fn parse_milliseconds(value: &str) -> Result<i64> {
    let mut millis = value.chars().take(3).collect::<String>();

    if millis.is_empty() || !millis.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(anyhow!("invalid VTT millisecond value"));
    }

    while millis.len() < 3 {
        millis.push('0');
    }

    Ok(millis.parse::<i64>()?)
}

#[cfg(test)]
mod tests {
    use super::{parse, parse_timestamp};

    #[test]
    fn parses_timestamp_with_hours() {
        assert_eq!(parse_timestamp("01:02:03.456").unwrap(), 3_723_456);
    }

    #[test]
    fn parses_timestamp_without_hours() {
        assert_eq!(parse_timestamp("02:03.456").unwrap(), 123_456);
    }

    #[test]
    fn parses_timestamp_with_comma_decimal_separator() {
        assert_eq!(parse_timestamp("00:00:01,500").unwrap(), 1_500);
    }

    #[test]
    fn pads_short_millisecond_fractions() {
        assert_eq!(parse_timestamp("00:00:01.5").unwrap(), 1_500);
        assert_eq!(parse_timestamp("00:00:01.05").unwrap(), 1_050);
    }

    #[test]
    fn rejects_timestamp_without_milliseconds() {
        assert!(parse_timestamp("00:00:01").is_err());
    }

    #[test]
    fn rejects_malformed_timestamp() {
        assert!(parse_timestamp("not-a-time").is_err());
        assert!(parse_timestamp("1:2:3:4.000").is_err());
    }

    #[test]
    fn parses_single_cue() {
        let content = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:02.500\nHello\nWorld\n";
        let cues = parse(content).cues;
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].start_ms, 1_000);
        assert_eq!(cues[0].end_ms, 2_500);
        assert_eq!(cues[0].text, "Hello\nWorld");
    }

    #[test]
    fn skips_notes_and_regions_and_empty_cues() {
        let content = "WEBVTT\n\nNOTE this is a comment\n\nSTYLE\n::cue { color: red }\n\n1\n00:00:01.000 --> 00:00:02.000\n\n2\n00:00:03.000 --> 00:00:04.000\nActual cue\n";
        let cues = parse(content).cues;
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Actual cue");
    }

    #[test]
    fn skips_cue_with_end_before_start() {
        let content = "1\n00:00:05.000 --> 00:00:01.000\nBroken cue\n";
        let parsed = parse(content);
        assert!(parsed.cues.is_empty());
        assert_eq!(parsed.invalid_blocks, 1);
    }

    #[test]
    fn invalid_blocks_do_not_discard_valid_cues_before_or_after_them() {
        let parsed = parse(concat!(
            "WEBVTT\n\n",
            "00:00:01.000 --> 00:00:02.000\nBefore\n\n",
            "00:00:03.000 --> 00:00:03.000\nZero duration\n\n",
            "00:00:05.000 --> 00:00:04.000\nReversed\n\n",
            "Not a timing line\nBroken\n\n",
            "broken --> 00:00:07.000\nBad timestamp\n\n",
            "00:00:08.000 --> 00:00:09.000\nAfter",
        ));
        assert_eq!(parsed.invalid_blocks, 4);
        let (line, error) = parsed.first_error.unwrap();
        assert_eq!(line, 6);
        assert_eq!(error, "VTT cue end must be after start");
        assert_eq!(
            parsed
                .cues
                .iter()
                .map(|cue| cue.text.as_str())
                .collect::<Vec<_>>(),
            ["Before", "After"]
        );
    }

    #[test]
    fn out_of_range_timestamps_are_rejected_without_overflow() {
        for timestamp in [
            "9223372036854775807:00:00.000",
            "-1:00:00.000",
            "00:60:00.000",
            "00:00:60.000",
        ] {
            assert!(parse_timestamp(timestamp).is_err(), "{timestamp}");
        }
    }

    #[test]
    fn unreadable_and_missing_sidecars_return_no_cues_and_can_be_replaced() {
        let directory =
            std::env::temp_dir().join(format!("vtt_sidecar_recovery_{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let media = directory.join("clip.mp4");
        let path = media.to_str().unwrap();
        assert!(super::load_sidecar(path, 1).is_empty());
        std::fs::write(media.with_extension("vtt"), [0xff, 0xfe]).unwrap();
        assert!(super::load_sidecar(path, 1).is_empty());
        std::fs::write(
            media.with_extension("vtt"),
            "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nRecovered\n",
        )
        .unwrap();
        assert_eq!(super::load_sidecar(path, 1)[0].text, "Recovered");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn clips_cues_to_the_played_interval_before_mapping_timestamps() {
        let cue = |start_ms, end_ms| super::VttCue {
            start_ms,
            end_ms,
            text: "Caption".into(),
        };
        // Seek into a cue and trim its end to the earlier clip boundary.
        let mapped = cue(1_000, 9_000)
            .on_timeline(50_000, 2_000, Some(4_000))
            .unwrap();
        assert_eq!((mapped.start_ms, mapped.end_ms), (50_000, 52_000));
        let mapped = cue(3_000, 9_000)
            .on_timeline(50_000, 2_000, Some(4_000))
            .unwrap();
        assert_eq!((mapped.start_ms, mapped.end_ms), (51_000, 52_000));
        // Boundary-only contact has no visible duration.
        assert!(
            cue(0, 2_000)
                .on_timeline(50_000, 2_000, Some(4_000))
                .is_none()
        );
        assert!(
            cue(4_000, 8_000)
                .on_timeline(50_000, 2_000, Some(4_000))
                .is_none()
        );
        assert!(
            cue(5_000, 8_000)
                .on_timeline(50_000, 2_000, Some(4_000))
                .is_none()
        );
        let mapped = cue(3_000, 9_000).on_timeline(50_000, 2_000, None).unwrap();
        assert_eq!(mapped.end_ms, 57_000);
    }

    #[test]
    fn normalizes_crlf_line_endings() {
        let content = "WEBVTT\r\n\r\n1\r\n00:00:01.000 --> 00:00:02.000\r\nHi\r\n";
        let cues = parse(content).cues;
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Hi");
    }
}
