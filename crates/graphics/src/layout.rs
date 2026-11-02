//! CPU/GPU-independent logo sizing and placement compatibility rules.
use anyhow::{Result, anyhow};

#[cfg(any(feature = "ffmpeg", test))]
use crate::even;

#[cfg(any(feature = "ffmpeg", test))]
pub(crate) fn logo_dimensions(
    scale: Option<&str>,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
) -> Result<(u32, u32)> {
    let Some(scale) = scale.filter(|scale| !scale.trim().is_empty()) else {
        return Ok((even(input_width).max(2), even(input_height).max(2)));
    };
    let (width, height) = scale
        .split_once(':')
        .or_else(|| scale.split_once('x'))
        .ok_or_else(|| anyhow!("logo scale must use WIDTH:HEIGHT or WIDTHxHEIGHT"))?;
    let width = parse_logo_dimension(width, input_width, output_width)?;
    let height = parse_logo_dimension(height, input_height, output_height)?;

    let (width, height) = match (width, height) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (
            width,
            ((u64::from(width) * u64::from(input_height)) / u64::from(input_width)) as u32,
        ),
        (None, Some(height)) => (
            ((u64::from(height) * u64::from(input_width)) / u64::from(input_height)) as u32,
            height,
        ),
        (None, None) => (input_width, input_height),
    };

    Ok((even(width).max(2), even(height).max(2)))
}

#[cfg(any(feature = "ffmpeg", test))]
fn parse_logo_dimension(value: &str, input: u32, output: u32) -> Result<Option<u32>> {
    let value = value.trim();

    if value == "-1" {
        return Ok(None);
    }

    if value == "iw" || value == "ih" {
        return Ok(Some(input));
    }

    if value == "W" || value == "H" || value == "main_w" || value == "main_h" {
        return Ok(Some(output));
    }

    if let Some(percent) = value.strip_suffix('%') {
        let percent = percent
            .trim()
            .parse::<f64>()
            .map_err(|_| anyhow!("unsupported logo scale expression {value:?}"))?;
        if !percent.is_finite() || percent <= 0.0 {
            return Err(anyhow!("unsupported logo scale expression {value:?}"));
        }

        return Ok(Some(((f64::from(output) * percent) / 100.0).round() as u32));
    }
    value
        .parse::<u32>()
        .map(Some)
        .map_err(|_| anyhow!("unsupported logo scale expression {value:?}"))
}

pub fn logo_position(
    position: &str,
    output_width: u32,
    output_height: u32,
    logo_width: u32,
    logo_height: u32,
) -> Result<(u32, u32)> {
    let (x, y) = position
        .split_once(':')
        .ok_or_else(|| anyhow!("logo position must use X:Y"))?;
    let x = eval_position_expr(x, output_width, logo_width)?;
    let y = eval_position_expr(y, output_height, logo_height)?;

    Ok((
        x.clamp(0, i64::from(output_width.saturating_sub(logo_width))) as u32,
        y.clamp(0, i64::from(output_height.saturating_sub(logo_height))) as u32,
    ))
}

fn eval_position_expr(expr: &str, main: u32, overlay: u32) -> Result<i64> {
    let normalized = expr
        .replace("main_w", "M")
        .replace("main_h", "M")
        .replace("overlay_w", "O")
        .replace("overlay_h", "O")
        .replace(['W', 'H'], "M")
        .replace(['w', 'h'], "O")
        .replace('-', "+-");
    let mut total = 0_i64;

    for part in normalized.split('+') {
        let part = part.trim();

        if part.is_empty() {
            continue;
        }

        let (sign, part) = part
            .strip_prefix('-')
            .map_or((1_i64, part), |part| (-1_i64, part));
        let value = match part {
            "M" => i64::from(main),
            "O" => i64::from(overlay),
            _ => part
                .parse::<i64>()
                .map_err(|_| anyhow!("unsupported logo position expression {expr:?}"))?,
        };
        total += sign * value;
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::logo_dimensions;

    #[test]
    fn logo_scale_accepts_output_percentages() {
        assert_eq!(
            logo_dimensions(Some("12%:-1"), 400, 200, 1920, 1080).unwrap(),
            (230, 114)
        );
        assert_eq!(
            logo_dimensions(Some("-1:10%"), 400, 200, 1920, 1080).unwrap(),
            (216, 108)
        );
    }

    #[test]
    fn logo_scale_rejects_invalid_percentages() {
        assert!(logo_dimensions(Some("0%:-1"), 400, 200, 1920, 1080).is_err());
        assert!(logo_dimensions(Some("nan%:-1"), 400, 200, 1920, 1080).is_err());
    }
}
