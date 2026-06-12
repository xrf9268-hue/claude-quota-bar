//! Status-line composition. Segments are pure functions over a `Context`;
//! the caller in `main.rs` handles all I/O (stdin parse, git lookup, cache
//! read) before invoking `render`.

use crate::ansi::{fg, reset};
use crate::git::GitInfo;
use crate::input::{Input, Window};
use crate::progress::battery_bar;
use crate::theme::Theme;
use crate::time_fmt::{countdown, fmt_elapsed, fmt_tokens};

pub const DEFAULT_LAYOUT: &[&str] = &["5h", "7d", "model", "session", "dir"];
pub const BAR_WIDTH: usize = 10;
pub const BRANCH_MAX_LEN: usize = 25;
/// Cap on the single-dirty-file path width. Past this, fall back to `*1`
/// so the statusline doesn't get blown out by paths like `tests/.../foo`.
pub const DIRTY_FILE_MAX_LEN: usize = 30;
/// Fallback context-usage estimate when stdin doesn't yet report a usable
/// token count. Covers the system prompt (~3k) + tools
/// (~15k) + CLAUDE.md (~300) that load before the first user turn, so the
/// bar never starts at a misleading 0%.
const BASELINE_TOKENS: u64 = 20_000;

pub struct Context<'a> {
    pub input: &'a Input,
    pub theme: &'a Theme,
    pub now_unix: u64,
    pub git_info: Option<&'a GitInfo>,
    pub layout: &'a [String],
    /// Accumulated active time of this session (idle gaps excluded), from
    /// `session::update`. `None` = no usable session_id, segment hides.
    pub session_active_secs: Option<u64>,
}

pub fn render(ctx: &Context) -> String {
    let mute = fg(ctx.theme.mute);
    let r = reset();
    let sep = format!("{mute} | {r}");

    // A layout with no recognized segment name (e.g. a stale cache-only
    // STATUSLINE_LAYOUT from before that segment was removed) would render
    // a blank line every prompt with no diagnostic — fall back to the
    // default layout instead. A recognized segment that legitimately hides
    // (dir with an empty cwd) still yields empty output.
    let recognized = ctx
        .layout
        .iter()
        .any(|s| DEFAULT_LAYOUT.contains(&s.as_str()));
    let mut parts: Vec<String> = Vec::new();
    let names: &mut dyn Iterator<Item = &str> = if recognized {
        &mut ctx.layout.iter().map(String::as_str)
    } else {
        &mut DEFAULT_LAYOUT.iter().copied()
    };
    for seg in names {
        if let Some(rendered) = build_segment(seg, ctx) {
            if !rendered.is_empty() {
                parts.push(rendered);
            }
        }
    }
    parts.join(&sep)
}

fn build_segment(name: &str, ctx: &Context) -> Option<String> {
    match name {
        "5h" => Some(build_window(ctx, "5h", window_5h(ctx.input))),
        "7d" => Some(build_window(ctx, "7d", window_7d(ctx.input))),
        "model" => Some(build_model(ctx)),
        "session" => build_session(ctx),
        "dir" => build_dir(ctx),
        _ => None,
    }
}

fn build_session(ctx: &Context) -> Option<String> {
    let secs = ctx.session_active_secs?;
    let ink = fg(ctx.theme.ink);
    let r = reset();
    Some(format!("{ink}⏱{}{r}", fmt_elapsed(secs)))
}

fn window_5h(input: &Input) -> Option<&Window> {
    input.rate_limits.as_ref()?.five_hour.as_ref()
}

fn window_7d(input: &Input) -> Option<&Window> {
    input.rate_limits.as_ref()?.seven_day.as_ref()
}

fn build_window(ctx: &Context, label: &str, window: Option<&Window>) -> String {
    let pct = window.map(|w| w.used_percentage);
    let bar = battery_bar(pct, ctx.theme, BAR_WIDTH);
    let reset_text = window
        .and_then(|w| w.resets_at)
        .map(|r| countdown(ctx.now_unix, r))
        .unwrap_or_else(|| "--".to_string());

    let mute = fg(ctx.theme.mute);
    let ink = fg(ctx.theme.ink);
    let r = reset();
    format!("{ink}{label}{r}{mute}[{r}{bar}{mute}]{r}{ink}⏰{reset_text}{r}")
}

