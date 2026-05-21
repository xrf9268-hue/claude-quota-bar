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
    /// Foreground for the percentage text where it lands on a filled
    /// (light-colored) severity cell. Dark so the text is readable.
    pub text_on_filled: [u8; 3],
    /// Foreground for the percentage text where it lands on an empty
    /// (dark gutter) cell. Light so the text is readable.
    pub text_on_empty: [u8; 3],
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

// Palette tuned for two things: (1) WCAG-grade contrast for the percentage
// text on every severity background by pairing light bgs with dark text;
// (2) hue alignment with the Anthropic brand palette where it doesn't
// fight legibility on a dark terminal.
pub const GRAPHITE: Theme = Theme {
    // Anthropic Green — olive, calm, "everything is fine".
    bg_ok: [120, 140, 93],
    // Warm amber — kept warm but slightly darkened from the original so
    // dark text on it lands well above WCAG AA.
    bg_warn: [216, 165, 96],
    // Coral red — darkened from the original vibrant tone to ease the
    // eye when a long-running session sits at 100%.
    bg_hot: [184, 80, 64],
    // Gutter — unchanged; dark enough to read light text on without
    // disappearing into the terminal background.
    bg_empty: [60, 65, 72],
    // Anthropic Dark — overlaid on the filled (light) severity cells.
    text_on_filled: [20, 20, 19],
    // Anthropic Light — overlaid on the gutter.
    text_on_empty: [250, 249, 245],
    // Subdued separators / brackets; kept darker than `ink` so the
    // structural punctuation recedes visually.
    mute: [120, 125, 132],
    // Warm off-white for segment labels (5h, 7d, dir).
    ink: [232, 230, 220],
    // Anthropic Blue — model name. Calmer than the prior vibrant mint
    // and distinct from the severity hues.
    model: [106, 155, 204],
    // Anthropic Orange — dirty-file count indicator; visually distinct
    // from `bg_warn`'s amber so it doesn't blend into the bar.
    warn: [217, 119, 87],
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
