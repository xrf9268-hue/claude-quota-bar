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

/// Parse a UTC ISO-8601 timestamp (`2026-07-20T07:00:00.000Z`) to unix epoch
/// seconds. Claude Code's `model_scoped` windows carry `resets_at` as a
/// `Date.toISOString()` string rather than the epoch number the other
/// windows use. Only `Z` / `+00:00` offsets are accepted — a non-UTC offset
/// means we can't trust the value, and "unknown" beats a shifted countdown.
pub fn parse_iso8601_utc(s: &str) -> Option<u64> {
    let s = s.trim();
    let rest = s.strip_suffix('Z').or_else(|| s.strip_suffix("+00:00"))?;
    let (date, time) = rest.split_once('T')?;

    let mut dp = date.split('-');
    let y: i64 = dp.next()?.parse().ok()?;
    let m: u32 = dp.next()?.parse().ok()?;
    let d: u32 = dp.next()?.parse().ok()?;
    if dp.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }

    // Fractional seconds are truncated: a countdown never needs sub-second
    // precision.
    let time = time.split('.').next()?;
    let mut tp = time.split(':');
    let hh: u64 = tp.next()?.parse().ok()?;
    let mm: u64 = tp.next()?.parse().ok()?;
    let ss: u64 = tp.next()?.parse().ok()?;
    if tp.next().is_some() || hh > 23 || mm > 59 || ss > 59 {
        return None;
    }

    let days = days_from_civil(y, i64::from(m), i64::from(d));
    if days < 0 {
        return None; // pre-epoch dates can't be a quota reset
    }
    Some(days as u64 * 86400 + hh * 3600 + mm * 60 + ss)
}

/// Days between 1970-01-01 and the given civil date (Howard Hinnant's
/// `days_from_civil` algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
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
    fn iso8601_utc_parses_toisostring_output() {
        // JS Date.toISOString() shape — the model_scoped resets_at format.
        assert_eq!(parse_iso8601_utc("1970-01-01T00:00:00.000Z"), Some(0),);
        assert_eq!(
            parse_iso8601_utc("2026-07-20T07:00:00.000Z"),
            Some(1_784_530_800),
        );
        // No fractional seconds is also fine.
        assert_eq!(
            parse_iso8601_utc("2026-07-20T07:00:00Z"),
            Some(1_784_530_800),
        );
        // Explicit UTC offset spelling.
        assert_eq!(
            parse_iso8601_utc("2026-07-20T07:00:00+00:00"),
            Some(1_784_530_800),
        );
    }

    #[test]
    fn iso8601_utc_rejects_garbage() {
        assert_eq!(parse_iso8601_utc(""), None);
        assert_eq!(parse_iso8601_utc("not a date"), None);
        // Missing timezone designator — ambiguous, refuse.
        assert_eq!(parse_iso8601_utc("2026-07-20T07:00:00"), None);
        // Non-UTC offset — a shifted countdown is worse than unknown.
        assert_eq!(parse_iso8601_utc("2026-07-20T07:00:00+02:00"), None);
        // Out-of-range components.
        assert_eq!(parse_iso8601_utc("2026-13-20T07:00:00Z"), None);
        assert_eq!(parse_iso8601_utc("2026-07-20T25:00:00Z"), None);
        // Pre-epoch dates can't be a quota reset.
        assert_eq!(parse_iso8601_utc("1969-12-31T23:59:59Z"), None);
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
