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

/// Parse a fixed-shape UTC ISO-8601 timestamp (`YYYY-MM-DDTHH:MM:SS[.fff]Z`)
/// to Unix seconds. Returns `None` on any structural deviation.
///
/// Claude Code writes exactly this shape into transcript lines, so a full
/// RFC-3339 parser (and a dependency) would be overkill. The date→days math
/// is Howard Hinnant's `days_from_civil`, valid across the Gregorian range.
pub fn parse_iso8601_to_unix(s: &str) -> Option<u64> {
    // Operate on bytes, not chars: a malformed timestamp with a multibyte
    // char before a fixed offset (e.g. `202é-05-...`) would make `&s[4..5]`
    // slice mid-char and panic, aborting the statusline. Byte indexing can't
    // hit a char boundary, and the digit check below rejects any non-ASCII.
    let b = s.as_bytes();
    // Minimum "YYYY-MM-DDTHH:MM:SS" is 19 ASCII bytes.
    if b.len() < 19 {
        return None;
    }
    // Separators must sit exactly where the fixed format puts them.
    if b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    // Parse a fixed-width run of ASCII digits; any non-digit byte (including
    // the high bytes of a multibyte char) makes the whole timestamp invalid.
    let field = |start: usize, len: usize| -> Option<i64> {
        let mut v: i64 = 0;
        for &c in &b[start..start + len] {
            if !c.is_ascii_digit() {
                return None;
            }
            v = v * 10 + i64::from(c - b'0');
        }
        Some(v)
    };
    let year = field(0, 4)?;
    let month = field(5, 2)?;
    let day = field(8, 2)?;
    let hour = field(11, 2)?;
    let min = field(14, 2)?;
    let sec = field(17, 2)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }

    // days_from_civil: days since 1970-01-01 for a proleptic Gregorian date.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146097 + doe - 719468;

    let total = days * 86400 + hour * 3600 + min * 60 + sec;
    u64::try_from(total).ok()
}

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

/// Format remaining prompt-cache TTL.
///
/// `age` is seconds since the last assistant message in the transcript.
/// `ttl` is Anthropic's prompt-cache lifetime (default 300s, 3600s if the
/// user has enabled the 1h extended-cache feature flag).
///
/// "COLD" means the cache has expired — the next API call will pay full
/// input-token price.
pub fn cache_remaining(age: Option<f64>, ttl: f64) -> String {
    let age = match age {
        None => return "COLD".to_string(),
        Some(a) => a.max(0.0),
    };
    let remaining = ttl - age;
    if remaining <= 0.0 {
        return "COLD".to_string();
    }
    let remaining_int = remaining.ceil() as u64;
    if remaining_int >= 3600 {
        let h = remaining_int / 3600;
        let m = (remaining_int % 3600) / 60;
        if m == 0 {
            format!("{h}h")
        } else {
            format!("{h}h{m:02}m")
        }
    } else if remaining_int >= 300 {
        format!("{}m", remaining_int / 60)
    } else if remaining_int >= 60 {
        let m = remaining_int / 60;
        let s = remaining_int % 60;
        format!("{m}m{s:02}s")
    } else {
        format!("{remaining_int}s")
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
    fn iso8601_parses_canonical_utc() {
        assert_eq!(
            parse_iso8601_to_unix("2026-05-26T04:27:11.826Z"),
            Some(1_779_769_631)
        );
        // Fractional seconds and the trailing Z are optional tail bytes.
        assert_eq!(
            parse_iso8601_to_unix("2026-05-26T04:27:11Z"),
            Some(1_779_769_631)
        );
        // Unix epoch.
        assert_eq!(parse_iso8601_to_unix("1970-01-01T00:00:00.000Z"), Some(0));
    }

    #[test]
    fn iso8601_rejects_malformed() {
        assert_eq!(parse_iso8601_to_unix(""), None);
        assert_eq!(parse_iso8601_to_unix("not-a-timestamp"), None);
        assert_eq!(parse_iso8601_to_unix("2026/05/26T04:27:11Z"), None); // wrong separators
        assert_eq!(parse_iso8601_to_unix("2026-13-26T04:27:11Z"), None); // bad month
        assert_eq!(parse_iso8601_to_unix("2026-05-26T25:27:11Z"), None); // bad hour
    }

    #[test]
    fn iso8601_multibyte_char_does_not_panic() {
        // Codex P2: a multibyte char before a fixed offset must be rejected,
        // not panic on a mid-char byte slice. `é` is two bytes, so a naive
        // `&s[4..5]` would split it and abort the statusline.
        assert_eq!(parse_iso8601_to_unix("202é-05-26T04:27:11Z"), None);
        assert_eq!(
            parse_iso8601_to_unix("2026-05-26T04:27:11é"),
            Some(1_779_769_631)
        );
        assert_eq!(
            parse_iso8601_to_unix("日本語日本語日本語日本語日本語日本語日"),
            None
        );
    }

    #[test]
    fn cache_levels() {
        assert_eq!(cache_remaining(None, 300.0), "COLD");
        assert_eq!(cache_remaining(Some(301.0), 300.0), "COLD");
        assert_eq!(cache_remaining(Some(0.0), 300.0), "5m");
        assert_eq!(cache_remaining(Some(240.0), 300.0), "1m00s");
        assert_eq!(cache_remaining(Some(290.0), 300.0), "10s");
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
