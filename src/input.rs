//! Parse Claude Code stdin JSON. Only the fields the renderer consumes are
//! declared; serde ignores the rest of the payload.
//!
//! Every field is optional via `#[serde(default)]` because Claude Code
//! ships a partial payload during the first few renders of a fresh session
//! (no `rate_limits`). The renderer falls back to placeholders.

use serde::Deserialize;

#[derive(Debug, Default, Deserialize, Clone)]
pub struct Input {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub model: Model,
    #[serde(default)]
    pub workspace: Workspace,
    #[serde(default)]
    pub context_window: Option<ContextWindow>,
    #[serde(default)]
    pub rate_limits: Option<RateLimits>,
    #[serde(default)]
    pub cost: Option<Cost>,
}

/// Session cost/usage counters. Only `total_duration_ms` is consumed — it is
/// Claude Code's wall-clock time since the session started (`Date.now() -
/// processStart`), rendered directly by the `session` segment. It grows while
/// idle and resets to zero on `--resume`/`--continue` (a resumed session is a
/// new process); both are acceptable for a "session elapsed" readout.
///
/// We deliberately no longer keep our own active-time ledger. The previous
/// approach integrated wall-clock between statusline renders, but renders are
/// sparse and event-driven — Claude Code's docs note the triggers "go quiet
/// when the main session is idle" (e.g. during long autonomous turns) — so
/// that integral systematically under-counted by 40-90% in real sessions.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct Cost {
    #[serde(default)]
    pub total_duration_ms: u64,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct Model {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub display_name: String,
}

impl Model {
    pub fn name(&self) -> &str {
        if !self.display_name.is_empty() {
            &self.display_name
        } else if !self.id.is_empty() {
            &self.id
        } else {
            "Claude"
        }
    }
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct Workspace {
    #[serde(default)]
    pub current_dir: String,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct ContextWindow {
    #[serde(default)]
    pub context_window_size: u64,
    /// Tokens currently in the context window: `input + cache_creation +
    /// cache_read` of the most recent API response (Claude Code >= 2.1.132;
    /// older versions shipped a cumulative session total here).
    #[serde(default)]
    pub total_input_tokens: u64,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct RateLimits {
    #[serde(default)]
    pub five_hour: Option<Window>,
    #[serde(default)]
    pub seven_day: Option<Window>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct Window {
    #[serde(default)]
    pub used_percentage: f64,
    #[serde(default)]
    pub resets_at: Option<u64>,
}

/// Parse a Claude Code stdin payload. This is the production parse path —
/// `main` feeds it raw stdin and falls back to `Input::default()` on error,
/// so it must keep rejecting invalid JSON rather than parsing leniently.
pub fn parse(s: &str) -> serde_json::Result<Input> {
    serde_json::from_str(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_name_prefers_display_name() {
        let m = Model {
            id: "claude-opus-4-7".into(),
            display_name: "Opus 4.7".into(),
        };
        assert_eq!(m.name(), "Opus 4.7");
    }

    #[test]
    fn model_name_falls_back_to_id() {
        let m = Model {
            id: "claude-opus-4-7".into(),
            display_name: String::new(),
        };
        assert_eq!(m.name(), "claude-opus-4-7");
    }

    #[test]
    fn model_name_falls_back_to_claude() {
        let m = Model::default();
        assert_eq!(m.name(), "Claude");
    }

    #[test]
    fn parse_full_payload() {
        let json = r#"{
            "model": {"id": "claude-opus-4-7", "display_name": "Opus 4.7"},
            "workspace": {"current_dir": "/foo"},
            "rate_limits": {
                "five_hour": {"used_percentage": 42, "resets_at": 1700000000},
                "seven_day": {"used_percentage": 35}
            },
            "context_window": {
                "used_percentage": 36.5,
                "context_window_size": 200000,
                "total_input_tokens": 65000,
                "total_output_tokens": 6000
            },
            "cost": {"total_cost_usd": 0.5}
        }"#;
        let input = parse(json).unwrap();
        assert_eq!(input.model.name(), "Opus 4.7");
        assert_eq!(input.workspace.current_dir, "/foo");
        let rl = input.rate_limits.unwrap();
        assert_eq!(rl.five_hour.unwrap().used_percentage, 42.0);
        assert_eq!(rl.seven_day.unwrap().used_percentage, 35.0);
        let ctx = input.context_window.unwrap();
        assert_eq!(ctx.context_window_size, 200000);
        assert_eq!(ctx.total_input_tokens, 65000);
    }

    #[test]
    fn parse_empty_object_uses_defaults() {
        let input = parse("{}").unwrap();
        assert_eq!(input.model.name(), "Claude");
        assert_eq!(input.workspace.current_dir, "");
        assert!(input.rate_limits.is_none());
        assert!(input.context_window.is_none());
    }

    #[test]
    fn parse_missing_resets_at_returns_none_field() {
        let json = r#"{"rate_limits": {"five_hour": {"used_percentage": 42}}}"#;
        let input = parse(json).unwrap();
        let fh = input.rate_limits.unwrap().five_hour.unwrap();
        assert_eq!(fh.used_percentage, 42.0);
        assert!(fh.resets_at.is_none());
    }

    #[test]
    fn parse_cost_duration() {
        let json = r#"{"cost": {"total_cost_usd": 0.5, "total_duration_ms": 145000}}"#;
        let input = parse(json).unwrap();
        assert_eq!(input.cost.unwrap().total_duration_ms, 145000);
    }

    #[test]
    fn parse_missing_cost_is_none() {
        let input = parse("{}").unwrap();
        assert!(input.cost.is_none());
    }

    #[test]
    fn parse_invalid_json_errors() {
        assert!(parse("not json").is_err());
    }

    #[test]
    fn parse_unknown_fields_are_ignored() {
        let json = r#"{"future_field": 42, "model": {"id": "x"}}"#;
        let input = parse(json).unwrap();
        assert_eq!(input.model.id, "x");
    }
}
