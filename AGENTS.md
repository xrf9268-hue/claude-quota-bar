# AGENTS.md

> Guidance for AI coding agents (Claude Code, Codex, Cursor, Cline, …) working on
> this repository. Conforms to [agents.md](https://agents.md/).

## Project at a glance

Rust statusline binary for [Claude Code](https://github.com/anthropics/claude-code).
Reads a JSON payload on stdin once per render, prints a colored line on stdout,
exits. Single-purpose: surface 5h/7d rate-limit quota, context-window usage,
and `dir:branch *N`.

We target the latest Claude Code only (≥ 2.1.132, where stdin's
`context_window.total_input_tokens` means current context occupancy). No
backward-compat shims for older stdin semantics.

- Edition 2024, MSRV 1.85.
- 2 runtime deps: `serde`, `serde_json`. Do not add more without a written
  justification — the binary is currently 460KB stripped and we want to
  keep it there.
- Library + binary split. `main.rs` does all I/O; `render.rs` is a pure
  function over a `Context`. Tests stay fast because of this.

## Build / test / lint

```sh
cargo build --release          # ~460KB binary at target/release/claude-quota-bar
cargo test                     # lib + integration
cargo fmt --all -- --check     # rustfmt clean
cargo clippy --all-targets -- -D warnings
```

CI runs all four on every push and PR via `.github/workflows/ci.yml`. PRs must
be clean on all four before merge.

## Iron Law: TDD

This codebase was built test-first. **Adhere to red-green-refactor for every
new function or behavior change:**

1. Write a failing test.
2. Run `cargo test` — watch it fail for the expected reason.
3. Write the minimum code to make it pass.
4. Run again — green.
5. Refactor with the green tests as a safety net.

Don't write production code without a failing test first.

## Code style

- One short doc-line per module at the top; explain *why* the file exists.
- Don't comment what the code does — comment *why* it surprises a reader. If a
  function's behavior would surprise someone reading it cold, that's the
  comment-worthy moment.
- No emojis in source.
- ANSI: use `crate::ansi::{fg, bg, reset}`. Respect `NO_COLOR` — emit empty
  strings, not escapes, when color is off.
- Battery bar (`progress::battery_bar`) batches consecutive cells of the same
  background. Don't regress that — per-cell escapes blow up the output 10×.
- Severity thresholds (`theme::WARN_THRESHOLD = 30.0`, `HOT_THRESHOLD = 70.0`)
  are global. Don't add a config knob without a real user need.

## Critical invariants

These are the load-bearing decisions that aren't obvious from reading code:

1. **`cache::maybe_save` runs BEFORE `cache::fill_from_cache`** in `main.rs`.
   Reversing them lets a partial stdin payload silently overwrite a good cache
   (the hydrated rate_limits trick `maybe_save` into thinking the partial raw
   is "has data"). See the commit message of the cache fix for the full failure
   mode.

2. **`cache::rollover` returns `None` for expired windows.** Don't "helpfully"
   synthesize a 0% reset value — that's a lie. The renderer shows `--%` when
   data is unknown; that's the honest UX.

3. **`fill_from_cache` only hydrates `rate_limits`**, not `context_window`.
   The cache is global per machine; context state belongs to a specific
   session and would render stale token counts across sessions.

4. **Statusline must not crash the parent.** There is deliberately no
   `?`-propagation in `main` — every fallible step degrades in place
   (invalid stdin → defaults, cache I/O errors ignored). Claude Code reads
   stdout — a crash shows up as a blank statusline (annoying) instead of a
   panic (broken).

5. **Bar text must use two fg colors.** `text_on_filled` (dark) overlaid on
   the light severity cells, `text_on_empty` (light) overlaid on the dark
   gutter. Collapsing to a single fg fails WCAG contrast on one of the two
   backgrounds and renders the percentage text near-invisible (the prior
   warn-state ratio was 1.6 : 1). The regression is caught by
   `partial_bar_uses_dark_text_on_filled_and_light_text_on_empty` in
   `progress.rs`.

6. **Context-token source** in `render::build_model`: stdin
   `total_input_tokens` → 20k baseline. Since Claude Code 2.1.132 stdin's
   `total_input_tokens` is the full current-context sum (`input +
   cache_creation + cache_read` of the most recent API response) — the old
   transcript-JSONL scan that compensated for stdin under-counting
   (anthropics/claude-code#22955) was removed along with the prompt-cache
   segment. Don't add output tokens to the sum — the last assistant
   message's input already includes the prior output. A zero stdin sum
   means "no API turn yet": render the `~20k` baseline, never a bare 0.
   A sum larger than the window itself is the pre-2.1.132 cumulative
   total leaking through an old Claude Code: render `--`, never a
   confident wrong number.

7. **A layout with no recognized segment falls back to `DEFAULT_LAYOUT`**
   (`render::render`). A stale `STATUSLINE_LAYOUT=cache` from before that
   segment was removed must not blank the statusline on every prompt; a
   recognized segment that legitimately hides (`dir` with empty cwd) still
   renders empty.

## Release process

Tag-driven, fully automated via `.github/workflows/release.yml`:

```sh
git tag vX.Y.Z
git push --tags
```

Triggers:
- 7-platform matrix build (macOS arm64/x64, Linux x64/arm64 glibc + musl, Win x64).
- GitHub Release with multi-platform tarballs + git-cliff-generated notes.
- NPM publish via OIDC Trusted Publishing — **no `NPM_TOKEN` secret stored**.

Trusted Publishing is already configured on all 8 npm packages (umbrella +
7 platforms). If a new platform target is added, you must:
1. Add it to `release.yml` matrix + `npm/platforms/`.
2. Bootstrap-publish a `0.0.0-bootstrap` placeholder so the npm package exists.
3. Configure Trusted Publisher in npm UI pointing at this repo + `release.yml`.

See `docs/npm-trusted-publishing.md` for the full one-time setup history.

## Out of scope (won't accept PRs)

We intentionally don't:
- Add a TUI configurator. Env vars are enough.
- Add a daemon mode. Cold start is 2.5ms; not worth the complexity.
- Add `--patch` to auto-edit `~/.claude/settings.json`.
- Add multiple visual "styles". One good design only.
- Add vim mode, cost, lines, API perf segments. Those were why the predecessor
  felt cluttered.
- Re-add the prompt-cache (`cache 4m12s`/`COLD`) segment. The TTL isn't
  exposed to the statusline so the countdown was a guess, and it froze
  exactly when it mattered (idle periods, unless `refreshInterval` is set).
- Support pre-2.1.132 Claude Code stdin semantics.

If you think one of these has changed, open an issue first.

## Repository map

```
src/
├── main.rs        # binary entry; reads stdin, calls render, prints stdout
├── lib.rs         # module declarations (re-exports for tests)
├── input.rs       # serde structs for Claude Code stdin payload
├── theme.rs       # color palette + severity thresholds
├── ansi.rs        # NO_COLOR-aware ANSI helpers + strip/width utils
├── progress.rs    # battery_bar — percentage text inside a colored bar
├── time_fmt.rs    # countdown formatting, token compaction (1.2k / 1.5M)
├── render.rs      # pure composition; takes Context, returns String
├── git.rs         # shells out to git with 1s timeout + 5s cache
└── cache.rs       # ~/.cache/claude-quota-bar/last_stdin.json hydration
tests/
└── integration.rs # assert_cmd snapshot tests of the full binary
npm/               # NPM umbrella + 7 platform packages for distribution
docs/              # one-off setup notes (NPM TP, etc.)
.github/workflows/ # CI + release pipeline
```

## When stuck

Read these in order:
1. The module's own top-line `//!` doc.
2. The relevant test in `#[cfg(test)] mod tests`.
3. Git log for the surrounding code (`git log -p -S "search term"`).
4. `docs/` for operational concerns.

Prefer reading the existing test before asking what a function does.
