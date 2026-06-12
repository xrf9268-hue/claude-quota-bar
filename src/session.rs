//! Per-session active-time accounting.
//!
//! Claude Code's stdin durations can't answer "how long have I been
//! working this session": `total_duration_ms` is `Date.now() - processStart`
//! (grows while idle, resets on resume) and `total_api_duration_ms` only
//! counts API wait. So we keep our own ledger per `session_id` — which is
//! stable across `--resume`/`--continue` — treating each statusline render
//! as a heartbeat: gaps short enough to be think-time accrue, longer gaps
//! count as interruptions and are dropped.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A render-to-render gap longer than this is an interruption (user left,
/// laptop slept), not think-time. 15 minutes is the same keystroke-timeout
/// convention WakaTime uses for "active coding time", and it's long enough
/// to ride out a big build/test tool call that emits no renders.
pub const IDLE_GAP_SECS: u64 = 15 * 60;

/// Session ledgers older than this are dead sessions; sweep them when a new
/// session appears. Matches the widest quota window (7d) so nothing a user
/// could still meaningfully resume gets deleted out from under them.
const CLEANUP_AFTER_SECS: u64 = 7 * 86400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub active_secs: u64,
    pub last_seen_unix: u64,
    pub last_api_ms: u64,
}

/// Fold one statusline render into the ledger.
///
/// A gap accrues only when it's short enough to be think-time AND the
/// payload shows Claude made progress since the last render (`api_ms`
/// changed — `!=` not `>`, because a resumed session restarts Claude Code's
/// in-process counter from zero). The progress check is what keeps
/// `statusLine.refreshInterval` users from accruing idle wall-clock: their
/// statusline re-renders every N seconds even when nothing is happening.
/// Without a cost object there is no progress signal, so the gap rule
/// stands alone rather than freezing the counter.
pub fn advance(prev: Option<State>, now_unix: u64, api_ms: Option<u64>) -> State {
    let Some(prev) = prev else {
        return State {
            active_secs: 0,
            last_seen_unix: now_unix,
            last_api_ms: api_ms.unwrap_or(0),
        };
    };
    let delta = now_unix.saturating_sub(prev.last_seen_unix);
    let progressed = api_ms.is_none_or(|a| a != prev.last_api_ms);
    let credit = if delta <= IDLE_GAP_SECS && progressed {
        delta
    } else {
        0
    };
    State {
        // Saturating: the ledger lives in the user's HOME and can be
        // hand-edited to u64::MAX; debug builds panic on overflow and the
        // statusline must never crash the parent.
        active_secs: prev.active_secs.saturating_add(credit),
        last_seen_unix: now_unix,
        last_api_ms: api_ms.unwrap_or(prev.last_api_ms),
    }
}

/// Load → advance → persist the ledger for one render. Returns `None` when
/// the session_id can't name a state file (empty, or containing anything
/// beyond the UUID alphabet — stdin is outside our trust boundary and the
/// id becomes a filename). All I/O errors degrade to "count from zero";
/// the statusline must never crash the parent.
pub fn update(session_id: &str, now_unix: u64, api_ms: Option<u64>) -> Option<State> {
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let dir = sessions_dir();
    let path = dir.join(format!("{session_id}.json"));
    let prev = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok());
    if !path.exists() {
        // New session on this machine: a natural, once-per-session moment
        // to sweep dead ledgers instead of paying a directory scan on
        // every render. Keyed on file existence, not parse success — a
        // corrupt ledger restarts its count but must not pay (or inflict)
        // a sweep on every render until persist repairs it.
        cleanup(&dir);
    }
    let next = advance(prev, now_unix, api_ms);
    let _ = persist(&path, &next);
    Some(next)
}

fn sessions_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".cache/claude-quota-bar/sessions")
}

