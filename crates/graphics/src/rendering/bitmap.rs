//! CPU RGBA bitmaps with an explicit alpha contract.

/// Tightly packed, row-major 8-bit RGBA with **straight** (unassociated) alpha.
/// RGB remains in the renderer's encoded color domain; this type does not imply
/// linear-light blending. Transparent pixels produced by the renderer are zero.
pub struct StraightRgbaBitmap {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Source-over for straight-alpha RGBA, including a translucent destination.
/// Dividing by the resulting coverage keeps RGB unassociated for later consumers.
pub fn source_over_straight(destination: &mut [u8; 4], source: [u8; 4]) {
    let source_alpha = u32::from(source[3]);
    let destination_alpha = u32::from(destination[3]);
    let inverse = 255 - source_alpha;
    let coverage = source_alpha * 255 + destination_alpha * inverse;

    if coverage == 0 {
        *destination = [0; 4];
        return;
    }

    for channel in 0..3 {
        let weighted = u32::from(source[channel]) * source_alpha * 255
            + u32::from(destination[channel]) * destination_alpha * inverse;
        destination[channel] = ((weighted + coverage / 2) / coverage) as u8;
    }
    destination[3] = ((coverage + 127) / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::source_over_straight;

    #[test]
    fn translucent_white_retains_its_color_on_transparent_pixels() {
        let mut result = [0; 4];
        source_over_straight(&mut result, [255, 255, 255, 128]);
        assert_eq!(result, [255, 255, 255, 128]);
    }

    #[test]
    fn overlapping_translucent_layers_match_a_high_precision_reference() {
        let mut result = [20, 100, 220, 90];
        let source = [240, 80, 30, 140];
        let sa = f64::from(source[3]) / 255.0;
        let da = f64::from(result[3]) / 255.0;
        let alpha = sa + da * (1.0 - sa);
        let expected: [u8; 4] = std::array::from_fn(|channel| {
            if channel == 3 {
                (alpha * 255.0).round() as u8
            } else {
                ((f64::from(source[channel]) * sa + f64::from(result[channel]) * da * (1.0 - sa))
                    / alpha)
                    .round() as u8
            }
        });
        source_over_straight(&mut result, source);
        assert_eq!(result, expected);
    }

    #[test]
    fn transparent_and_opaque_endpoints_are_exact() {
        let mut result = [40, 80, 120, 70];
        source_over_straight(&mut result, [255, 0, 0, 0]);
        assert_eq!(result, [40, 80, 120, 70]);
        source_over_straight(&mut result, [20, 30, 40, 255]);
        assert_eq!(result, [20, 30, 40, 255]);
        let mut transparent = [200, 100, 50, 0];
        source_over_straight(&mut transparent, [255, 255, 255, 0]);
        assert_eq!(transparent, [0; 4]);
    }
}
