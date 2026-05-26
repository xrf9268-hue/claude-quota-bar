# Changelog

All notable changes to this project will be documented in this file.

## [0.3.1](https://github.com/xrf9268-hue/claude-quota-bar/compare/v0.3.0...v0.3.1) - 2026-05-26

### Other

- automate releases with release-plz + crates.io/npm trusted publishing ([#6](https://github.com/xrf9268-hue/claude-quota-bar/pull/6))

## [0.3.0] - 2026-05-26

Runtime analysis of v0.2.0 found six gaps between tested units and shipped
behavior; a follow-up Codex review caught a panic in the new code. All fixed.

### Features

- The `cache` segment now renders. It shipped in the default layout and the
  README but `main.rs` hardcoded it off, so it never appeared. It now shows the
  prompt cache's remaining time (`4m12s`) or `COLD` once expired, anchored to the
  last transcript turn's timestamp. TTL defaults to 3600s (Claude.ai Pro/Max
  accounts get the 1-hour cache automatically and the active TTL isn't exposed),
  overridable via the new `STATUSLINE_CACHE_TTL` environment variable.

### Bug Fixes

- Git segment no longer vanishes when Claude Code runs from a repository
  subdirectory — work-tree detection now walks up the ancestor chain for `.git`
  instead of only matching the repo root.
- `--version` / `--help` now print version and usage instead of blocking on
  stdin and rendering a stale-cache status line.
- The global cache is persisted only when stdin carries a `session_id`, so a
  hand-run invocation or unrelated script can't poison the bar every terminal
  reads.
- Absurd `resets_at` countdowns past 90 days now render `--` rather than nonsense
  like `95141d14h`.
- An unreadable or future cache mtime (clock skew) is treated as stale instead of
  trusting a possibly-expired snapshot.
- The ISO-8601 timestamp parser reads bytes instead of `str` slices, so a
  multibyte character at a fixed offset can no longer panic the statusline (Codex
  review finding).

### CI

- Bump actions to Node 24 versions
- Track GitHub Actions versions with Dependabot
- Also track Cargo deps with Dependabot
- Bump the actions group with 2 updates

## [0.2.0] - 2026-05-21

### Bug Fixes

- Split rename destination on FIRST arrow, not last
- Skip C-quoted source before splitting rename row
- Cap per-line read to bound foreground allocation
- Treat try_wait errors as transient in run_with_timeout
- Decode C-style path escapes + check rename status in both columns
- Sanitize control chars in dirty filename — ANSI injection guard
- Keep line-aligned first record in tail-window scan

### CI

- Suppress DEP0040 punycode warning from rust-cache action

### Chores

- Clean up npm bootstrap workflow + support NPM_OTP (#2)
- V0.2.0

### Documentation

- Add essay on publishing a Rust CLI to NPM via Trusted Publishing
- Tighten Rust CLI essay after independent review pass (#1)
- Update AGENTS.md for transcript module + token precedence
- Tighten AGENTS.md — drop test count, superpowers ref, compress #5
- Promote dual-fg rule from code style to critical invariant

### Features

- Transcript-based context tokens, half-glyph bar, single dirty filename
- WCAG-grade bar contrast + Anthropic-aligned palette

### Performance

- Tail-read 64 KB instead of full-scan per render

## [0.1.0] - 2026-05-18

### CI

- Switch NPM publish to Trusted Publishing (OIDC)
- Bump actions/checkout|upload-artifact|download-artifact|setup-node to v5
- Cross-compile x86_64-apple-darwin from macos-14
- Use Node 24 (bundled npm 11+) instead of upgrading npm in-place

### Documentation

- Add AGENTS.md for AI coding agents (agents.md spec)

### Features

- Initial claude-quota-bar project (v0.1.0)


