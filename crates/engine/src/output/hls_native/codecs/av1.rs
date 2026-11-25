pub(super) fn codec(config: &[u8]) -> Option<String> {
    if config.len() < 4 || config[0] != 0x81 {
        return None;
    }
    let profile = config[1] >> 5;
    let level = config[1] & 31;
    let tier = if config[2] & 0x80 == 0 { 'M' } else { 'H' };
    let depth = match config[2] & 0x60 {
        0 => 8,
        0x40 => 10,
        0x60 => 12,
        _ => return None,
    };

    Some(format!("av01.{profile}.{level:02}{tier}.{depth:02}"))
}
