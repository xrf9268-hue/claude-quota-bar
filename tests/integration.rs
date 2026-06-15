//! End-to-end binary tests. Spawn the compiled binary, feed JSON via
//! stdin, assert visible content of the rendered status line.
//!
//! Each test uses a fresh tempdir as `HOME` so the cross-session cache
//! (`~/.cache/claude-quota-bar/last_stdin.json`) doesn't bleed state from
//! a prior test that wrote rate_limits.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

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
fn layout_env_var_controls_segments() {
    let (stdout, _h) = run(&fixture("full_session.json"), Some("model"));
    assert!(stdout.contains("Opus 4.7"));
    assert!(
        !stdout.contains("42%"),
        "5h should be hidden when layout=model: {stdout:?}"
    );
}
