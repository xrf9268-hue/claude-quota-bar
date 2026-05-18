//! Color palette. One default theme (`graphite`) tuned to match the
//! reference screenshot. Severity colors flip green→yellow→red at 30%/70%.

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Battery-bar background when usage is comfortable.
    pub bg_ok: [u8; 3],
    /// Battery-bar background when usage crosses the warning threshold.
    pub bg_warn: [u8; 3],
    /// Battery-bar background when usage crosses the critical threshold.
    pub bg_hot: [u8; 3],
    /// Battery-bar background for unfilled cells (dark gutter).
    pub bg_empty: [u8; 3],
    /// Foreground for text overlaid on the battery bar.
    pub text_on_bar: [u8; 3],
    /// Muted color for separators and bracket framing.
    pub mute: [u8; 3],
    /// Ink for non-bar text (reset countdown, model, dir).
    pub ink: [u8; 3],
    /// Accent color for the model segment.
    pub model: [u8; 3],
    /// Accent color when the dirty file count is non-zero.
    pub warn: [u8; 3],
}

pub const WARN_THRESHOLD: f64 = 30.0;
pub const HOT_THRESHOLD: f64 = 70.0;

pub const GRAPHITE: Theme = Theme {
    bg_ok: [108, 167, 116],
    bg_warn: [232, 178, 96],
    bg_hot: [220, 100, 92],
    bg_empty: [60, 65, 72],
    text_on_bar: [238, 235, 224],
    mute: [120, 125, 132],
    ink: [200, 200, 200],
    model: [120, 220, 140],
    warn: [232, 178, 96],
};

impl Theme {
    pub fn severity_bg(&self, pct: f64) -> [u8; 3] {
        if pct >= HOT_THRESHOLD {
            self.bg_hot
        } else if pct >= WARN_THRESHOLD {
            self.bg_warn
        } else {
            self.bg_ok
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_warn_returns_ok_bg() {
        assert_eq!(GRAPHITE.severity_bg(0.0), GRAPHITE.bg_ok);
        assert_eq!(GRAPHITE.severity_bg(29.9), GRAPHITE.bg_ok);
    }

    #[test]
    fn at_or_above_warn_returns_warn_bg() {
        assert_eq!(GRAPHITE.severity_bg(WARN_THRESHOLD), GRAPHITE.bg_warn);
        assert_eq!(GRAPHITE.severity_bg(50.0), GRAPHITE.bg_warn);
        assert_eq!(GRAPHITE.severity_bg(69.9), GRAPHITE.bg_warn);
    }

    #[test]
    fn at_or_above_hot_returns_hot_bg() {
        assert_eq!(GRAPHITE.severity_bg(HOT_THRESHOLD), GRAPHITE.bg_hot);
        assert_eq!(GRAPHITE.severity_bg(100.0), GRAPHITE.bg_hot);
        assert_eq!(GRAPHITE.severity_bg(150.0), GRAPHITE.bg_hot);
    }
}
