//! Cross-session stdin cache.
//!
//! Anthropic only ships `rate_limits` when the user actually makes a
//! request. A freshly-opened terminal that hasn't sent any messages yet
//! receives stdin without rate-limit fields. We cache the last "good"
//! stdin so the bar can show a recent value instead of "--%".
//!
//! Stale-window guard: if the cached `resets_at` is in the past, the
//! window has rolled over and the cached `pct` is meaningless. The cache
//! reader signals "unknown" (None) so the renderer falls back to "--%".

use crate::input::{Input, RateLimits, Window};
use anyhow::Result;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const STALE_AFTER_SECS: u64 = 600;

pub fn cache_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".cache/claude-quota-bar/last_stdin.json")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Persist stdin to disk so the next render can recover rate_limits when
/// Claude Code ships partial payloads. The cache is global per-machine: it's
/// account-level quota, intentionally shared across this user's sessions so a
/// freshly-opened terminal isn't blank.
///
/// Two guards before writing:
/// - the payload must carry rate-limit data (else we'd clobber a useful
///   snapshot with an empty one);
/// - the payload must look like it came from Claude Code (`session_id`
///   present), so a hand-run invocation or unrelated script piping JSON
///   can't poison the bar every terminal reads.
pub fn maybe_save(input: &Input, raw: &str) -> Result<()> {
    let has_data = input
        .rate_limits
        .as_ref()
        .map(|r| r.five_hour.is_some() || r.seven_day.is_some())
        .unwrap_or(false);
    if !has_data || input.session_id.is_empty() {
        return Ok(());
    }
    let path = cache_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, raw)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// If `input` is missing rate_limits, try to restore them from the cache.
/// Honors window rollover: an expired `resets_at` is converted to "unknown".
pub fn fill_from_cache(input: &mut Input) {
    let needs_fill = input
        .rate_limits
        .as_ref()
        .map(|r| r.five_hour.is_none() && r.seven_day.is_none())
        .unwrap_or(true);
    if !needs_fill {
        return;
    }
    let path = cache_path();
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => return,
    };
    // Refuse the cache unless we can positively prove it's fresh. If the
    // mtime is unreadable, or `elapsed()` errors because the mtime is in the
    // future (clock skew / NTP step), we can't establish freshness — treat
    // that as stale rather than falling through and trusting a stale value.
    let fresh = meta
        .modified()
        .ok()
        .and_then(|m| m.elapsed().ok())
        .is_some_and(|e| e.as_secs() <= STALE_AFTER_SECS);
    if !fresh {
        return;
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return,
    };
    let cached: Input = match serde_json::from_str(&raw) {
        Ok(c) => c,
        Err(_) => return,
    };
    let now_unix = now();
    let mut limits = RateLimits::default();
    if let Some(rl) = cached.rate_limits {
        limits.five_hour = rollover(rl.five_hour, now_unix);
        limits.seven_day = rollover(rl.seven_day, now_unix);
    }
    if limits.five_hour.is_some() || limits.seven_day.is_some() {
        input.rate_limits = Some(limits);
    }
    // Deliberately do NOT hydrate context_window / transcript_path: the
    // cache is global per-machine, not per-session, so reusing those
    // fields across sessions renders stale token counts and points cache
    // age at the wrong transcript.
}

