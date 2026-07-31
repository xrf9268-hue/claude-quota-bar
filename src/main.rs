//! Binary entry — thin wrapper that wires stdin/IO into the pure renderer.
//! Statusline output must never crash the parent (Claude Code reads stdout
//! and ignores errors), so every fallible step degrades in place: invalid
//! stdin parses to defaults and cache I/O errors are ignored.

use claude_quota_bar::{cache, git, input, render, theme, time_fmt};
use std::io::Read;

fn main() {
    // Handle conventional CLI flags before touching stdin. Without this a
    // `claude-quota-bar --version` would block on stdin and then render a
    // status line from stale cache — surprising for a published CLI, and a
    // path that could overwrite the global cache.
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-V" | "--version" => {
                println!("claude-quota-bar {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-h" | "--help" => {
                print_help();
                return;
            }
            _ => {}
        }
    }

    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);

    // Parse stdin best-effort. Invalid or empty stdin still produces a
    // (mostly blank) status line — better than a crashed parent process.
    let mut data = if raw.trim().is_empty() {
        input::Input::default()
    } else {
        input::parse(&raw).unwrap_or_default()
    };

    // Persist BEFORE hydrating: if we did it after fill_from_cache, then a
    // partial stdin (no rate_limits) would inherit rate_limits from the
    // cache, satisfy maybe_save's "has rate_limits" check, and overwrite
    // the good cache with the partial raw payload — losing the data the
    // next render relies on. Save reflects only what stdin actually shipped.
    let _ = cache::maybe_save(&data, &raw);
    cache::fill_from_cache(&mut data);

    let git_info = git::status(&data.workspace.current_dir);

    let now_unix = time_fmt::now_unix();

    // Session elapsed time is Claude Code's own wall-clock counter
    // (`cost.total_duration_ms`), converted to seconds. No local ledger: the
    // statusline is a stateless render. `None` when stdin shipped no `cost`
    // object yet, so the segment hides instead of showing a fake 0s.
    let session_elapsed_secs = data.cost.as_ref().map(|c| c.total_duration_ms / 1000);

    let layout: Vec<String> = match std::env::var("STATUSLINE_LAYOUT") {
        Ok(s) => s.split(',').map(|x| x.trim().to_string()).collect(),
        Err(_) => render::DEFAULT_LAYOUT
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };

    // Severity thresholds: env override, silently falling back to the
    // 30/70 defaults on anything unparseable (degrade, never crash).
    let mut active_theme = theme::GRAPHITE;
    if let Ok(s) = std::env::var("STATUSLINE_THRESHOLDS") {
        if let Some((warn, hot)) = theme::parse_thresholds(&s) {
            active_theme.warn_threshold = warn;
            active_theme.hot_threshold = hot;
        }
    }

    let ctx = render::Context {
        input: &data,
        theme: &active_theme,
        now_unix,
        git_info: git_info.as_ref(),
        layout: &layout,
        session_elapsed_secs,
    };

    let line = render::render(&ctx);
    println!("{line}");
}

fn print_help() {
    println!(
        "claude-quota-bar {ver} — fast Claude Code statusline\n\
         \n\
         Reads Claude Code's status-line JSON on stdin and prints one\n\
         formatted line. Intended to be wired into ~/.claude/settings.json\n\
         as a `statusLine` command, not run by hand.\n\
         \n\
         Usage:\n\
         \x20 claude-quota-bar [--version] [--help]\n\
         \n\
         Environment:\n\
         \x20 STATUSLINE_LAYOUT      Comma-separated segments (default: 5h,7d,fable,model,session,dir)\n\
         \x20 STATUSLINE_THRESHOLDS  Severity flip points as \"warn,hot\" percentages (default: 30,70)\n\
         \x20 NO_COLOR               Disable ANSI color; fall back to block glyphs",
        ver = env!("CARGO_PKG_VERSION"),
    );
}
