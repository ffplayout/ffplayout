use super::bits::Bits;

fn object_type(bits: &mut Bits<'_>) -> Option<u32> {
    match bits.read(5)? {
        31 => Some(32 + bits.read(6)?),
        value => Some(value),
    }
}

fn frequency(bits: &mut Bits<'_>) -> Option<()> {
    match bits.read(4)? {
        0..=12 => Some(()),
        15 if bits.read(24)? != 0 => Some(()),
        _ => None,
    }
}

pub(super) fn codec(data: &[u8]) -> Option<String> {
    let mut bits = Bits::new(data);
    let mut audio_type = object_type(&mut bits)?;

    frequency(&mut bits)?;

    let channels = bits.read(4)?;

    // Program Config Elements and other audio object syntaxes need a fuller parser.
    if !(1..=7).contains(&channels) || !matches!(audio_type, 2 | 5 | 29) {
        return None;
    }

    let explicit_sbr = audio_type != 2;

    if explicit_sbr {
        frequency(&mut bits)?;

        if object_type(&mut bits)? != 2 {
            return None;
        }
    }

    // GASpecificConfig for the AAC-LC core, including a possible core coder delay.
    bits.read(1)?;

    if bits.read(1)? != 0 {
        bits.read(14)?;
    }

    if bits.read(1)? != 0 {
        return None;
    }

    // Implicit SBR/PS signalling is common in encoder AudioSpecificConfig data.
    if !explicit_sbr && bits.remaining() >= 16 {
        let sync = bits.read(11)?;

        if sync != 0 {
            if sync != 0x2b7 || object_type(&mut bits)? != 5 {
                return None;
            }

            if bits.read(1)? != 0 {
                frequency(&mut bits)?;
                audio_type = 5;

                if bits.remaining() >= 12 {
                    let sync = bits.read(11)?;

                    if sync != 0 {
                        if sync != 0x548 {
                            return None;
                        }

                        if bits.read(1)? != 0 {
                            audio_type = 29;
                        }
                    }
                }
            }
        }
    }

    // Do not reinterpret truncated extensions or unknown trailing syntax as LC.
    while bits.remaining() != 0 {
        if bits.read(1)? != 0 {
            return None;
        }
    }

    Some(format!("mp4a.40.{audio_type}"))
}