fn rollover(window: Option<Window>, now_unix: u64) -> Option<Window> {
    let w = window?;
    match w.resets_at {
        Some(resets) if resets > now_unix => Some(w),
        // Stale resets_at means the window has rolled. The cached pct is
        // meaningless — synthesizing "0% used" would be a lie. Return
        // None so the renderer shows --% instead.
        Some(_) => None,
        None => Some(w),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // Cache tests mutate the HOME env var. Serialize them so they don't
    // stomp on each other's tempdirs.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_home<F: FnOnce(&TempDir)>(f: F) {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = TempDir::new().unwrap();
        unsafe { std::env::set_var("HOME", home.path()) };
        f(&home);
        unsafe { std::env::remove_var("HOME") };
    }

    #[test]
    fn rollover_future_keeps_value() {
        let w = Window {
            used_percentage: 42.0,
            resets_at: Some(2000),
        };
        let r = rollover(Some(w), 1000).unwrap();
        assert_eq!(r.used_percentage, 42.0);
        assert_eq!(r.resets_at, Some(2000));
    }

    #[test]
    fn rollover_past_returns_none() {
        let w = Window {
            used_percentage: 42.0,
            resets_at: Some(100),
        };
        assert!(rollover(Some(w), 1000).is_none());
    }

    #[test]
    fn rollover_at_boundary_returns_none() {
        let w = Window {
            used_percentage: 42.0,
            resets_at: Some(1000),
        };
        assert!(rollover(Some(w), 1000).is_none());
    }

    #[test]
    fn rollover_no_resets_at_keeps_value() {
        let w = Window {
            used_percentage: 42.0,
            resets_at: None,
        };
        let r = rollover(Some(w), 1000).unwrap();
        assert_eq!(r.used_percentage, 42.0);
    }

    #[test]
    fn fill_from_cache_hydrates_rate_limits() {
        with_temp_home(|_| {
            let payload = format!(
                r#"{{"session_id":"s","rate_limits":{{"five_hour":{{"used_percentage":42,"resets_at":{}}}}}}}"#,
                now() + 600
            );
            let cached: Input = serde_json::from_str(&payload).unwrap();
            maybe_save(&cached, &payload).unwrap();

            let mut fresh: Input = serde_json::from_str("{}").unwrap();
            fill_from_cache(&mut fresh);

            let fh = fresh.rate_limits.unwrap().five_hour.unwrap();
            assert_eq!(fh.used_percentage, 42.0);
        });
    }

    #[test]
    fn fill_from_cache_does_not_hydrate_context_window() {
        with_temp_home(|_| {
            let payload = format!(
                r#"{{"session_id":"s","rate_limits":{{"five_hour":{{"used_percentage":42,"resets_at":{}}}}},"context_window":{{"context_window_size":999999}}}}"#,
                now() + 600
            );
            let cached: Input = serde_json::from_str(&payload).unwrap();
            maybe_save(&cached, &payload).unwrap();

            let mut fresh: Input = serde_json::from_str("{}").unwrap();
            fill_from_cache(&mut fresh);

            assert!(fresh.rate_limits.is_some());
            assert!(
                fresh.context_window.is_none(),
                "context_window must not be hydrated across sessions"
            );
        });
    }

    #[test]
    fn maybe_save_skips_when_no_rate_limits() {
        with_temp_home(|home| {
            let original = format!(
                r#"{{"session_id":"s","rate_limits":{{"five_hour":{{"used_percentage":42,"resets_at":{}}}}}}}"#,
                now() + 600
            );
            let original_input: Input = serde_json::from_str(&original).unwrap();
            maybe_save(&original_input, &original).unwrap();

            let empty = r#"{"session_id":"s"}"#;
            let empty_input: Input = serde_json::from_str(empty).unwrap();
            maybe_save(&empty_input, empty).unwrap();

            let path = home.path().join(".cache/claude-quota-bar/last_stdin.json");
            let actual = std::fs::read_to_string(&path).unwrap();
            assert!(
                actual.contains("42"),
                "cache was corrupted by empty payload: {actual:?}"
            );
        });
    }

    #[test]
    fn maybe_save_skips_without_session_id() {
        with_temp_home(|home| {
            // rate_limits present but no session_id → not Claude Code; must
            // not write, so a hand-run / unrelated script can't poison the
            // cache every terminal reads.
            let payload = format!(
                r#"{{"rate_limits":{{"five_hour":{{"used_percentage":77,"resets_at":{}}}}}}}"#,
                now() + 600
            );
            let input: Input = serde_json::from_str(&payload).unwrap();
            maybe_save(&input, &payload).unwrap();

            let path = home.path().join(".cache/claude-quota-bar/last_stdin.json");
            assert!(
                !path.exists(),
                "cache must not be written without a session_id"
            );
        });
    }
}
