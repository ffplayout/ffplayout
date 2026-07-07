use ff_engine::{
    RgbaColor, TextBackgroundConfig, TextConfig, TextPosition, TextScroll, TextWeight,
};

use crate::db::models::TextPreset;

pub fn text_config(preset: &TextPreset, text: Option<String>, use_filename: bool) -> TextConfig {
    let mut text_color = parse_color(&preset.text_color, RgbaColor::opaque(255, 255, 255));
    text_color.a = opacity_to_alpha(preset.text_opacity);

    let background = preset.background_enabled.then(|| {
        let mut color = parse_color(
            &preset.background_color,
            RgbaColor {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
        );
        color.a = opacity_to_alpha(preset.background_opacity);
        TextBackgroundConfig {
            color,
            padding: preset.background_padding,
        }
    });

    TextConfig {
        text,
        use_filename,
        filename_regex: (!preset.filename_regex.trim().is_empty())
            .then(|| preset.filename_regex.clone()),
        font_family: (!preset.font_family.trim().is_empty()).then(|| preset.font_family.clone()),
        font_weight: match preset.font_weight.as_str() {
            "semibold" => TextWeight::Semibold,
            "bold" => TextWeight::Bold,
            _ => TextWeight::Normal,
        },
        font_size: preset.font_size,
        line_spacing: preset.line_spacing,
        text_color,
        opacity: preset.opacity,
        position_x: parse_position(&preset.position_x),
        position_y: parse_position(&preset.position_y),
        background,
        scroll: match preset.scroll_direction.as_str() {
            "left_to_right" => TextScroll::LeftToRight {
                pixels_per_second: preset.scroll_speed,
            },
            "right_to_left" => TextScroll::RightToLeft {
                pixels_per_second: preset.scroll_speed,
            },
            _ => TextScroll::None,
        },
        scroll_repeat: preset.scroll_repeat,
        fade_in_seconds: preset.fade_in_seconds,
        fade_out_seconds: preset.fade_out_seconds,
    }
}

fn parse_position(value: &str) -> TextPosition {
    let value = value.trim();
    if value.eq_ignore_ascii_case("center") {
        return TextPosition::Center;
    }
    if let Some(offset) = value.strip_prefix("end:").and_then(|v| v.parse().ok()) {
        return TextPosition::End(offset);
    }
    value
        .parse()
        .map(TextPosition::Pixels)
        .unwrap_or(TextPosition::Pixels(0))
}

fn parse_color(value: &str, fallback: RgbaColor) -> RgbaColor {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return fallback;
    }
    let Ok(rgb) = u32::from_str_radix(hex, 16) else {
        return fallback;
    };
    RgbaColor {
        r: ((rgb >> 16) & 0xff) as u8,
        g: ((rgb >> 8) & 0xff) as u8,
        b: (rgb & 0xff) as u8,
        a: 255,
    }
}

fn opacity_to_alpha(opacity: f64) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
}
