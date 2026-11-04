use std::path::Path;

use anyhow::{Result, anyhow, ensure};
use ff_graphics::playback::Transition;
use serde_json::Value;

use crate::file::norm_abs_path;

use super::super::{GraphicAction, GraphicEvent, microseconds};

/// Validate saved definitions without opening media or starting decoder workers.
pub(super) fn validate(settings: &Value, root: &Path) -> Result<()> {
    ensure!(
        settings.to_string().len() <= 64 * 1024,
        "Graphic settings are too large"
    );
    let parsed: GraphicEvent = serde_json::from_value(settings.clone())?;
    let GraphicAction::Take {
        layer,
        source,
        size,
        opacity,
        fade_in,
        hold,
        duration,
        fade_out,
        pause,
        repeat,
        ..
    } = parsed.action
    else {
        anyhow::bail!("A saved graphic must contain a Take definition");
    };
    ensure!(layer <= 9_007_199_254_740_991, "Invalid layer ID");
    ensure!(
        !source.trim().is_empty() && !source.contains("://"),
        "A local graphic source is required"
    );
    norm_abs_path(root, &source).map_err(|error| anyhow!(error.to_string()))?;
    ensure!(
        opacity.is_finite() && (0.0..=1.0).contains(&opacity),
        "Invalid opacity"
    );
    ensure!(
        duration.is_none() || hold.is_none(),
        "Use duration or hold, not both"
    );

    if let Some((width, height)) = size {
        ensure!(
            (2..=8192).contains(&width)
                && (2..=8192).contains(&height)
                && width.is_multiple_of(2)
                && height.is_multiple_of(2),
            "Graphic size must be even and within 2..8192"
        );
    }

    let mut transition = Transition {
        fade_in_us: microseconds(fade_in)? as u64,
        hold_us: hold
            .map(microseconds)
            .transpose()?
            .map(|value| value as u64),
        fade_out_us: microseconds(fade_out)? as u64,
        pause_us: microseconds(pause)? as u64,
        repeat: repeat.playback(),
    };

    if let Some(duration) = duration {
        transition = transition.within(microseconds(duration)? as u64);
    }

    transition.validate()
}
