//! Text playback evaluated from canonical program-frame indices, without a backend.
use crate::style::{TextPosition, TextScroll, TextStyle};

#[derive(Clone, Copy, Debug, Default)]
pub struct TextTiming {
    pub fade_start_frame: i64,
    pub scroll_start_frame: i64,
    pub end_frame: Option<i64>,
}

#[derive(Clone, Copy, Debug)]
pub struct TextPreparation {
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub frames_per_second: u32,
    pub timing: TextTiming,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextPlacement {
    pub x: i32,
    pub y: i32,
    pub opacity: f64,
}

pub struct TextPlayback {
    scroll: TextScroll,
    scroll_repeat: i32,
    fade_in_frames: i64,
    fade_out_frames: i64,
    fade_start_frame: i64,
    scroll_start_frame: i64,
    end_frame: Option<i64>,
    base_x: i32,
    base_y: i32,
    output_width: u32,
    overlay_width: u32,
    fps: u32,
}

impl TextPlayback {
    pub fn new(style: &TextStyle, context: TextPreparation, dimensions: (u32, u32)) -> Self {
        Self {
            scroll: style.scroll,
            scroll_repeat: style.scroll_repeat,
            fade_in_frames: seconds_to_frames(style.fade_in_seconds, context.frames_per_second),
            fade_out_frames: seconds_to_frames(style.fade_out_seconds, context.frames_per_second),
            fade_start_frame: context.timing.fade_start_frame,
            scroll_start_frame: context.timing.scroll_start_frame,
            end_frame: context.timing.end_frame,
            base_x: text_position(style.position_x, context.canvas_width, dimensions.0),
            base_y: text_position(style.position_y, context.canvas_height, dimensions.1),
            output_width: context.canvas_width,
            overlay_width: dimensions.0,
            fps: context.frames_per_second,
        }
    }

    pub fn evaluate(&self, program_frame: i64, scroll_frame: i64) -> TextPlacement {
        TextPlacement {
            x: self.x_at(scroll_frame),
            y: self.base_y,
            opacity: self.opacity_at(program_frame),
        }
    }

    fn opacity_at(&self, program_frame: i64) -> f64 {
        let mut opacity: f64 = 1.0;

        if self.fade_in_frames > 0 {
            let elapsed = (program_frame - self.fade_start_frame).max(0);
            opacity = opacity.min(elapsed as f64 / self.fade_in_frames as f64);
        }

        if self.fade_out_frames > 0
            && let Some(end_frame) = self.end_frame
        {
            let remaining = (end_frame - program_frame).max(0);
            opacity = opacity.min(remaining as f64 / self.fade_out_frames as f64);
        }
        opacity.clamp(0.0, 1.0)
    }

    fn x_at(&self, program_frame: i64) -> i32 {
        let elapsed = (program_frame - self.scroll_start_frame).max(0);

        match self.scroll {
            TextScroll::None => self.base_x,
            TextScroll::LeftToRight { pixels_per_second } => {
                let offset = self.scroll_offset(elapsed, pixels_per_second);
                -i32::try_from(self.overlay_width).unwrap_or(i32::MAX)
                    + i32::try_from(offset).unwrap_or(i32::MAX)
            }
            TextScroll::RightToLeft { pixels_per_second } => {
                let offset = self.scroll_offset(elapsed, pixels_per_second);
                i32::try_from(self.output_width).unwrap_or(i32::MAX)
                    - i32::try_from(offset).unwrap_or(i32::MAX)
            }
        }
    }

    fn scroll_offset(&self, elapsed: i64, pixels_per_second: u32) -> i64 {
        let offset = elapsed * i64::from(pixels_per_second) / i64::from(self.fps.max(1));
        let travel = i64::from(self.output_width) + i64::from(self.overlay_width);

        if travel <= 0 {
            return offset;
        }

        if self.scroll_repeat < 0 {
            return offset % travel;
        }

        let cycles = i64::from(self.scroll_repeat) + 1;

        if offset < travel.saturating_mul(cycles) {
            offset % travel
        } else {
            offset
        }
    }
}

fn text_position(position: TextPosition, output: u32, overlay: u32) -> i32 {
    match position {
        TextPosition::Pixels(value) => value,
        TextPosition::Center => ((output as i32 - overlay as i32) / 2).max(0),
        TextPosition::End(offset) => output as i32 - overlay as i32 - offset,
    }
}

fn seconds_to_frames(seconds: f64, fps: u32) -> i64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        0
    } else {
        (seconds * f64::from(fps)).round().max(1.0) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_is_frame_rate_independent_and_uses_separate_clocks() {
        let style = TextStyle {
            fade_in_seconds: 2.0,
            fade_out_seconds: 2.0,
            scroll: TextScroll::RightToLeft {
                pixels_per_second: 100,
            },
            position_y: TextPosition::End(8),
            ..TextStyle::default()
        };
        for fps in [25, 50] {
            let fps_frame = i64::from(fps);
            let context = TextPreparation {
                canvas_width: 320,
                canvas_height: 180,
                frames_per_second: fps,
                timing: TextTiming {
                    fade_start_frame: 3 * fps_frame,
                    scroll_start_frame: 7 * fps_frame,
                    end_frame: Some(10 * fps_frame),
                },
            };
            let playback = TextPlayback::new(&style, context, (40, 20));
            assert_eq!(
                playback.evaluate(4 * fps_frame, 8 * fps_frame),
                TextPlacement {
                    x: 220,
                    y: 152,
                    opacity: 0.5
                }
            );
            assert_eq!(playback.evaluate(5 * fps_frame, 8 * fps_frame).opacity, 1.0);
            assert_eq!(playback.evaluate(9 * fps_frame, 8 * fps_frame).opacity, 0.5);
            assert_eq!(
                playback.evaluate(10 * fps_frame, 8 * fps_frame).opacity,
                0.0
            );
            assert_eq!(
                playback.evaluate(2 * fps_frame, 6 * fps_frame),
                TextPlacement {
                    x: 320,
                    y: 152,
                    opacity: 0.0
                }
            );
        }
    }

    #[test]
    fn finite_scrolling_exits_while_repeating_scrolling_wraps_without_state() {
        let mut style = TextStyle {
            scroll: TextScroll::RightToLeft {
                pixels_per_second: 100,
            },
            scroll_repeat: 0,
            ..TextStyle::default()
        };
        let context = TextPreparation {
            canvas_width: 320,
            canvas_height: 180,
            frames_per_second: 25,
            timing: TextTiming::default(),
        };
        let finite = TextPlayback::new(&style, context, (40, 20));
        assert_eq!(finite.evaluate(100, 100).x, -80);
        style.scroll_repeat = -1;
        let repeating = TextPlayback::new(&style, context, (40, 20));
        assert_eq!(repeating.evaluate(100, 100).x, 280);
        assert_eq!(repeating.evaluate(0, 0).x, 320);
        assert_eq!(repeating.evaluate(100, 100).x, 280);
    }
}
