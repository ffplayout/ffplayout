use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write as _},
    path::Path,
};

use anyhow::{Context, Result, ensure};
use ffmpeg_next::codec;

use super::codecs;

#[derive(Default)]
pub(super) struct Initialization {
    uri: Option<String>,
    bytes: Vec<u8>,
    codecs: Option<String>,
}

impl Initialization {
    pub fn uri(&self) -> Option<&str> {
        self.uri.as_deref()
    }

    pub fn codecs(&self) -> Option<&str> {
        self.codecs.as_deref()
    }

    pub fn matches(&self, previous: &Path) -> Result<bool> {
        // Compare the complete init section, including sample descriptions,
        // track clocks, video dimensions and both codec configuration records.
        let mut previous_bytes = Vec::new();
        File::open(previous)?
            .take(self.bytes.len() as u64 + 1)
            .read_to_end(&mut previous_bytes)?;
        Ok(self.bytes == previous_bytes)
    }

    pub fn publish(
        &mut self,
        source: &Path,
        fragment: &Path,
        has_video: bool,
        audio: Option<&codec::Parameters>,
    ) -> Result<()> {
        let mut input = File::open(source)?;
        let end = input.metadata()?.len();
        let (ftyp, first_end) = read_box(&mut input, end)?;
        input.seek(SeekFrom::Start(first_end))?;
        let (moov, init_end) = read_box(&mut input, end)?;
        input.seek(SeekFrom::Start(init_end))?;
        let (moof, _) = read_box(&mut input, end)?;
        ensure!(
            ftyp == *b"ftyp" && moov == *b"moov" && moof == *b"moof",
            "unexpected native fMP4 box layout"
        );
        ensure!(
            init_end <= 1024 * 1024,
            "native fMP4 initialization is too large"
        );

        input.seek(SeekFrom::Start(0))?;
        let mut initialization = vec![0; init_end as usize];
        input.read_exact(&mut initialization)?;

        if self.uri.is_none() {
            let mut identifiers = Vec::new();
            if has_video {
                identifiers.push(
                    video_codec(&initialization)
                        .context("invalid native fMP4 video configuration")?,
                );
            }
            if let Some(audio) = audio {
                identifiers.push(
                    codecs::aac_codec(audio).context("invalid native fMP4 AAC configuration")?,
                );
            }
            let init = fragment.with_extension("init.mp4");
            let uri = init
                .file_name()
                .and_then(|name| name.to_str())
                .context("invalid fMP4 init filename")?;

            publish_file(&init, |file| {
                file.write_all(&initialization)?;
                Ok(())
            })?;
            self.codecs = Some(identifiers.join(","));
            self.uri = Some(uri.to_owned());
            self.bytes = initialization;
        } else {
            ensure!(
                initialization == self.bytes,
                "native fMP4 initialization changed; restart the output with new codec parameters"
            );
        }

        publish_file(fragment, |file| {
            ensure!(
                io::copy(&mut input, file)? == end - init_end,
                "incomplete native fMP4 fragment"
            );
            Ok(())
        })
    }
}

pub(super) fn temporary_media(path: &Path) -> Result<tempfile::NamedTempFile> {
    let parent = path.parent().context("HLS media has no directory")?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".hls-");

    #[cfg(unix)]
    {
        use std::{fs::Permissions, os::unix::fs::PermissionsExt};

        builder.permissions(Permissions::from_mode(0o666));
    }

    builder
        .tempfile_in(parent)
        .with_context(|| format!("staging {}", path.display()))
}

pub(super) fn publish_media(temp: tempfile::NamedTempFile, path: &Path) -> Result<()> {
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path)
        .with_context(|| format!("publishing {}", path.display()))?;

    let directory = path.parent().context("HLS media has no directory")?;

    ff_hls::sync_directory(directory)
        .with_context(|| format!("synchronizing HLS directory {}", directory.display()))?;

    Ok(())
}

fn publish_file(path: &Path, write: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
    let mut file = temporary_media(path)?;

    write(file.as_file_mut())?;

    publish_media(file, path)
}

fn read_box(input: &mut (impl Read + Seek), end: u64) -> Result<([u8; 4], u64)> {
    let start = input.stream_position()?;
    let mut header = [0; 8];
    input.read_exact(&mut header)?;
    let size = match u32::from_be_bytes(header[..4].try_into()?) {
        0 => end.saturating_sub(start),
        1 => {
            let mut extended = [0; 8];
            input.read_exact(&mut extended)?;
            u64::from_be_bytes(extended)
        }
        size => u64::from(size),
    };
    let box_end = start
        .checked_add(size)
        .context("native fMP4 box overflow")?;
    ensure!(
        box_end >= input.stream_position()? && box_end <= end,
        "invalid native fMP4 box size"
    );

    Ok((header[4..].try_into()?, box_end))
}

fn boxes(mut bytes: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    std::iter::from_fn(move || {
        let size = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
        let payload = bytes.get(8..size)?;
        let tag = bytes.get(4..8)?;
        bytes = bytes.get(size..)?;
        Some((tag, payload))
    })
}

fn child<'a>(bytes: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(bytes).find_map(|(name, value)| (name == tag).then_some(value))
}

fn video_codec(initialization: &[u8]) -> Option<String> {
    boxes(child(initialization, b"moov")?).find_map(|(tag, track)| {
        if tag != b"trak" {
            return None;
        }
        let samples = child(
            child(child(child(track, b"mdia")?, b"minf")?, b"stbl")?,
            b"stsd",
        )?;
        boxes(samples.get(8..)?).find_map(|(tag, entry)| {
            let extra = entry.get(78..)?;
            match tag {
                b"avc1" => codecs::avc_codec(child(extra, b"avcC")?),
                b"hvc1" => codecs::hevc_codec(child(extra, b"hvcC")?),
                b"av01" => codecs::av1_codec(child(extra, b"av1C")?),
                _ => None,
            }
        })
    })
}
