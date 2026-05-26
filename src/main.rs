//! Binary entry — thin wrapper that wires stdin/IO into the pure renderer.
//! Statusline output must never crash the parent (Claude Code reads stdout
//! and ignores errors), so errors are swallowed to stderr.

use anyhow::Result;
use claude_quota_bar::{cache, git, input, render, theme, time_fmt, transcript};
use std::io::Read;

/// Default prompt-cache TTL. Anthropic's standard cache lives 5 minutes, but
/// Claude.ai subscribers (Pro/Max) — exactly the users who get `rate_limits`,
/// our audience — are automatically granted the 1-hour extended cache, and
/// neither stdin nor the transcript exposes which TTL is active. Defaulting
/// to 3600s avoids reporting "COLD" while a subscriber's cache is still warm;
/// `STATUSLINE_CACHE_TTL` (seconds) overrides for the 5-minute case.
const DEFAULT_CACHE_TTL_SECS: f64 = 3600.0;

fn main() {
    if let Err(e) = run() {
        eprintln!("claude-quota-bar: {e}");
    }
}

fn run() -> Result<()> {
    // Handle conventional CLI flags before touching stdin. Without this a
    // `claude-quota-bar --version` would block on stdin and then render a
    // status line from stale cache — surprising for a published CLI, and a
    // path that could overwrite the global cache.
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-V" | "--version" => {
                println!("claude-quota-bar {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                print_help();
                return Ok(());
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
    // One transcript scan yields both the context-token count and the
    // timestamp of that last turn — the anchor for prompt-cache age.
    let last_usage = transcript::last_usage(&data.transcript_path);
    let transcript_tokens = last_usage.as_ref().map(|u| u.tokens);
    let cache_state = last_usage
        .as_ref()
        .and_then(|u| u.timestamp_unix)
        .map(|ts| {
            let age = now_unix.saturating_sub(ts) as f64;
            time_fmt::cache_remaining(Some(age), cache_ttl_secs())
        });

    let layout: Vec<String> = match std::env::var("STATUSLINE_LAYOUT") {
        Ok(s) => s.split(',').map(|x| x.trim().to_string()).collect(),
        Err(_) => render::DEFAULT_LAYOUT
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };

    let ctx = render::Context {
        input: &data,
        theme: &theme::GRAPHITE,
        now_unix,
        cache_state,
        git_info: git_info.as_ref(),
        layout: &layout,
        transcript_tokens,
    };

    let line = render::render(&ctx);
    println!("{line}");
    Ok(())
}

/// Resolve the prompt-cache TTL: `STATUSLINE_CACHE_TTL` (seconds) if set and
/// parseable to a positive number, otherwise [`DEFAULT_CACHE_TTL_SECS`].
fn cache_ttl_secs() -> f64 {
    std::env::var("STATUSLINE_CACHE_TTL")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|&v| v > 0.0)
        .unwrap_or(DEFAULT_CACHE_TTL_SECS)
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
         \x20 STATUSLINE_LAYOUT     Comma-separated segments (default: 5h,7d,model,cache,dir)\n\
         \x20 STATUSLINE_CACHE_TTL  Prompt-cache TTL in seconds (default: {ttl})\n\
         \x20 NO_COLOR              Disable ANSI color; fall back to block glyphs",
        ver = env!("CARGO_PKG_VERSION"),
        ttl = DEFAULT_CACHE_TTL_SECS as u64,
    );
}
