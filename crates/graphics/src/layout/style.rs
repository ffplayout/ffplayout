//! Renderer-independent layout and styling; no pipeline or backend handles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWeight {
    #[default]
    Normal,
    Semibold,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextPosition {
    Pixels(i32),
    Center,
    End(i32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBackgroundConfig {
    pub color: RgbaColor,
    pub padding: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextScroll {
    None,
    LeftToRight { pixels_per_second: u32 },
    RightToLeft { pixels_per_second: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogoStyle {
    pub scale: Option<String>,
    pub opacity: f64,
    pub position: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub font_family: Option<String>,
    pub font_weight: TextWeight,
    pub font_size: f32,
    pub line_spacing: f32,
    pub text_color: RgbaColor,
    pub opacity: f64,
    pub position_x: TextPosition,
    pub position_y: TextPosition,
    pub background: Option<TextBackgroundConfig>,
    pub scroll: TextScroll,
    pub scroll_repeat: i32,
    pub fade_in_seconds: f64,
    pub fade_out_seconds: f64,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: None,
            font_weight: TextWeight::Normal,
            font_size: 48.0,
            line_spacing: 0.0,
            text_color: RgbaColor::opaque(255, 255, 255),
            opacity: 1.0,
            position_x: TextPosition::Pixels(32),
            position_y: TextPosition::Pixels(32),
            background: None,
            scroll: TextScroll::None,
            scroll_repeat: -1,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
        }
    }
}
