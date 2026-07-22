//! Precomputed 32x32 RGBA rendition of frontend/public/favicon.ico.
//!
//! The hexadecimal raw pixels avoid a runtime image decoder and therefore an
//! additional desktop dependency.

pub(super) const DESKTOP_ICON_WIDTH: u32 = 32;
pub(super) const DESKTOP_ICON_HEIGHT: u32 = 32;
const DESKTOP_ICON_HEX: &[u8] = include_bytes!("icon.rgba.hex");

pub(super) fn desktop_icon_rgba() -> Vec<u8> {
    let mut rgba = Vec::with_capacity((DESKTOP_ICON_WIDTH * DESKTOP_ICON_HEIGHT * 4) as usize);
    let mut high_nibble = None;

    for byte in DESKTOP_ICON_HEX.iter().copied() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if let Some(high) = high_nibble.take() {
            rgba.push(high << 4 | hex_digit(byte));
        } else {
            high_nibble = Some(hex_digit(byte));
        }
    }

    debug_assert!(high_nibble.is_none());
    debug_assert_eq!(
        rgba.len(),
        (DESKTOP_ICON_WIDTH * DESKTOP_ICON_HEIGHT * 4) as usize
    );
    rgba
}

fn hex_digit(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => unreachable!("desktop icon contains a non-hexadecimal byte"),
    }
}
