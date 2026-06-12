//! Time / countdown formatting. Pure functions, no clock reads except the
//! `now_unix()` helper which is the only impurity.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Upper bound on a believable `resets_at` countdown. The widest quota
/// window is 7 days; anything past 90 days is treated as a bad value.
const MAX_REASONABLE_RESET_SECS: u64 = 90 * 86400;

/// Format remaining time until a unix timestamp.
///
/// Granularity ladders down as time gets shorter so the user sees the most
/// useful unit at every scale:
/// - `>= 1d`  → `8d3h`
/// - `>= 1h`  → `2h45m`
/// - `>= 1m`  → `26m`
/// - `< 1m`   → `45s`
pub fn countdown(now: u64, resets_at: u64) -> String {
    if resets_at <= now {
        return "--".to_string();
    }
    let secs = resets_at - now;
    // Sanity cap: the longest legitimate window (7d) resets within ~7 days.
    // A value beyond ~90 days is a bogus / sentinel `resets_at` — rendering
    // "95141d14h" is worse than admitting "unknown".
    if secs > MAX_REASONABLE_RESET_SECS {
        return "--".to_string();
    }
    fmt_elapsed(secs)
}

/// Format an elapsed duration with the same granularity ladder as
/// `countdown`, but with no "expired" or sanity-cap states — an elapsed
/// value is always a fact, never a prediction.
pub fn fmt_elapsed(secs: u64) -> String {
    if secs >= 86400 {
        let days = secs / 86400;
        let hours = (secs % 86400) / 3600;
        if hours == 0 {
            format!("{days}d")
        } else {
            format!("{days}d{hours}h")
        }
    } else if secs >= 3600 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        if mins == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h{mins:02}m")
        }
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

/// Compress a token count to a short label (`1234` → `1.2k`, `1_500_000` → `1.5M`).
pub fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_levels() {
        assert_eq!(countdown(0, 0), "--");
        assert_eq!(countdown(100, 50), "--");
        assert_eq!(countdown(0, 45), "45s");
        assert_eq!(countdown(0, 60), "1m");
        assert_eq!(countdown(0, 1560), "26m");
        assert_eq!(countdown(0, 3600), "1h");
        assert_eq!(countdown(0, 3600 + 45 * 60), "1h45m");
        assert_eq!(countdown(0, 86400), "1d");
        assert_eq!(countdown(0, 8 * 86400 + 3 * 3600), "8d3h");
    }

    #[test]
    fn countdown_caps_absurd_resets_at() {
        // A bogus far-future resets_at must not render "95141d14h".
        assert_eq!(countdown(0, 9_999_999_999), "--");
        // Just past the 90-day cap → unknown.
        assert_eq!(countdown(0, 90 * 86400 + 1), "--");
        // At the cap boundary → still rendered.
        assert_eq!(countdown(0, 90 * 86400), "90d");
    }

    #[test]
    fn elapsed_levels() {
        assert_eq!(fmt_elapsed(0), "0s");
        assert_eq!(fmt_elapsed(45), "45s");
        assert_eq!(fmt_elapsed(60), "1m");
        assert_eq!(fmt_elapsed(1560), "26m");
        assert_eq!(fmt_elapsed(3600), "1h");
        assert_eq!(fmt_elapsed(3600 + 45 * 60), "1h45m");
        assert_eq!(fmt_elapsed(86400), "1d");
        assert_eq!(fmt_elapsed(8 * 86400 + 3 * 3600), "8d3h");
    }

    #[test]
    fn tokens() {
        assert_eq!(fmt_tokens(500), "500");
        assert_eq!(fmt_tokens(1_500), "1.5k");
        assert_eq!(fmt_tokens(71_000), "71.0k");
        assert_eq!(fmt_tokens(200_000), "200.0k");
        assert_eq!(fmt_tokens(1_500_000), "1.5M");
    }
}
