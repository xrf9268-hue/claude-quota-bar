//! Battery-bar rendering — percentage text centered inside a colored bar,
//! filled cells use the severity background, empty cells use the gutter.
//! Text foreground flips per cell so digits stay readable on both the
//! light severity bg and the dark gutter.
//!
//! In NO_COLOR mode, fill state is encoded with Unicode block glyphs
//! (`█` full, `▄` half, `░` empty) so the bar still conveys progress
//! without escape codes.

use crate::ansi::{bg, color_enabled, fg, reset};
use crate::theme::Theme;

pub const DEFAULT_WIDTH: usize = 10;

const FILL_GLYPH: char = '█';
const HALF_GLYPH: char = '▄';
const EMPTY_GLYPH: char = '░';

pub fn battery_bar(pct: Option<f64>, theme: &Theme, width: usize) -> String {
    let text = match pct {
        Some(p) => format!("{}%", p.round() as i64),
        None => "--%".to_string(),
    };

    let text_len = text.chars().count();
    let pad_total = width.saturating_sub(text_len);
    let pad_left = pad_total / 2;
    let pad_right = pad_total - pad_left;
    let mut centered = String::with_capacity(width);
    centered.extend(std::iter::repeat_n(' ', pad_left));
    centered.push_str(&text);
    centered.extend(std::iter::repeat_n(' ', pad_right));

    // Fill cell count — clamp to [0, width]. Round half-up via +0.5.
    // For any positive pct that math-rounds to zero, force one cell so
    // small non-zero usage is visually distinct from "--%".
    let filled = match pct {
        Some(p) => {
            let clamped = p.clamp(0.0, 100.0);
            let raw = (clamped / 100.0) * width as f64;
            let rounded = (raw + 0.5).floor() as usize;
            if p > 0.0 && rounded == 0 {
                1
            } else {
                rounded.min(width)
            }
        }
        None => 0,
    };

    if !color_enabled() {
        // Double-resolution counter for NO_COLOR mode: each cell can be
        // full / half / empty. Lets a 10-wide bar feel like 20 cells.
        let filled_halves = match pct {
            Some(p) => {
                let clamped = p.clamp(0.0, 100.0);
                let raw_h = (clamped / 100.0) * (width * 2) as f64;
                let rounded = (raw_h + 0.5).floor() as usize;
                if p > 0.0 && rounded == 0 {
                    1
                } else {
                    rounded.min(width * 2)
                }
            }
            None => 0,
        };
        let full_cells = filled_halves / 2;
        let has_half = filled_halves % 2 == 1;
        return centered
            .chars()
            .enumerate()
            .map(|(i, ch)| {
                if ch != ' ' {
                    ch
                } else if i < full_cells {
                    FILL_GLYPH
                } else if i == full_cells && has_half {
                    HALF_GLYPH
                } else {
                    EMPTY_GLYPH
                }
            })
            .collect();
    }

    let bg_fill = match pct {
        Some(p) => theme.severity_bg(p),
        None => theme.bg_empty,
    };
    let bg_empty = theme.bg_empty;
    let fg_filled = fg(theme.text_on_filled);
    let fg_empty = fg(theme.text_on_empty);

    // Batch consecutive cells with the same background. The fill/empty
    // transition produces at most two color groups per bar, so the inner
    // text (e.g. "42%") stays contiguous in the output — simpler for
    // tests and ~10× less ANSI overhead per render than per-char escapes.
    // The fg color flips with the bg so the digits stay readable on either
    // side (WCAG contrast fails if one fg covers both light + dark bgs).
    let mut out = String::with_capacity(width + 64);
    let mut last_bg: Option<[u8; 3]> = None;
    for (i, ch) in centered.chars().enumerate() {
        let in_filled = i < filled;
        let current_bg = if in_filled { bg_fill } else { bg_empty };
        if last_bg != Some(current_bg) {
            out.push_str(&bg(current_bg));
            out.push_str(if in_filled { &fg_filled } else { &fg_empty });
            last_bg = Some(current_bg);
        }
        out.push(ch);
    }
    out.push_str(reset());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::{strip_ansi, visible_width};
    use crate::theme::GRAPHITE;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_no_color<F: FnOnce()>(set: bool, f: F) {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            if set {
                std::env::set_var("NO_COLOR", "1");
            } else {
                std::env::remove_var("NO_COLOR");
            }
        }
        f();
        unsafe { std::env::remove_var("NO_COLOR") };
    }

    #[test]
    fn unknown_pct_shows_dashes() {
        with_no_color(false, || {
            let out = strip_ansi(&battery_bar(None, &GRAPHITE, 10));
            assert!(out.contains("--%"), "expected --%, got {out:?}");
        });
    }

    #[test]
    fn known_pct_shows_percentage_text() {
        with_no_color(false, || {
            let out = strip_ansi(&battery_bar(Some(42.0), &GRAPHITE, 10));
            assert!(out.contains("42%"), "expected 42% in {out:?}");
        });
    }

    #[test]
    fn rounds_pct_to_nearest_integer() {
        with_no_color(false, || {
            let out = strip_ansi(&battery_bar(Some(35.7), &GRAPHITE, 10));
            assert!(out.contains("36%"));
        });
    }

    #[test]
    fn visible_width_matches_requested_width() {
        with_no_color(false, || {
            assert_eq!(visible_width(&battery_bar(Some(50.0), &GRAPHITE, 10)), 10);
            assert_eq!(visible_width(&battery_bar(Some(50.0), &GRAPHITE, 14)), 14);
        });
    }

    #[test]
    fn visible_width_in_no_color_mode() {
        with_no_color(true, || {
            assert_eq!(visible_width(&battery_bar(Some(42.0), &GRAPHITE, 10)), 10);
        });
    }

    #[test]
    fn color_mode_emits_ansi_codes() {
        with_no_color(false, || {
            let out = battery_bar(Some(50.0), &GRAPHITE, 10);
            assert!(out.contains("\x1b["), "expected ANSI codes in {out:?}");
        });
    }

    #[test]
    fn no_color_mode_has_no_ansi() {
        with_no_color(true, || {
            let out = battery_bar(Some(50.0), &GRAPHITE, 10);
            assert!(!out.contains("\x1b"), "unexpected ANSI in {out:?}");
        });
    }

    #[test]
    fn no_color_mode_no_half_glyph_at_exact_decile() {
        with_no_color(true, || {
            // 50% with width=10 → exactly 5 cells. No half-glyph expected.
            let out = battery_bar(Some(50.0), &GRAPHITE, 10);
            assert!(!out.contains('▄'), "unexpected half-glyph at 50%: {out:?}");
        });
    }

    #[test]
    fn no_color_mode_uses_half_glyph_at_quarter_intervals() {
        with_no_color(true, || {
            // 25% with width=10 → 2.5 cells filled = 2 full + 1 half.
            let out = battery_bar(Some(25.0), &GRAPHITE, 10);
            let full = out.chars().filter(|&c| c == '█').count();
            let half = out.chars().filter(|&c| c == '▄').count();
            assert_eq!(full, 2, "expected 2 full cells in {out:?}");
            assert_eq!(half, 1, "expected 1 half cell in {out:?}");
        });
    }

    #[test]
    fn no_color_mode_uses_fill_glyphs() {
        with_no_color(true, || {
            let out = battery_bar(Some(50.0), &GRAPHITE, 10);
            assert!(out.contains('█'), "no FILL glyph in {out:?}");
            assert!(out.contains('░'), "no EMPTY glyph in {out:?}");
        });
    }

    #[test]
    fn pct_above_100_does_not_panic() {
        with_no_color(false, || {
            let out = battery_bar(Some(150.0), &GRAPHITE, 10);
            assert_eq!(visible_width(&out), 10);
        });
    }

    #[test]
    fn negative_pct_does_not_panic() {
        with_no_color(false, || {
            let out = battery_bar(Some(-5.0), &GRAPHITE, 10);
            assert_eq!(visible_width(&out), 10);
        });
    }

    #[test]
    fn small_nonzero_pct_lights_at_least_one_cell() {
        with_no_color(true, || {
            let out = battery_bar(Some(1.0), &GRAPHITE, 10);
            let lit = out.chars().filter(|&c| c == '█' || c == '▄').count();
            assert!(lit >= 1, "expected at least one lit cell in {out:?}");
        });
    }

    #[test]
    fn partial_bar_uses_dark_text_on_filled_and_light_text_on_empty() {
        // The percentage text spans cells with both fill states, so the
        // bar must switch fg color per cell to stay readable on either
        // background. One color for both sides yields a WCAG-failing
        // contrast on at least one of them.
        with_no_color(false, || {
            let out = battery_bar(Some(50.0), &GRAPHITE, 10);
            let [fr, fg_, fb] = GRAPHITE.text_on_filled;
            let [er, eg, eb] = GRAPHITE.text_on_empty;
            let filled_fg = format!("\x1b[38;2;{fr};{fg_};{fb}m");
            let empty_fg = format!("\x1b[38;2;{er};{eg};{eb}m");
            assert!(
                out.contains(&filled_fg),
                "missing dark fg on filled cells in {out:?}"
            );
            assert!(
                out.contains(&empty_fg),
                "missing light fg on empty cells in {out:?}"
            );
        });
    }

    #[test]
    fn hot_threshold_uses_hot_background() {
        with_no_color(false, || {
            let out = battery_bar(Some(85.0), &GRAPHITE, 10);
            let [r, g, b] = GRAPHITE.bg_hot;
            let needle = format!("\x1b[48;2;{r};{g};{b}m");
            assert!(out.contains(&needle), "hot bg missing in {out:?}");
        });
    }

    #[test]
    fn ok_threshold_uses_ok_background() {
        with_no_color(false, || {
            let out = battery_bar(Some(10.0), &GRAPHITE, 10);
            let [r, g, b] = GRAPHITE.bg_ok;
            let needle = format!("\x1b[48;2;{r};{g};{b}m");
            assert!(out.contains(&needle), "ok bg missing in {out:?}");
        });
    }

    #[test]
    fn none_pct_no_color_shows_only_empty_glyphs() {
        with_no_color(true, || {
            let out = battery_bar(None, &GRAPHITE, 10);
            assert_eq!(visible_width(&out), 10);
            assert!(
                !out.contains('█'),
                "unknown state should not show fill in {out:?}"
            );
        });
    }
}
