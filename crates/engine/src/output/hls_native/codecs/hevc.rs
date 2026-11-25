use std::fmt::Write as _;

pub(super) fn codec(config: &[u8]) -> Option<String> {
    if config.len() < 23 || config[0] != 1 {
        return None;
    }
    let profile = config[1] & 31;
    let space = ["", "A", "B", "C"][(config[1] >> 6) as usize];
    let tier = if config[1] & 32 == 0 { 'L' } else { 'H' };
    let flags = u32::from_be_bytes(config[2..6].try_into().ok()?).reverse_bits();
    let level = config[12];
    let mut declaration = format!("hvc1.{space}{profile}.{flags:X}.{tier}{level}");
    let constraints = &config[6..12];
    if let Some(last) = constraints.iter().rposition(|value| *value != 0) {
        for value in &constraints[..=last] {
            write!(declaration, ".{value:02X}").ok()?;
        }
    }

    Some(declaration)
}