fn persist(path: &std::path::Path, state: &State) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string(state).map_err(std::io::Error::other)?;
    // PID-unique tmp name: concurrent renders of the same session would
    // otherwise interleave write/rename on a shared tmp file and silently
    // drop one heartbeat. Orphaned tmps (killed process) age out via
    // cleanup, which sweeps by mtime regardless of extension.
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, raw)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn cleanup(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|e| e.as_secs() > CLEANUP_AFTER_SECS);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::with_temp_home;

    fn state(active_secs: u64, last_seen_unix: u64, last_api_ms: u64) -> State {
        State {
            active_secs,
            last_seen_unix,
            last_api_ms,
        }
    }

    // --- advance: the pure accrual rule ---

    #[test]
    fn first_render_starts_at_zero() {
        let s = advance(None, 1000, Some(500));
        assert_eq!(s.active_secs, 0);
        assert_eq!(s.last_seen_unix, 1000);
        assert_eq!(s.last_api_ms, 500);
    }

    #[test]
    fn short_gap_with_api_progress_accrues() {
        let s = advance(Some(state(10, 1000, 500)), 1060, Some(700));
        assert_eq!(s.active_secs, 70);
        assert_eq!(s.last_seen_unix, 1060);
        assert_eq!(s.last_api_ms, 700);
    }

    #[test]
    fn gap_at_threshold_still_accrues() {
        let s = advance(Some(state(0, 1000, 1)), 1000 + IDLE_GAP_SECS, Some(2));
        assert_eq!(s.active_secs, IDLE_GAP_SECS);
    }

    #[test]
    fn long_gap_is_an_interruption() {
        // User walked away for an hour: nothing accrues, but the heartbeat
        // moves forward so the next short gap counts again.
        let s = advance(Some(state(300, 1000, 500)), 1000 + 3600, Some(700));
        assert_eq!(s.active_secs, 300);
        assert_eq!(s.last_seen_unix, 1000 + 3600);
    }

    #[test]
    fn idle_refresh_without_api_progress_does_not_accrue() {
        // statusLine.refreshInterval re-runs the script every N seconds even
        // when idle. Same api_ms as last render = no progress = no credit.
        let s = advance(Some(state(300, 1000, 500)), 1005, Some(500));
        assert_eq!(s.active_secs, 300);
        assert_eq!(s.last_seen_unix, 1005);
    }

    #[test]
    fn missing_cost_falls_back_to_gap_only() {
        // No cost object in stdin → no progress signal → trust the gap rule
        // alone rather than freezing the counter.
        let s = advance(Some(state(300, 1000, 500)), 1060, None);
        assert_eq!(s.active_secs, 360);
        assert_eq!(s.last_api_ms, 500, "missing api_ms must not clobber");
    }

    #[test]
    fn api_reset_after_resume_still_counts() {
        // A resumed session restarts Claude Code's in-process API counter
        // from zero, so api_ms goes *down*. That's still progress.
        let s = advance(Some(state(300, 1000, 900_000)), 1060, Some(1500));
        assert_eq!(s.active_secs, 360);
        assert_eq!(s.last_api_ms, 1500);
    }

    #[test]
    fn clock_skew_backwards_does_not_panic_or_accrue() {
        let s = advance(Some(state(300, 1000, 500)), 900, Some(700));
        assert_eq!(s.active_secs, 300);
        assert_eq!(s.last_seen_unix, 900);
    }

    #[test]
    fn absurd_persisted_active_secs_does_not_panic() {
        // The ledger lives in the user's HOME and can be hand-edited to
        // u64::MAX. Debug builds panic on overflow — and the statusline
        // must never crash the parent.
        let s = advance(Some(state(u64::MAX, 1000, 1)), 1060, Some(2));
        assert_eq!(s.active_secs, u64::MAX);
    }

    // --- update: load → advance → save round trip ---

    #[test]
    fn update_accumulates_across_invocations() {
        with_temp_home(|_| {
            let first = update("sess-1", 1000, Some(100)).unwrap();
            assert_eq!(first.active_secs, 0);
            let second = update("sess-1", 1060, Some(200)).unwrap();
            assert_eq!(second.active_secs, 60);
        });
    }

    #[test]
    fn sessions_do_not_share_state() {
        with_temp_home(|_| {
            update("sess-a", 1000, Some(100));
            update("sess-a", 1060, Some(200));
            let other = update("sess-b", 1120, Some(300)).unwrap();
            assert_eq!(other.active_secs, 0);
        });
    }

    #[test]
    fn corrupt_state_file_restarts_from_zero() {
        with_temp_home(|home| {
            update("sess-1", 1000, Some(100));
            let path = home
                .path()
                .join(".cache/claude-quota-bar/sessions/sess-1.json");
            std::fs::write(&path, "not json").unwrap();
            let s = update("sess-1", 1060, Some(200)).unwrap();
            assert_eq!(s.active_secs, 0);
        });
    }

    #[test]
    fn empty_session_id_returns_none() {
        with_temp_home(|_| {
            assert!(update("", 1000, Some(100)).is_none());
        });
    }

    #[test]
    fn session_id_with_path_characters_is_rejected() {
        with_temp_home(|home| {
            assert!(update("../../evil", 1000, Some(100)).is_none());
            assert!(
                !home.path().join("evil.json").exists(),
                "must not write outside the sessions dir"
            );
        });
    }

    #[test]
    fn stale_session_files_are_cleaned_on_new_session() {
        with_temp_home(|home| {
            update("old-sess", 1000, Some(100));
            let dir = home.path().join(".cache/claude-quota-bar/sessions");
            let old = dir.join("old-sess.json");
            let ancient = std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 86400);
            std::fs::File::options()
                .write(true)
                .open(&old)
                .unwrap()
                .set_modified(ancient)
                .unwrap();

            update("new-sess", 2000, Some(100));
            assert!(!old.exists(), "8-day-old session file must be removed");
            assert!(dir.join("new-sess.json").exists());
        });
    }

    #[test]
    fn corrupt_state_file_does_not_trigger_cleanup() {
        // Cleanup is "once per new session", keyed on the file not existing.
        // A corrupt-but-present ledger must not pay the directory scan (and
        // must not sweep other sessions' files as a side effect).
        with_temp_home(|home| {
            update("sess-1", 1000, Some(100));
            update("other-sess", 1500, Some(100));
            let dir = home.path().join(".cache/claude-quota-bar/sessions");
            let other = dir.join("other-sess.json");
            let ancient = std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 86400);
            std::fs::File::options()
                .write(true)
                .open(&other)
                .unwrap()
                .set_modified(ancient)
                .unwrap();

            std::fs::write(dir.join("sess-1.json"), "not json").unwrap();
            update("sess-1", 2060, Some(200));
            assert!(
                other.exists(),
                "corrupt existing ledger must not trigger a sweep"
            );
        });
    }

    #[test]
    fn fresh_session_files_survive_cleanup() {
        with_temp_home(|home| {
            update("recent-sess", 1000, Some(100));
            update("new-sess", 2000, Some(100));
            let dir = home.path().join(".cache/claude-quota-bar/sessions");
            assert!(dir.join("recent-sess.json").exists());
        });
    }
}
