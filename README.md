# claude-quota-bar

Fast Rust statusline for [Claude Code](https://github.com/anthropics/claude-code).
Battery-style 5-hour / 7-day quota bars, context-window indicator, session
elapsed time, and `dir:branch *N` — at ~2.5ms cold start and a ~0.5MB binary.

```
5h[███42%░░░░]✦⏰26m | 7d[███35%░░░░]⏰8d3h | Opus 4.7(71.0k/1.0M·7%) | ⏳2h15m | proj:main *3
```

Requires Claude Code ≥ 2.1.132 (where `context_window.total_input_tokens`
reports the current context occupancy rather than a cumulative session total).

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

# cargo (compiles from source — npm ships a prebuilt binary, so it's faster)
cargo install claude-quota-bar

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

Default layout: `5h,7d,fable,model,session,dir`.

| Segment | Source | What it shows |
|---------|--------|---------------|
| `5h`    | `rate_limits.five_hour` | Battery bar with `%` inside, `⏰` countdown to reset, and pace hints (`▲`/`✦`, see below) |
| `7d`    | `rate_limits.seven_day` | Same, weekly window |
| `fable` | `rate_limits.model_scoped` | Same, for the per-model Fable allowance (Max/Team Premium: Fable at 50% of limits). Hidden until the server ships a Fable bucket |
| `model` | `model` + `context_window` | `Opus 4.7(71.0k/1.0M·7%)` — model, ctx tokens used / window, and ctx occupancy % (Claude Code's own `used_percentage` when shipped, derived from the token counts otherwise) |
| `session` | `cost.total_duration_ms` | `⏳2h15m` — wall-clock time this session |
| `sid`   | `session_id` | `#3f9a1c2b-7d4e-4a10-9c33-8b21ef0d55aa` — the session id, opt-in (see below) |

Plus one non-segment token: `nl` breaks the line, so the remaining segments
render as a second status row.
| `dir`   | `workspace.current_dir` + git | `proj:main *3 ↑1 ↓2` — dir, branch, dirty count, ahead/behind |

### Pace hints

The `5h`/`7d` bars compare quota used against how much of the window has
elapsed — both are known, since the window length is fixed and `resets_at` is
in the payload. One glyph can appear between the bar and the countdown:

- `▲` — usage runs ≥10 percentage points ahead of elapsed time (amber; red at
  ≥25pp). At this pace you hit the wall before the reset.
- `✦` — the window resets within 10% of its length and ≥30% of the quota is
  still unused (green). Expiring allowance: use it or lose it.

No glyph means on pace or comfortably under — usage trails wall-clock most of
the time (nights, weekends), so a "below pace" marker would be permanently lit
noise. Hints hide when `resets_at` is missing, already past, or farther out
than the window length itself (observed on real `7d` payloads — pace math
against a wrong window length would mislead). The `fable` bar never shows
hints: model-scoped buckets carry no contractual window length.

### How `session` counts time

It shows Claude Code's own `cost.total_duration_ms` — wall-clock since the
session started — formatted directly. A few consequences worth knowing:

- It **includes idle time** (it keeps ticking while you're reading a reply or
  at lunch). Without `statusLine.refreshInterval` set, the statusline only
  re-renders at each turn's completion, so the value you see is effectively
  sampled at the last stop and stays put until the next turn.
- It **resets to zero on `--resume` / `--continue`** — a resumed session is a
  new process, so the counter starts over.

> An earlier version kept its own per-session ledger that tried to subtract
> idle gaps. But the statusline render is sparse and event-driven (Claude
> Code's triggers "go quiet when the session is idle", and stay quiet through
> long autonomous turns), so integrating wall-clock between renders
> *under-counted real sessions by 40–90%*. A direct wall-clock readout has no
> measurement error; the trade-off is that it counts idle time.

When Anthropic hasn't yet shipped `rate_limits` (first few renders of a fresh
session), the bar displays `--%`. A cross-session cache at
`~/.cache/claude-quota-bar/last_stdin.json` restores the most recent values, so
opening a new terminal doesn't blank the bar.

## Configuration

Configured via environment variables:

| Variable | Default | Meaning |
|----------|---------|---------|
| `STATUSLINE_LAYOUT` | `5h,7d,fable,model,session,dir` | Comma-separated segment names (order matters) |
| `STATUSLINE_THRESHOLDS` | `30,70` | Severity flip points (green→yellow→red) as `warn,hot` percentages |
| `STATUSLINE_SID_LEN` | unset (full id) | Characters of the session id the `sid` segment prints |
| `NO_COLOR` | unset | If set, strips all ANSI — falls back to `█`/`░` glyphs |

Severity thresholds (green / yellow / red) flip at 30% and 70% used by
default — for the quota bars and the ctx occupancy percentage alike. Override
with e.g. `STATUSLINE_THRESHOLDS=50,80`; unparseable values silently fall back
to the defaults.

### Session id (`sid`)

Not in the default layout — a full UUID is 36 columns. Add it when you want the
id on screen to copy, instead of digging it out of `/status`:

```sh
STATUSLINE_LAYOUT=5h,7d,model,session,sid,dir
```

It pairs with `claude -r <session_id> --fork-session`, which forks the current
conversation into a new session — a side chat in another pane, or a small
independent task — leaving the original untouched. The id prints bare after a
mute `#` so a double-click selects the UUID alone. Set
`STATUSLINE_SID_LEN=8` if you only want enough to tell sessions apart.

Those 36 columns fit better on a row of their own. The layout token `nl`
breaks the line, and Claude Code renders each output line as its own status
row:

```sh
STATUSLINE_LAYOUT=5h,7d,fable,model,session,nl,dir,sid
```

```
5h[███42%░░░░]✦⏰25m | 7d[███35%░░░░]⏰8d2h | Opus 5(78.0k/1.0M·8%) | ⏳2h15m
claude-quota-bar:feat/sid-segment *5 | #3f9a1c2b-7d4e-4a10-9c33-8b21ef0d55aa
```

That split is 77 / 76 columns where the one-line version was 116 — it fits an
80-column terminal, and it puts the changing numbers on top and the "where am
I" identity below. Moving `dir` down has a second benefit: branch names (up to
25 chars) and the single-dirty-file name (up to 30) are the widest thing on the
line, so with them on row 2 the quota bars stop sliding sideways every time you
switch branches.

`nl` is just a token in the layout, so any segment can move and more than one
break is allowed. A row whose segments all hide produces no blank line — with
`nl,sid` in a session that ships no id, you get one row, not one row and an
empty one.

## Development

Requires Rust ≥ 1.85 (Edition 2024).

```sh
cargo test
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

Releases are fully automated (release-plz Release PR → crates.io + npm + GitHub
Release via OIDC). See [docs/RELEASING.md](docs/RELEASING.md).

## License

MIT — see [LICENSE](./LICENSE).

## Acknowledgements

Visual inspired by [leeguooooo/claude-code-usage-bar](https://github.com/leeguooooo/claude-code-usage-bar).
Release / NPM publishing pattern adapted from [Haleclipse/CCometixLine](https://github.com/Haleclipse/CCometixLine).
