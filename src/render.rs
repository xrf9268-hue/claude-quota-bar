//! Status-line composition. Segments are pure functions over a `Context`;
//! the caller in `main.rs` handles all I/O (stdin parse, git lookup, cache
//! read) before invoking `render`.

use crate::ansi::{fg, reset};
use crate::git::GitInfo;
use crate::input::{Input, Window};
use crate::progress::battery_bar;
use crate::theme::Theme;
use crate::time_fmt::{countdown, fmt_tokens};

pub const DEFAULT_LAYOUT: &[&str] = &["5h", "7d", "model", "cache", "dir"];
pub const BAR_WIDTH: usize = 10;
pub const BRANCH_MAX_LEN: usize = 25;

pub struct Context<'a> {
    pub input: &'a Input,
    pub theme: &'a Theme,
    pub now_unix: u64,
    pub cache_state: Option<String>,
    pub git_info: Option<&'a GitInfo>,
    pub layout: &'a [String],
}

pub fn render(ctx: &Context) -> String {
    let mute = fg(ctx.theme.mute);
    let r = reset();
    let sep = format!("{mute} | {r}");

    let mut parts: Vec<String> = Vec::new();
    for seg in ctx.layout {
        if let Some(rendered) = build_segment(seg.as_str(), ctx) {
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
        "cache" => build_cache(ctx),
        "dir" => build_dir(ctx),
        _ => None,
    }
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
            let used = cw.total_input_tokens + cw.total_output_tokens;
            let used_text = fmt_tokens(used);
            let win_text = fmt_tokens(cw.context_window_size);
            format!("{model_color}{name}{r}{mute}({r}{used_text}{mute}/{r}{win_text}{mute}){r}")
        }
        _ => format!("{model_color}{name}{r}"),
    }
}

fn build_cache(ctx: &Context) -> Option<String> {
    let state = ctx.cache_state.as_ref()?;
    let mute = fg(ctx.theme.mute);
    let ink = fg(ctx.theme.ink);
    let warn = fg(ctx.theme.warn);
    let r = reset();
    // Color the state by urgency: COLD is the warning color so the user
    // notices at a glance the cache has rolled.
    let state_fg = if state == "COLD" { &warn } else { &ink };
    Some(format!("{mute}cache{r} {state_fg}{state}{r}"))
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
            out.push_str(&format!(" {warn}*{}{r}", git.dirty_count));
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
            cache_state: None,
            git_info: git,
            layout,
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
                used_percentage: Some(35.5),
                context_window_size: 200_000,
                total_input_tokens: 65_000,
                total_output_tokens: 6_000,
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
    fn model_includes_context_tokens() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["model"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert!(
                out.contains("71.0k"),
                "missing input+output tokens in {out:?}"
            );
            assert!(out.contains("200.0k"), "missing window size in {out:?}");
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
                detached: false,
                dirty_count: 0,
                ahead: 0,
                behind: 0,
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
                detached: false,
                dirty_count: 3,
                ahead: 0,
                behind: 0,
            };
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, Some(&git))));
            assert!(out.contains("*3"), "missing dirty count in {out:?}");
        });
    }

    #[test]
    fn dir_segment_hides_dirty_when_clean() {
        no_color(|| {
            let inp = full_input();
            let git = GitInfo {
                branch: "main".into(),
                detached: false,
                dirty_count: 0,
                ahead: 0,
                behind: 0,
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
    fn cache_segment_shows_state() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["cache"]);
            let mut c = ctx(&inp, &lay, None);
            c.cache_state = Some("2m45s".into());
            let out = strip_ansi(&render(&c));
            assert!(out.contains("cache"));
            assert!(out.contains("2m45s"));
        });
    }

    #[test]
    fn cache_segment_hidden_when_no_state() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["cache"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, None)));
            assert_eq!(out, "");
        });
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
                detached: false,
                dirty_count: 0,
                ahead: 0,
                behind: 0,
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
                detached: false,
                dirty_count: 0,
                ahead: 2,
                behind: 1,
            };
            let lay = layout(&["dir"]);
            let out = strip_ansi(&render(&ctx(&inp, &lay, Some(&git))));
            assert!(out.contains("↑2"));
            assert!(out.contains("↓1"));
        });
    }

    #[test]
    fn cold_cache_state_renders() {
        no_color(|| {
            let inp = full_input();
            let lay = layout(&["cache"]);
            let mut c = ctx(&inp, &lay, None);
            c.cache_state = Some("COLD".into());
            let out = strip_ansi(&render(&c));
            assert!(out.contains("COLD"));
        });
    }
}
