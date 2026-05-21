//! Binary entry — thin wrapper that wires stdin/IO into the pure renderer.
//! Statusline output must never crash the parent (Claude Code reads stdout
//! and ignores errors), so errors are swallowed to stderr.

use anyhow::Result;
use claude_quota_bar::{cache, git, input, render, theme, time_fmt, transcript};
use std::io::Read;

fn main() {
    if let Err(e) = run() {
        eprintln!("claude-quota-bar: {e}");
    }
}

fn run() -> Result<()> {
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
    let transcript_tokens = transcript::last_usage_tokens(&data.transcript_path);

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
        now_unix: time_fmt::now_unix(),
        cache_state: None,
        git_info: git_info.as_ref(),
        layout: &layout,
        transcript_tokens,
    };

    let line = render::render(&ctx);
    println!("{line}");
    Ok(())
}
