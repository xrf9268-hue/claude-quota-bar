//! Minimal ANSI helpers. Respects `NO_COLOR` (https://no-color.org/).
//!
//! `color_enabled()` reads `NO_COLOR` on every call — sub-microsecond
//! lookup, lets tests toggle without per-process global state.

pub fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

pub fn fg(rgb: [u8; 3]) -> String {
    if !color_enabled() {
        return String::new();
    }
    format!("\x1b[38;2;{};{};{}m", rgb[0], rgb[1], rgb[2])
}

pub fn bg(rgb: [u8; 3]) -> String {
    if !color_enabled() {
        return String::new();
    }
    format!("\x1b[48;2;{};{};{}m", rgb[0], rgb[1], rgb[2])
}

pub fn reset() -> &'static str {
    if color_enabled() { "\x1b[0m" } else { "" }
}

pub fn bold() -> &'static str {
    if color_enabled() { "\x1b[1m" } else { "" }
}

/// Approximate visible width of a string with ANSI sequences stripped.
///
/// Counts each non-escape `char` as 1 column — sufficient for ASCII and
/// common BMP glyphs. Full Unicode width (CJK / emoji wide) would need
/// the `unicode-width` crate; not pulled in for one-cell-per-char output.
pub fn visible_width(s: &str) -> usize {
    strip_ansi(s).chars().count()
}

/// Strip ANSI CSI / SGR sequences from a string, keeping only printable
/// characters. Used by tests and width helpers.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_esc = false;
    for ch in s.chars() {
        if ch == '\x1b' {
            in_esc = true;
            continue;
        }
        if in_esc {
            if ch.is_ascii_alphabetic() {
                in_esc = false;
            }
            continue;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // Tests in this module mutate the process-wide NO_COLOR env var. We
    // serialize them via ENV_LOCK and recover from poison (a panic inside
    // a previous test poisons the mutex; we want surviving tests to still
    // run rather than cascade-fail).
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
    fn color_enabled_by_default() {
        with_no_color(false, || assert!(color_enabled()));
    }

    #[test]
    fn no_color_env_disables_color() {
        with_no_color(true, || assert!(!color_enabled()));
    }

    #[test]
    fn fg_emits_24bit_sequence_when_enabled() {
        with_no_color(false, || {
            assert_eq!(fg([10, 20, 30]), "\x1b[38;2;10;20;30m");
        });
    }

    #[test]
    fn bg_emits_24bit_sequence_when_enabled() {
        with_no_color(false, || {
            assert_eq!(bg([200, 100, 50]), "\x1b[48;2;200;100;50m");
        });
    }

    #[test]
    fn fg_empty_when_color_disabled() {
        with_no_color(true, || {
            assert_eq!(fg([10, 20, 30]), "");
        });
    }

    #[test]
    fn reset_empty_when_color_disabled() {
        with_no_color(true, || {
            assert_eq!(reset(), "");
        });
    }

    #[test]
    fn reset_returns_ansi_when_enabled() {
        with_no_color(false, || {
            assert_eq!(reset(), "\x1b[0m");
        });
    }

    #[test]
    fn bold_returns_ansi_when_enabled() {
        with_no_color(false, || {
            assert_eq!(bold(), "\x1b[1m");
        });
    }

    #[test]
    fn visible_width_counts_plain_chars() {
        assert_eq!(visible_width("hello"), 5);
    }

    #[test]
    fn visible_width_ignores_csi_sequences() {
        assert_eq!(visible_width("\x1b[38;2;1;2;3mhi\x1b[0m"), 2);
    }

    #[test]
    fn visible_width_ignores_multiple_sequences() {
        assert_eq!(visible_width("\x1b[1m[\x1b[0m42%\x1b[1m]\x1b[0m"), 5);
    }

    #[test]
    fn visible_width_handles_empty_string() {
        assert_eq!(visible_width(""), 0);
    }
}