fn build_model(ctx: &Context) -> String {
    let name = ctx.input.model.name();
    let model_color = fg(ctx.theme.model);
    let mute = fg(ctx.theme.mute);
    let r = reset();
    match &ctx.input.context_window {
        Some(cw) if cw.context_window_size > 0 => {
            // `total_input_tokens` is the current context occupancy (input +
            // cache reads + cache writes of the most recent API response,
            // Claude Code >= 2.1.132). Zero means "no API turn yet" — show
            // the ~20k baseline rather than a misleading 0. A count larger
            // than the window itself can't be a real occupancy (it's the
            // pre-2.1.132 cumulative-session total leaking through an old
            // Claude Code) — show "--" rather than a confident wrong number.
            let used_text = if cw.total_input_tokens > cw.context_window_size {
                "--".to_string()
            } else if cw.total_input_tokens > 0 {
                fmt_tokens(cw.total_input_tokens)
            } else {
                format!("~{}", fmt_tokens(BASELINE_TOKENS))
            };
            let win_text = fmt_tokens(cw.context_window_size);
            format!("{model_color}{name}{r}{mute}({r}{used_text}{mute}/{r}{win_text}{mute}){r}")
        }
        _ => format!("{model_color}{name}{r}"),
    }
}

fn build_dir(ctx: &Context) -> Option<String> {
    let cwd = &ctx.input.workspace.current_dir;
    if cwd.is_empty() {
        return None;
    }
    let basename = std::path::Path::new(cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(cwd.as_str());

    let mute = fg(ctx.theme.mute);
    let ink = fg(ctx.theme.ink);
    let warn = fg(ctx.theme.warn);
    let r = reset();
    let mut out = format!("{ink}{basename}{r}");
    if let Some(git) = ctx.git_info {
        let branch = truncate_branch(&git.branch, BRANCH_MAX_LEN);
        out.push_str(&format!("{mute}:{r}{ink}{branch}{r}"));
        if git.dirty_count > 0 {
            if let Some(file) = git
                .dirty_file
                .as_deref()
                .filter(|f| git.dirty_count == 1 && f.chars().count() <= DIRTY_FILE_MAX_LEN)
            {
                out.push_str(&format!(" {warn}*{file}{r}"));
            } else {
                out.push_str(&format!(" {warn}*{}{r}", git.dirty_count));
            }
        }
        if git.ahead > 0 {
            out.push_str(&format!(" {ink}↑{}{r}", git.ahead));
        }
        if git.behind > 0 {
            out.push_str(&format!(" {warn}↓{}{r}", git.behind));
        }
    }
    Some(out)
}

fn truncate_branch(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::strip_ansi;
    use crate::input::{ContextWindow, Model, RateLimits, Window, Workspace};
    use crate::theme::GRAPHITE;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn no_color<F: FnOnce()>(f: F) {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("NO_COLOR", "1") };
        f();
        unsafe { std::env::remove_var("NO_COLOR") };
    }

    fn layout(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    fn ctx<'a>(input: &'a Input, layout: &'a [String], git: Option<&'a GitInfo>) -> Context<'a> {
        Context {
            input,
            theme: &GRAPHITE,
            now_unix: 1_700_000_000,
            git_info: git,
            layout,
            session_active_secs: None,
        }
    }

    fn full_input() -> Input {
        Input {
            model: Model {
                id: "claude-opus-4-7".into(),
                display_name: "Opus 4.7".into(),
            },
            workspace: Workspace {
                current_dir: "/Users/x/proj".into(),
            },
            rate_limits: Some(RateLimits {
                five_hour: Some(Window {
                    used_percentage: 42.0,
                    resets_at: Some(1_700_000_000 + 26 * 60),
                }),
                seven_day: Some(Window {
                    used_percentage: 35.0,
                    resets_at: Some(1_700_000_000 + 8 * 86400 + 3 * 3600),
                }),
            }),
            context_window: Some(ContextWindow {
                context_window_size: 200_000,
                total_input_tokens: 65_000,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn renders_5h_percentage_in_bar() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["5h"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("42%"), "missing 42% in {out:?}");
        });
    }

    #[test]
    fn renders_7d_percentage_in_bar() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["7d"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("35%"), "missing 35% in {out:?}");
        });
    }

    #[test]
    fn renders_5h_label_prefix() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["5h"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("5h"), "missing 5h label in {out:?}");
        });
    }

    #[test]
    fn renders_reset_countdown_after_bar() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["5h"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("26m"), "missing 26m countdown in {out:?}");
        });
    }

    #[test]
    fn renders_model_name() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["model"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("Opus 4.7"), "missing model name in {out:?}");
        });
    }

    #[test]
    fn model_shows_baseline_when_stdin_tokens_zero() {
        no_color(|| {
            let mut inp = full_input();
            // Fresh session: stdin context_window present but
            // total_input_tokens still 0 — display the ~20k baseline so
            // users see the system prompt + tools load, not a misleading 0.
            if let Some(cw) = inp.context_window.as_mut() {
                cw.total_input_tokens = 0;
            }
            let lay = layout(&["model"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(
                out.contains("~20.0k"),
                "expected ~20.0k baseline in {out:?}"
            );
            assert!(out.contains("200.0k"));
        });
    }

    #[test]
    fn model_shows_stdin_total_input_tokens() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["model"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            // `total_input_tokens` already includes cache reads/writes and
            // the prior turn's output — display it as-is.
            assert!(out.contains("65.0k"), "expected 65.0k in {out:?}");
            assert!(out.contains("200.0k"), "missing window size in {out:?}");
        });
    }

    #[test]
    fn model_shows_unknown_when_tokens_exceed_window() {
        no_color(|| {
            let mut inp = full_input();
            // A count larger than the window itself can't be a real
            // occupancy — that's the pre-2.1.132 cumulative-session
            // semantics leaking through. Show "--", not a confident lie.
            if let Some(cw) = inp.context_window.as_mut() {
                cw.total_input_tokens = 1_500_000;
            }
            let lay = layout(&["model"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("--/200.0k"), "expected --/200.0k in {out:?}");
            assert!(!out.contains("1.5M"), "bogus count leaked: {out:?}");
        });
    }

    #[test]
    fn model_allows_exactly_full_window() {
        no_color(|| {
            let mut inp = full_input();
            if let Some(cw) = inp.context_window.as_mut() {
                cw.total_input_tokens = 200_000;
            }
            let lay = layout(&["model"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(
                out.contains("200.0k/200.0k"),
                "a 100%-full window is legitimate: {out:?}"
            );
        });
    }

    #[test]
    fn dir_segment_shows_basename() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("proj"), "missing dir basename in {out:?}");
        });
    }

    #[test]
    fn dir_segment_hidden_when_workspace_empty() {
        no_color(|| {
            let mut inp = full_input();
            inp.workspace.current_dir = String::new();
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert_eq!(out, "");
        });
    }

    #[test]
    fn dir_segment_with_git_shows_branch() {
        no_color(|| {
            let inp = full_input();
            let git = GitInfo {
                branch: "main".into(),
                ..Default::default()
            };
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, Some(&git))));
            assert!(out.contains("proj"));
            assert!(out.contains("main"), "missing branch in {out:?}");
        });
    }

    #[test]
    fn dir_segment_shows_dirty_count() {
        no_color(|| {
            let inp = full_input();
            let git = GitInfo {
                branch: "main".into(),
                dirty_count: 3,
                ..Default::default()
            };
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, Some(&git))));
            assert!(out.contains("*3"), "missing dirty count in {out:?}");
        });
    }

    #[test]
    fn dir_segment_shows_filename_when_single_dirty() {
        no_color(|| {
            let inp = full_input();
            let git = GitInfo {
                branch: "main".into(),
                dirty_count: 1,
                dirty_file: Some("src/foo.rs".into()),
                ..Default::default()
            };
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, Some(&git))));
            assert!(out.contains("src/foo.rs"), "missing filename in {out:?}");
            assert!(
                !out.contains("*1"),
                "should not show *1 when filename available: {out:?}"
            );
        });
    }

    #[test]
    fn dir_segment_falls_back_to_count_when_filename_too_long() {
        no_color(|| {
            let inp = full_input();
            let long = "a".repeat(40);
            let git = GitInfo {
                branch: "main".into(),
                dirty_count: 1,
                dirty_file: Some(long.clone()),
                ..Default::default()
            };
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, Some(&git))));
            assert!(
                out.contains("*1"),
                "expected *1 fallback for long filename: {out:?}"
            );
            assert!(
                !out.contains(&long),
                "long filename should not be shown: {out:?}"
            );
        });
    }

    #[test]
    fn dir_segment_hides_dirty_when_clean() {
        no_color(|| {
            let inp = full_input();
            let git = GitInfo {
                branch: "main".into(),
                ..Default::default()
            };
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, Some(&git))));
            assert!(
                !out.contains("*"),
                "unexpected dirty mark in clean state: {out:?}"
            );
        });
    }

    #[test]
    fn session_segment_shows_active_time() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["session"]);
            let mut c = ctx(&inp, &lay, None);
            c.session_active_secs = Some(2 * 3600 + 15 * 60);
            let out = strip_ansi(&render(&c));
            assert!(out.contains("2h15m"), "missing active time in {out:?}");
        });
    }

    #[test]
    fn session_segment_hidden_when_unknown() {
        no_color(|| {
            // No session_id on stdin (hand-run, invalid payload) → no
            // ledger → hide the segment rather than show a fake 0s.
            let inp = full_input();
            let lay = layout(&["session"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert_eq!(out, "");
        });
    }

    #[test]
    fn session_segment_shows_zero_on_fresh_session() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["session"]);
            let mut c = ctx(&inp, &lay, None);
            c.session_active_secs = Some(0);
            let out = strip_ansi(&render(&c));
            assert!(out.contains("0s"), "fresh session shows 0s: {out:?}");
        });
    }

    #[test]
    fn default_layout_places_session_before_dir() {
        assert_eq!(DEFAULT_LAYOUT, &["5h", "7d", "model", "session", "dir"]);
    }

    #[test]
    fn segments_separated_by_pipe() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["5h", "model"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains(" | "), "missing pipe separator in {out:?}");
        });
    }

    #[test]
    fn unknown_rate_limit_shows_dashes() {
        no_color(|| {
            let mut inp = full_input();
            inp.rate_limits = None;
            let lay = layout(&["5h"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("--%"), "expected --% placeholder in {out:?}");
        });
    }

    #[test]
    fn unknown_segments_in_layout_are_skipped() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["bogus", "5h"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("42%"));
            assert!(!out.contains("bogus"));
        });
    }

    #[test]
    fn dir_segment_truncates_long_branch() {
        no_color(|| {
            let inp = full_input();
            let long = "feature/this-is-a-very-long-branch-name-that-needs-truncation";
            let git = GitInfo {
                branch: long.into(),
                ..Default::default()
            };
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, Some(&git))));
            // The full branch is 61 chars; the truncated form must be
            // <= BRANCH_MAX_LEN + 1 (for the ellipsis) and must end in '…'.
            assert!(!out.contains(long), "untruncated long branch in {out:?}");
            assert!(out.contains('…'), "missing ellipsis in {out:?}");
            // Quick upper-bound check on visible segment length.
            let dir_part = out.rsplit(" | ").next().unwrap_or(&out);
            assert!(
                dir_part.chars().count() <= "proj:".len() + BRANCH_MAX_LEN + 1,
                "dir segment longer than expected: {dir_part:?}"
            );
        });
    }

    #[test]
    fn ahead_and_behind_counts_shown() {
        no_color(|| {
            let inp = full_input();
            let git = GitInfo {
                branch: "main".into(),
                ahead: 2,
                behind: 1,
                ..Default::default()
            };
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, Some(&git))));
            assert!(out.contains("↑2"));
            assert!(out.contains("↓1"));
        });
    }

    #[test]
    fn removed_cache_segment_in_layout_is_skipped() {
        no_color(|| {
            // Users with a stale STATUSLINE_LAYOUT still listing "cache"
            // must get the other segments, not a crash or a stray label.
            let inp = full_input();
            let lay = layout(&["cache", "5h"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("42%"));
            assert!(
                !out.contains("cache"),
                "stale cache segment leaked: {out:?}"
            );
        });
    }

    #[test]
    fn layout_with_no_known_segment_falls_back_to_default() {
        no_color(|| {
            // A layout made entirely of unknown names (e.g. a stale
            // cache-only STATUSLINE_LAYOUT from before the segment was
            // removed) must not blank the statusline.
            let inp = full_input();
            let lay = layout(&["cache"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(out.contains("42%"), "expected default fallback: {out:?}");
            assert!(
                out.contains("Opus 4.7"),
                "expected default fallback: {out:?}"
            );
        });
    }
}
