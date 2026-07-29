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
    /// Context occupancy as a percentage (0-100), computed by Claude Code
    /// itself on recent versions (observed on 2.1.220). Preferred over
    /// deriving from the token fields; `None` on older versions.
    #[serde(default)]
    pub used_percentage: Option<f64>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct RateLimits {
    #[serde(default)]
    pub five_hour: Option<Window>,
    #[serde(default)]
    pub seven_day: Option<Window>,
    /// Per-model weekly buckets from the server `limits[]` array (e.g. the
    /// Fable allowance on Max/Team Premium plans). Claude Code labels each
    /// bucket with a server-supplied `display_name` ("Fable", "Fable 5").
    /// Parsed leniently: a malformed entry is dropped instead of failing the
    /// whole payload, because this field's wire shape is still settling.
    #[serde(default, deserialize_with = "lenient_model_scoped")]
    pub model_scoped: Vec<ModelScopedWindow>,
}

impl RateLimits {
    /// The Fable usage bucket, if the server shipped one. Matches on the
    /// server-supplied label ("Fable", "Fable 5", ...) rather than a fixed
    /// key — that's how Claude Code itself identifies the bucket.
    pub fn fable(&self) -> Option<&ModelScopedWindow> {
        self.model_scoped.iter().find(|w| {
            w.display_name
                .trim()
                .to_ascii_lowercase()
                .starts_with("fable")
        })
    }
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct Window {
    #[serde(default)]
    pub used_percentage: f64,
    #[serde(default)]
    pub resets_at: Option<u64>,
}

/// One per-model quota bucket. Normalized at parse time so the rest of the
/// code never sees the wire variance: `used_percentage` (0-100, the shape the
/// statusline uses for the other windows) or `utilization` (0-1, the shape of
/// Claude Code's internal snapshot), and `resets_at` as epoch seconds or an
/// ISO-8601 string.
#[derive(Debug, Default, Clone)]
pub struct ModelScopedWindow {
    pub display_name: String,
    /// Percentage of the bucket used (0-100), or None when the server sent
    /// null / nothing usable.
    pub used_percentage: Option<f64>,
    /// Unix epoch seconds when the bucket resets, or None when absent or
    /// unparseable.
    pub resets_at: Option<u64>,
}

fn lenient_model_scoped<'de, D>(d: D) -> Result<Vec<ModelScopedWindow>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    let Some(arr) = v.as_array() else {
        return Ok(Vec::new());
    };
    Ok(arr.iter().filter_map(model_scoped_entry).collect())
}

fn model_scoped_entry(v: &serde_json::Value) -> Option<ModelScopedWindow> {
    let obj = v.as_object()?;
    let display_name = obj.get("display_name")?.as_str()?.to_string();
    // `used_percentage` (0-100) wins if both are present — it's the already-
    // normalized statusline shape; `utilization` (0-1) is the raw fraction.
    let used_percentage = obj
        .get("used_percentage")
        .and_then(serde_json::Value::as_f64)
        .or_else(|| {
            obj.get("utilization")
                .and_then(serde_json::Value::as_f64)
                .map(|u| u * 100.0)
        })
        .filter(|p| p.is_finite());
    let resets_at = obj.get("resets_at").and_then(|r| {
        r.as_u64()
            .or_else(|| r.as_str().and_then(crate::time_fmt::parse_iso8601_utc))
    });
    Some(ModelScopedWindow {
        display_name,
        used_percentage,
        resets_at,
    })
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
        assert_eq!(ctx.used_percentage, Some(36.5));
    }

    #[test]
    fn parse_context_window_without_used_percentage() {
        // Older Claude Code versions don't ship the field; `null` must
        // also read as unknown rather than failing the payload.
        let json = r#"{"context_window": {"context_window_size": 200000}}"#;
        let cw = parse(json).unwrap().context_window.unwrap();
        assert_eq!(cw.used_percentage, None);

        let json =
            r#"{"context_window": {"context_window_size": 200000, "used_percentage": null}}"#;
        let cw = parse(json).unwrap().context_window.unwrap();
        assert_eq!(cw.used_percentage, None);
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
    fn parse_model_scoped_statusline_shape() {
        // The shape the statusline uses for its other windows: normalized
        // used_percentage + epoch resets_at.
        let json = r#"{"rate_limits": {"model_scoped": [
            {"display_name": "Fable", "used_percentage": 18, "resets_at": 1700000600}
        ]}}"#;
        let rl = parse(json).unwrap().rate_limits.unwrap();
        let f = rl.fable().unwrap();
        assert_eq!(f.used_percentage, Some(18.0));
        assert_eq!(f.resets_at, Some(1_700_000_600));
    }

    #[test]
    fn parse_model_scoped_internal_snapshot_shape() {
        // Claude Code's internal snapshot shape: utilization fraction (0-1)
        // + ISO-8601 resets_at.
        let json = r#"{"rate_limits": {"model_scoped": [
            {"display_name": "Fable 5", "utilization": 0.18, "resets_at": "2026-07-20T07:00:00.000Z"}
        ]}}"#;
        let rl = parse(json).unwrap().rate_limits.unwrap();
        let f = rl.fable().unwrap();
        assert_eq!(f.used_percentage, Some(18.0));
        assert_eq!(f.resets_at, Some(1_784_530_800));
    }

    #[test]
    fn parse_model_scoped_null_fields_are_unknown() {
        let json = r#"{"rate_limits": {"model_scoped": [
            {"display_name": "Fable", "utilization": null, "resets_at": null}
        ]}}"#;
        let rl = parse(json).unwrap().rate_limits.unwrap();
        let f = rl.fable().unwrap();
        assert_eq!(f.used_percentage, None);
        assert_eq!(f.resets_at, None);
    }

    #[test]
    fn fable_ignores_other_model_buckets() {
        let json = r#"{"rate_limits": {"model_scoped": [
            {"display_name": "Opus", "used_percentage": 50},
            {"display_name": "Fable 5", "used_percentage": 18}
        ]}}"#;
        let rl = parse(json).unwrap().rate_limits.unwrap();
        assert_eq!(rl.fable().unwrap().used_percentage, Some(18.0));
        assert_eq!(rl.model_scoped.len(), 2);
    }

    #[test]
    fn malformed_model_scoped_entry_does_not_fail_payload() {
        // A junk entry (or the whole field being the wrong type) must drop
        // silently — nuking the entire statusline over one experimental
        // field would be worse than missing the segment.
        let json = r#"{
            "model": {"id": "claude-fable-5"},
            "rate_limits": {
                "five_hour": {"used_percentage": 42},
                "model_scoped": [
                    {"display_name": 42},
                    "junk",
                    {"display_name": "Fable", "used_percentage": 18}
                ]
            }
        }"#;
        let input = parse(json).unwrap();
        let rl = input.rate_limits.unwrap();
        assert_eq!(rl.five_hour.as_ref().unwrap().used_percentage, 42.0);
        assert_eq!(rl.model_scoped.len(), 1);
        assert_eq!(rl.fable().unwrap().used_percentage, Some(18.0));

        let json =
            r#"{"rate_limits": {"five_hour": {"used_percentage": 42}, "model_scoped": "wat"}}"#;
        let rl = parse(json).unwrap().rate_limits.unwrap();
        assert!(rl.model_scoped.is_empty());
        assert!(rl.five_hour.is_some());
    }

    #[test]
    fn parse_unknown_fields_are_ignored() {
        let json = r#"{"future_field": 42, "model": {"id": "x"}}"#;
        let input = parse(json).unwrap();
        assert_eq!(input.model.id, "x");
    }
}
