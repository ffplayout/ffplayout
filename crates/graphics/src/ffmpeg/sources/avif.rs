//! AVIF auxiliary-image associations, which FFmpeg does not expose as stream metadata.
use std::{
    collections::HashMap,
    ffi::CStr,
    fs::File,
    io::{Read, Seek, SeekFrom},
};

use anyhow::{Context, Result, ensure};
use ffmpeg_next::format;

use crate::surface::Alpha;

const ALPHA_TYPE: &[u8] = b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0";
const METADATA_LIMIT: usize = 16 * 1024 * 1024;
const ENTRY_LIMIT: usize = 4096;

type BoxRef<'a> = ([u8; 4], &'a [u8]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Association {
    pub color: usize,
    pub alpha: usize,
    pub mode: Alpha,
}

pub(super) fn streams(input: &format::context::Input) -> Result<Option<Association>> {
    let metadata = input.metadata();
    let avif = ["major_brand", "compatible_brands"].iter().any(|key| {
        metadata
            .get(key)
            .is_some_and(|value| value.contains("avif") || value.contains("avis"))
    });

    if !avif {
        return Ok(None);
    }

    // AVFormatContext owns this NUL-terminated URL for the lifetime of Input.
    let url = unsafe {
        let pointer = (*input.as_ptr()).url;
        ensure!(!pointer.is_null(), "AVIF graphics source has no local path");
        CStr::from_ptr(pointer)
    }
    .to_str()
    .context("AVIF graphics path is not UTF-8")?;
    let mut file = File::open(url).context("opening AVIF auxiliary-image metadata")?;
    let mut metadata = Vec::new();
    let mut movie = Vec::new();
    let length = file.metadata()?.len();
    let mut count = 0;

    while file.stream_position()? < length {
        count += 1;
        ensure!(count <= ENTRY_LIMIT, "too many AVIF boxes");
        let start = file.stream_position()?;
        let mut header = [0; 8];
        file.read_exact(&mut header)?;
        let size = u32::from_be_bytes(header[..4].try_into()?) as u64;
        let (size, header_size) = if size == 1 {
            let mut extended = [0; 8];
            file.read_exact(&mut extended)?;
            (u64::from_be_bytes(extended), 16)
        } else if size == 0 {
            (length - start, 8)
        } else {
            (size, 8)
        };
        ensure!(
            size >= header_size && size <= length - start,
            "invalid AVIF box size"
        );
        let destination = match &header[4..] {
            b"meta" => Some(&mut metadata),
            b"moov" => Some(&mut movie),
            _ => None,
        };

        if let Some(destination) = destination {
            let size = usize::try_from(size - header_size)?;
            ensure!(size <= METADATA_LIMIT, "AVIF metadata exceeds 16 MiB");
            ensure!(destination.is_empty(), "duplicate AVIF metadata box");
            destination.resize(size, 0);
            file.read_exact(destination)?;
        }

        file.seek(SeekFrom::Start(start + size))?;
    }

    let pair = if movie.is_empty() {
        let mut children = metadata.as_slice();
        take(&mut children, 4)?;
        let mut primary = child(children, b"pitm")?.context("AVIF has no primary image")?;
        let version = integer(&mut primary, 1)?;
        take(&mut primary, 3)?;
        let color = integer(&mut primary, if version == 0 { 2 } else { 4 })? as u32;
        item_alpha(&metadata, color)?.map(|(alpha, mode)| (color, alpha, mode))
    } else {
        track_alpha(&movie)?
    };

    pair.map(|(color, alpha, mode)| {
        ensure!(color != alpha, "AVIF alpha cannot reference itself");
        // Recent FFmpeg versions expose both poster items and animation tracks
        // with the same IDs. Select the timed track rather than its poster.
        let index = |id| {
            input
                .streams()
                .filter(|stream| stream.id() as u32 == id)
                .max_by_key(|stream| (stream.frames(), stream.duration(), stream.index()))
                .map(|stream| stream.index())
                .context("AVIF color/alpha image is not exposed by this FFmpeg demuxer")
        };

        Ok(Association {
            color: index(color)?,
            alpha: index(alpha)?,
            mode,
        })
    })
    .transpose()
}

fn boxes(mut bytes: &[u8]) -> Result<Vec<BoxRef<'_>>> {
    let mut result = Vec::new();

    while !bytes.is_empty() {
        ensure!(result.len() < ENTRY_LIMIT, "too many AVIF metadata entries");
        let size = integer(&mut bytes, 4)? as usize;
        let kind: [u8; 4] = take(&mut bytes, 4)?.try_into()?;
        let (size, header) = if size == 1 {
            (usize::try_from(integer(&mut bytes, 8)?)?, 16)
        } else if size == 0 {
            (bytes.len() + 8, 8)
        } else {
            (size, 8)
        };
        ensure!(size >= header, "invalid AVIF metadata box size");
        result.push((kind, take(&mut bytes, size - header)?));
    }

    Ok(result)
}

fn child<'a>(bytes: &'a [u8], kind: &[u8; 4]) -> Result<Option<&'a [u8]>> {
    Ok(boxes(bytes)?
        .into_iter()
        .find(|(name, _)| name == kind)
        .map(|(_, data)| data))
}

