# claude-quota-bar

Fast Rust statusline for [Claude Code](https://github.com/anthropics/claude-code).
Battery-style 5-hour / 7-day quota bars, context-window indicator, prompt-cache
state, and `dir:branch *N` — at ~2.5ms cold start and a 459KB binary.

```
5h[███42%░░░░]⏰26m | 7d[███35%░░░░]⏰8d3h | Opus 4.7(71.0k/200.0k) | cache 4m12s | proj:main *3
```

## Why this and not the Python ones

- **Speed.** Claude Code renders statusLine on every prompt. Python is ~50ms cold
  start; this is ~2.5ms. Subjective UX difference is real.
- **No runtime deps.** One stripped binary; no `claude-monitor`, no `pip`.
- **Focused.** Shows what you need to make in-session decisions (how much quota
  is left, when does it reset) — not what you already did (cost, lines changed).

## Install

```sh
# npm (recommended — works on any platform with Node ≥ 16)
npm install -g claude-quota-bar

# cargo
cargo install --git https://github.com/xrf9268-hue/claude-quota-bar

# pre-built binary (macOS arm64 example)
curl -L https://github.com/xrf9268-hue/claude-quota-bar/releases/latest/download/claude-quota-bar-aarch64-apple-darwin.tar.gz | tar xz
mv claude-quota-bar /usr/local/bin/
```

Then wire it into Claude Code (`~/.claude/settings.json`):

```json
{
  "statusLine": {
    "type": "command",
    "command": "claude-quota-bar",
    "padding": 0
  }
}
```

## Segments

Default layout: `5h,7d,model,cache,dir`.

| Segment | Source | What it shows |
|---------|--------|---------------|
| `5h`    | `rate_limits.five_hour` | Battery bar with `%` inside, plus `⏰` countdown to reset |
| `7d`    | `rate_limits.seven_day` | Same, weekly window |
| `model` | `model` + `context_window` | `Opus 4.7(71k/200k)` — model + ctx tokens used / window |
| `cache` | transcript scan | Time left on the prompt cache (`4m12s`), or `COLD` once it has expired |
| `dir`   | `workspace.current_dir` + git | `proj:main *3 ↑1 ↓2` — dir, branch, dirty count, ahead/behind |

When Anthropic hasn't yet shipped `rate_limits` (first few renders of a fresh
session), the bar displays `--%`. A cross-session cache at
`~/.cache/claude-quota-bar/last_stdin.json` restores the most recent values, so
opening a new terminal doesn't blank the bar.

## Configuration

Configured via environment variables:

| Variable | Default | Meaning |
|----------|---------|---------|
| `STATUSLINE_LAYOUT` | `5h,7d,model,cache,dir` | Comma-separated segment names (order matters) |
| `STATUSLINE_CACHE_TTL` | `3600` | Prompt-cache lifetime in seconds. The default assumes the 1-hour extended cache that Claude.ai Pro/Max accounts get automatically; set `300` for the standard 5-minute cache. The active TTL isn't exposed to the statusline, so it can't be auto-detected. |
| `NO_COLOR` | unset | If set, strips all ANSI — falls back to `█`/`░` glyphs |

Severity thresholds (green / yellow / red) flip at 30% and 70% quota used.

## Development

Requires Rust ≥ 1.85 (Edition 2024).

```sh
cargo test                          # 70 tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

# Manual visual test
cat <<EOF | cargo run --release
{
  "model": {"display_name": "Opus 4.7"},
  "workspace": {"current_dir": "/tmp"},
  "rate_limits": {"five_hour": {"used_percentage": 42, "resets_at": $(($(date +%s) + 26*60))}}
}
EOF
```

## License

MIT — see [LICENSE](./LICENSE).

## Acknowledgements

Visual inspired by [leeguooooo/claude-code-usage-bar](https://github.com/leeguooooo/claude-code-usage-bar).
Release / NPM publishing pattern adapted from [Haleclipse/CCometixLine](https://github.com/Haleclipse/CCometixLine).
