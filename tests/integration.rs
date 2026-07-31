//! End-to-end binary tests. Spawn the compiled binary, feed JSON via
//! stdin, assert visible content of the rendered status line.
//!
//! Each test uses a fresh tempdir as `HOME` so the cross-session cache
//! (`~/.cache/claude-quota-bar/last_stdin.json`) doesn't bleed state from
//! a prior test that wrote rate_limits.

use assert_cmd::Command;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_secs()
}

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(path).expect("fixture missing")
}

fn run(stdin: &str, layout: Option<&str>) -> (String, TempDir) {
    let home = TempDir::new().expect("tempdir");
    let stdout = run_in_home(stdin, layout, &home);
    (stdout, home)
}

fn run_in_home(stdin: &str, layout: Option<&str>, home: &TempDir) -> String {
    let mut cmd = Command::cargo_bin("claude-quota-bar").unwrap();
    cmd.env("NO_COLOR", "1")
        .env("HOME", home.path())
        .env_remove("STATUSLINE_LAYOUT");
    if let Some(l) = layout {
        cmd.env("STATUSLINE_LAYOUT", l);
    }
    let out = cmd.write_stdin(stdin).assert().success();
    String::from_utf8_lossy(&out.get_output().stdout).into_owned()
}

#[test]
fn full_session_renders_quota_and_model() {
    let (stdout, _h) = run(&fixture("full_session.json"), None);
    assert!(stdout.contains("42%"), "missing 5h pct in {stdout:?}");
    assert!(stdout.contains("35%"), "missing 7d pct in {stdout:?}");
    assert!(stdout.contains("Opus 4.7"), "missing model in {stdout:?}");
    assert!(stdout.contains("proj"), "missing dir in {stdout:?}");
}

#[test]
fn no_rate_limits_shows_dashes() {
    let (stdout, _h) = run(&fixture("no_rate_limits.json"), None);
    assert!(
        stdout.contains("--%"),
        "missing --% placeholder in {stdout:?}"
    );
    assert!(stdout.contains("Opus 4.7"), "missing model in {stdout:?}");
}

#[test]
fn empty_stdin_does_not_crash() {
    run("", None);
}

#[test]
fn invalid_json_does_not_crash() {
    run("not json", None);
}

#[test]
fn session_segment_shows_total_duration() {
    // full_session.json carries cost.total_duration_ms = 145000 (145s).
    // The segment renders Claude Code's own wall-clock counter directly.
    let (stdout, _h) = run(&fixture("full_session.json"), Some("session"));
    assert!(
        stdout.contains("⏳2m"),
        "missing elapsed time in {stdout:?}"
    );
}

#[test]
fn session_segment_hidden_without_cost() {
    // no_rate_limits.json ships no `cost` object → no duration → segment
    // hides rather than showing a fake 0s.
    let (stdout, _h) = run(&fixture("no_rate_limits.json"), Some("session"));
    assert_eq!(stdout.trim(), "", "segment must hide: {stdout:?}");
}

#[test]
fn fable_bucket_renders_in_default_layout() {
    // fable_session.json carries rate_limits.model_scoped with a Fable
    // bucket (utilization 0.18, ISO resets_at) — the internal-snapshot wire
    // shape. The segment is part of the default layout.
    let (stdout, _h) = run(&fixture("fable_session.json"), None);
    assert!(
        stdout.contains("Fable["),
        "missing Fable segment in {stdout:?}"
    );
    assert!(stdout.contains("18%"), "missing Fable pct in {stdout:?}");
    assert!(stdout.contains("42%"), "5h must still render in {stdout:?}");
}

#[test]
fn fable_segment_hidden_without_bucket() {
    // full_session.json has rate_limits but no model_scoped — the segment
    // must hide instead of rendering a dead "Fable[--%]".
    let (stdout, _h) = run(&fixture("full_session.json"), Some("fable"));
    assert_eq!(stdout.trim(), "", "segment must hide: {stdout:?}");
}

#[test]
fn model_segment_shows_context_percentage() {
    // full_session.json ships context_window.used_percentage = 35.5.
    // Claude Code's own number (→ 36%) must win over the value derived
    // from the token fields (65k/200k → 33%).
    let (stdout, _h) = run(&fixture("full_session.json"), Some("model"));
    assert!(stdout.contains("·36%"), "missing ctx pct in {stdout:?}");
}

#[test]
fn pace_marker_renders_end_to_end() {
    // 60% used with 4h of the 5h window still to go (20% elapsed) → far
    // over pace → the ▲ marker sits between the bar and the countdown.
    let json = format!(
        r#"{{"session_id":"s","model":{{"display_name":"Opus 4.7"}},"rate_limits":{{"five_hour":{{"used_percentage":60,"resets_at":{}}}}}}}"#,
        now_unix() + 4 * 3600
    );
    let (stdout, _h) = run(&json, Some("5h"));
    assert!(stdout.contains("]▲⏰"), "missing pace marker in {stdout:?}");
}

#[test]
fn windfall_marker_renders_end_to_end() {
    // 42% used, 18m to reset: more than 30pp of quota is about to expire →
    // the ✦ use-it-or-lose-it marker shows.
    let json = format!(
        r#"{{"session_id":"s","model":{{"display_name":"Opus 4.7"}},"rate_limits":{{"five_hour":{{"used_percentage":42,"resets_at":{}}}}}}}"#,
        now_unix() + 18 * 60
    );
    let (stdout, _h) = run(&json, Some("5h"));
    assert!(
        stdout.contains("]✦⏰"),
        "missing windfall marker in {stdout:?}"
    );
}

#[test]
fn thresholds_env_var_recolors_severity() {
    // 42% used is amber under the default 30/70 thresholds but must stay
    // green when STATUSLINE_THRESHOLDS raises the warn flip to 50%.
    let home = TempDir::new().expect("tempdir");
    let json = format!(
        r#"{{"session_id":"s","rate_limits":{{"five_hour":{{"used_percentage":42,"resets_at":{}}}}}}}"#,
        now_unix() + 3600
    );
    let mut cmd = Command::cargo_bin("claude-quota-bar").unwrap();
    cmd.env_remove("NO_COLOR")
        .env("HOME", home.path())
        .env("STATUSLINE_LAYOUT", "5h")
        .env("STATUSLINE_THRESHOLDS", "50,80");
    let out = cmd.write_stdin(json).assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
    let [okr, okg, okb] = claude_quota_bar::theme::GRAPHITE.bg_ok;
    let [wr, wg, wb] = claude_quota_bar::theme::GRAPHITE.bg_warn;
    assert!(
        stdout.contains(&format!("\x1b[48;2;{okr};{okg};{okb}m")),
        "expected ok bg at 42% with 50,80 thresholds: {stdout:?}"
    );
    assert!(
        !stdout.contains(&format!("\x1b[48;2;{wr};{wg};{wb}m")),
        "warn bg leaked at 42% with 50,80 thresholds: {stdout:?}"
    );
}

#[test]
fn layout_env_var_controls_segments() {
    let (stdout, _h) = run(&fixture("full_session.json"), Some("model"));
    assert!(stdout.contains("Opus 4.7"));
    assert!(
        !stdout.contains("42%"),
        "5h should be hidden when layout=model: {stdout:?}"
    );
}