fn take<'a>(bytes: &mut &'a [u8], size: usize) -> Result<&'a [u8]> {
    ensure!(size <= bytes.len(), "truncated AVIF metadata");
    let (value, rest) = bytes.split_at(size);
    *bytes = rest;

    Ok(value)
}

fn integer(bytes: &mut &[u8], size: usize) -> Result<u64> {
    Ok(take(bytes, size)?
        .iter()
        .fold(0, |number, byte| (number << 8) | u64::from(*byte)))
}

fn item_alpha(metadata: &[u8], color: u32) -> Result<Option<(u32, Alpha)>> {
    let mut metadata = metadata;
    take(&mut metadata, 4)?; // meta FullBox header
    let Some(properties) = child(metadata, b"iprp")? else {
        return Ok(None);
    };
    let Some(property_boxes) = child(properties, b"ipco")? else {
        return Ok(None);
    };
    let property_boxes = boxes(property_boxes)?;
    let mut associations = HashMap::new();

    for (_, mut data) in boxes(properties)?
        .into_iter()
        .filter(|(name, _)| name == b"ipma")
    {
        let version = integer(&mut data, 1)?;
        let flags = integer(&mut data, 3)?;
        let count = integer(&mut data, 4)?;
        ensure!(
            count <= ENTRY_LIMIT as u64,
            "too many AVIF item associations"
        );

        for _ in 0..count {
            let id = integer(&mut data, if version == 0 { 2 } else { 4 })? as u32;
            let count = integer(&mut data, 1)?;
            let mut alpha = false;

            for _ in 0..count {
                let index = integer(&mut data, if flags & 1 == 0 { 1 } else { 2 })? as usize
                    & if flags & 1 == 0 { 0x7f } else { 0x7fff };

                if index > 0 {
                    let (kind, value) = property_boxes
                        .get(index - 1)
                        .context("invalid AVIF property index")?;
                    alpha |= kind == b"auxC" && value.get(4..) == Some(ALPHA_TYPE);
                }
            }

            *associations.entry(id).or_insert(false) |= alpha;
            ensure!(associations.len() <= ENTRY_LIMIT, "too many AVIF items");
        }
    }

    let Some(mut references) = child(metadata, b"iref")? else {
        return Ok(None);
    };
    let version = integer(&mut references, 1)?;
    take(&mut references, 3)?;
    let mut alpha = None;
    let mut premultiplied = Vec::new();

    for (kind, mut reference) in boxes(references)? {
        let source = integer(&mut reference, if version == 0 { 2 } else { 4 })? as u32;
        let count = integer(&mut reference, 2)?;

        for _ in 0..count {
            let target = integer(&mut reference, if version == 0 { 2 } else { 4 })? as u32;

            if kind == *b"auxl" && target == color && associations.get(&source) == Some(&true) {
                ensure!(alpha.is_none(), "multiple AVIF alpha images");
                alpha = Some(source);
            }

            if kind == *b"prem" && source == color {
                premultiplied.push(target);
            }
        }
    }

    Ok(alpha.map(|id| {
        (
            id,
            if premultiplied.contains(&id) {
                Alpha::PremultipliedEncoded
            } else {
                Alpha::Straight
            },
        )
    }))
}

fn track_alpha(movie: &[u8]) -> Result<Option<(u32, u32, Alpha)>> {
    let mut alpha = None;
    let mut premultiplied = Vec::new();

    for (_, track) in boxes(movie)?
        .into_iter()
        .filter(|(kind, _)| kind == b"trak")
    {
        let mut header = child(track, b"tkhd")?.context("AVIF track has no header")?;
        let version = integer(&mut header, 1)?;
        take(&mut header, if version == 1 { 19 } else { 11 })?;
        let id = integer(&mut header, 4)? as u32;
        let references = child(track, b"tref")?.unwrap_or_default();

        if let Some(mut data) = child(references, b"prem")? {
            while !data.is_empty() {
                premultiplied.push((id, integer(&mut data, 4)? as u32));
            }
        }

        let Some(mut targets) = child(references, b"auxl")? else {
            continue;
        };
        let color = integer(&mut targets, 4)? as u32;
        ensure!(
            targets.is_empty(),
            "AVIF alpha track must reference one color track"
        );
        let mut sample = Some(track);

        for kind in [b"mdia", b"minf", b"stbl", b"stsd"] {
            sample = sample.map(|data| child(data, kind)).transpose()?.flatten();
        }

        let Some(mut sample) = sample else { continue };
        take(&mut sample, 8)?; // stsd FullBox header and entry count

        for (_, mut entry) in boxes(sample)?
            .into_iter()
            .filter(|(kind, _)| kind == b"av01")
        {
            take(&mut entry, 78)?; // VisualSampleEntry

            if child(entry, b"auxi")?.and_then(|data| data.get(4..)) == Some(ALPHA_TYPE) {
                ensure!(alpha.is_none(), "multiple AVIF alpha tracks");
                alpha = Some((color, id));
            }
        }
    }

    Ok(alpha.map(|(color, alpha)| {
        (
            color,
            alpha,
            if premultiplied.contains(&(color, alpha)) {
                Alpha::PremultipliedEncoded
            } else {
                Alpha::Straight
            },
        )
    }))
}

#[cfg(test)]
mod tests;
