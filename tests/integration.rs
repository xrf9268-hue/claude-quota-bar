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
fn session_segment_advances_persisted_active_time() {
    let home = TempDir::new().expect("tempdir");
    let dir = home.path().join(".cache/claude-quota-bar/sessions");
    fs::create_dir_all(&dir).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // 2h15m accrued, last heartbeat 10s ago, api counter differs from the
    // fixture's (fixture cost has no total_api_duration_ms → parses as 0)
    // so the 10s gap counts as progress.
    fs::write(
        dir.join("test-session.json"),
        format!(
            r#"{{"active_secs":8100,"last_seen_unix":{},"last_api_ms":1}}"#,
            now - 10
        ),
    )
    .unwrap();

    let stdout = run_in_home(&fixture("full_session.json"), Some("session"), &home);
    assert!(
        stdout.contains("2h15m"),
        "missing active time in {stdout:?}"
    );
}

#[test]
fn session_segment_hidden_without_session_id() {
    let (stdout, _h) = run(&fixture("no_rate_limits.json"), Some("session"));
    assert_eq!(stdout.trim(), "", "segment must hide: {stdout:?}");
}

#[test]
fn fresh_session_shows_zero_active_time() {
    let (stdout, _h) = run(&fixture("full_session.json"), None);
    assert!(stdout.contains("⏳0s"), "missing ⏳0s in {stdout:?}");
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
