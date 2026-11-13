fn sps_codec(nal: &[u8]) -> Option<[u8; 3]> {
    if nal.len() < 5 || nal[0] & 0x9f != 7 || nal[0] & 0x60 == 0 {
        return None;
    }

    let codec = [nal[1], nal[2], nal[3]];

    // Reserved constraint bits and unspecified profile/level cannot identify AVC.
    if codec[0] == 0 || codec[1] & 3 != 0 || codec[2] == 0 {
        return None;
    }

    Some(codec)
}

fn take_nal<'a>(data: &mut &'a [u8]) -> Option<&'a [u8]> {
    let size = usize::from(u16::from_be_bytes(data.get(..2)?.try_into().ok()?));
    let end = 2_usize.checked_add(size)?;
    let nal = data.get(2..end)?;
    *data = data.get(end..)?;

    Some(nal)
}

fn avcc_codec(data: &[u8]) -> Option<[u8; 3]> {
    if data.len() < 7 || data[4] & 0xfc != 0xfc || data[4] & 3 == 2 || data[5] & 0xe0 != 0xe0 {
        return None;
    }

    let expected = [data[1], data[2], data[3]];
    let count = data[5] & 0x1f;
    let mut nals = &data[6..];

    if count == 0 {
        return None;
    }

    for _ in 0..count {
        if sps_codec(take_nal(&mut nals)?)? != expected {
            return None;
        }
    }

    let (&count, rest) = nals.split_first()?;
    nals = rest;

    if count == 0 {
        return None;
    }

    for _ in 0..count {
        let nal = take_nal(&mut nals)?;

        if nal.len() < 2 || nal[0] & 0x9f != 8 || nal[0] & 0x60 == 0 {
            return None;
        }
    }

    Some(expected)
}

fn start_code(data: &[u8]) -> Option<usize> {
    data.windows(3).position(|bytes| bytes == [0, 0, 1])
}

pub(super) fn codec(data: &[u8]) -> Option<String> {
    let value = if data.first() == Some(&1) {
        avcc_codec(data)?
    } else {
        let start = start_code(data)?;

        if data[..start].iter().any(|byte| *byte != 0) {
            return None;
        }

        let mut rest = &data[start + 3..];
        let mut found = None;

        loop {
            let next = start_code(rest);
            let nal = &rest[..next.unwrap_or(rest.len())];

            if nal.first().is_some_and(|byte| byte & 0x1f == 7) {
                let value = sps_codec(nal)?;

                if found.is_some_and(|previous| previous != value) {
                    return None;
                }

                found = Some(value);
            }

            let Some(next) = next else { break };
            rest = &rest[next + 3..];
        }

        found?
    };

    Some(format!(
        "avc1.{:02x}{:02x}{:02x}",
        value[0], value[1], value[2]
    ))
}
