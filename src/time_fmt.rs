//! Time / countdown formatting. Pure functions, no clock reads except the
//! `now_unix()` helper which is the only impurity.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
