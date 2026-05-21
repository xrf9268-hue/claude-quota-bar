//! Git status — shells out to `git` with a tight timeout and 5s cache.
//!
//! The statusline renders at ~1Hz; spawning a process every tick is wasteful.
//! We cache the result keyed by cwd hash under `/tmp` and short-circuit
//! repeat lookups within the TTL.

use serde::{Deserialize, Serialize};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_TTL_SECS: u64 = 5;
const GIT_TIMEOUT_MS: u64 = 1000;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    pub branch: String,
    pub detached: bool,
    pub dirty_count: u32,
    /// Populated only when `dirty_count == 1`. Carries the single changed
    /// path so the dir segment can render the filename instead of "*1".
    pub dirty_file: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    ts: u64,
    cwd: String,
    info: Option<GitInfo>,
}

/// Look up git state for `cwd`. Returns None when the directory isn't a
/// git repo or git itself isn't installed / times out.
pub fn status(cwd: &str) -> Option<GitInfo> {
    if cwd.is_empty() {
        return None;
    }
    let cache_path = cache_path_for(cwd);
    if let Some(info) = read_cache(&cache_path, cwd) {
        return info;
    }
    let info = run_git(cwd);
    write_cache(&cache_path, cwd, &info);
    info
}

fn cache_path_for(cwd: &str) -> PathBuf {
    let mut h = DefaultHasher::new();
    cwd.hash(&mut h);
    let tmp = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    tmp.join(format!("claude-quota-bar-git-{:x}.json", h.finish()))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cache(path: &Path, cwd: &str) -> Option<Option<GitInfo>> {
    let raw = std::fs::read_to_string(path).ok()?;
    let entry: CacheEntry = serde_json::from_str(&raw).ok()?;
    if entry.cwd != cwd {
        return None;
    }
    if now().saturating_sub(entry.ts) > CACHE_TTL_SECS {
        return None;
    }
    Some(entry.info)
}

fn write_cache(path: &Path, cwd: &str, info: &Option<GitInfo>) {
    let entry = CacheEntry {
        ts: now(),
        cwd: cwd.to_string(),
        info: info.clone(),
    };
    if let Ok(json) = serde_json::to_string(&entry) {
        let _ = std::fs::write(path, json);
    }
}

fn run_git(cwd: &str) -> Option<GitInfo> {
    // Quick check: is this a git repo at all? Avoids spawning the heavier
    // `status` call when we'll get an error anyway.
    if !Path::new(cwd).join(".git").exists() {
        return None;
    }

    let output = run_with_timeout(
        Command::new("git")
            .args([
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--branch",
            ])
            .current_dir(cwd),
        GIT_TIMEOUT_MS,
    )?;

    if !output.status.success() {
        return None;
    }
    parse_status(&String::from_utf8_lossy(&output.stdout))
}

fn parse_status(out: &str) -> Option<GitInfo> {
    let mut info = GitInfo::default();
    let mut lines = out.lines();
    let header = lines.next()?;
    if !header.starts_with("## ") {
        return None;
    }
    let body = &header[3..];

    if let Some(rest) = body.strip_prefix("HEAD (no branch)") {
        info.detached = true;
        info.branch = "HEAD".to_string();
        let _ = rest;
    } else {
        // Branch name ends at "..." (upstream divider) or space ([ahead...]).
        // Strip the "No commits yet on " prefix git prepends in a brand-new
        // repo without commits, so the user sees just the branch name.
        let stripped = body.strip_prefix("No commits yet on ").unwrap_or(body);
        let end = stripped.find("...").unwrap_or(stripped.len());
        info.branch = stripped[..end].to_string();

        if let Some(brackets_start) = body.find('[') {
            if let Some(brackets_end) = body[brackets_start..].find(']') {
                let inside = &body[brackets_start + 1..brackets_start + brackets_end];
                for part in inside.split(',') {
                    let part = part.trim();
                    if let Some(n) = part.strip_prefix("ahead ") {
                        info.ahead = n.parse().unwrap_or(0);
                    } else if let Some(n) = part.strip_prefix("behind ") {
                        info.behind = n.parse().unwrap_or(0);
                    }
                }
            }
        }
    }

    let mut first_line: Option<&str> = None;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        if info.dirty_count == 0 {
            first_line = Some(line);
        }
        info.dirty_count += 1;
    }
    if info.dirty_count == 1
        && let Some(l) = first_line
    {
        info.dirty_file = extract_dirty_path(l);
    }
    Some(info)
}

/// Extract the displayable path from a porcelain v1 row (`XY path`).
///
/// - For rename (`R`) and copy (`C`) status codes in the index slot, the
///   payload is `old -> new`; the separator is the FIRST ` -> ` on the
///   row (a destination filename may itself contain ` -> `). We surface
///   the destination, which is everything after that first separator.
/// - For all other codes a literal ` -> ` is part of the filename and must
///   be preserved.
/// - Git wraps paths containing unusual characters in C-style double
///   quotes; strip the outer pair so the rendered name is the actual path.
fn extract_dirty_path(line: &str) -> Option<String> {
    // The first three bytes are always ASCII (status chars + space), so
    // direct byte indexing is safe. The path tail may be UTF-8.
    let bytes = line.as_bytes();
    if bytes.len() < 3 {
        return None;
    }
    let x = bytes[0] as char;
    let raw = line.get(3..)?;
    let path = if matches!(x, 'R' | 'C') {
        raw.split_once(" -> ").map(|(_, dest)| dest).unwrap_or(raw)
    } else {
        raw
    };
    let cleaned = path
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(path);
    Some(cleaned.to_string())
}

/// Run a command with a wall-clock timeout. On timeout we kill the child
/// and return None; the renderer treats that as "no git info".
fn run_with_timeout(cmd: &mut Command, timeout_ms: u64) -> Option<std::process::Output> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = SystemTime::now();
    loop {
        match child.try_wait().ok()? {
            Some(_status) => {
                return child.wait_with_output().ok();
            }
            None => {
                let elapsed = start
                    .elapsed()
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(timeout_ms);
                if elapsed >= timeout_ms {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_branch() {
        let s = "## main...origin/main\n M src/foo.rs\n?? new.txt\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.branch, "main");
        assert_eq!(info.dirty_count, 2);
        assert_eq!(info.ahead, 0);
        assert_eq!(info.behind, 0);
    }

    #[test]
    fn parse_captures_single_dirty_filename() {
        let s = "## main...origin/main\n M src/foo.rs\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.dirty_count, 1);
        assert_eq!(info.dirty_file.as_deref(), Some("src/foo.rs"));
    }

    #[test]
    fn parse_strips_outer_quotes_from_path() {
        // Porcelain v1 wraps paths containing non-printable / non-ASCII
        // chars in C-style double quotes. Strip them so the display name
        // doesn't carry the framing.
        let s = "## main...origin/main\n M \"weird path.rs\"\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.dirty_count, 1);
        assert_eq!(info.dirty_file.as_deref(), Some("weird path.rs"));
    }

    #[test]
    fn parse_does_not_split_arrow_for_non_rename_status() {
        // A modified (not renamed) file whose name literally contains ` -> `
        // must keep the full path. Only R / C status rows use the arrow as
        // the old/new separator.
        let s = "## main...origin/main\n M weird -> name.txt\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.dirty_count, 1);
        assert_eq!(info.dirty_file.as_deref(), Some("weird -> name.txt"));
    }

    #[test]
    fn parse_rename_destination_with_arrow_in_name() {
        // Git's porcelain v1 separator is the FIRST ` -> ` on a rename row.
        // A destination filename can itself contain ` -> ` (it's a legal
        // ASCII filename and git only quotes paths with truly unprintable
        // chars). A greedy `rsplit` truncates the destination wrongly.
        let s = "## main...origin/main\nR  old.txt -> new -> name.txt\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.dirty_count, 1);
        assert_eq!(info.dirty_file.as_deref(), Some("new -> name.txt"));
    }

    #[test]
    fn parse_rename_quoted_destination_with_arrow() {
        // Codex-flagged variant: when the destination is also C-quoted,
        // the right outer quote must still be stripped after taking the
        // (full) destination.
        let s = "## main...origin/main\nR  \"old.txt\" -> \"new -> name.txt\"\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.dirty_count, 1);
        assert_eq!(info.dirty_file.as_deref(), Some("new -> name.txt"));
    }

    #[test]
    fn parse_copy_status_takes_destination() {
        // C (copied) uses the same `old -> new` syntax as R.
        let s = "## main...origin/main\nC  src/a.rs -> src/b.rs\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.dirty_count, 1);
        assert_eq!(info.dirty_file.as_deref(), Some("src/b.rs"));
    }

    #[test]
    fn parse_rename_captures_new_name() {
        let s = "## main...origin/main\nR  src/old.rs -> src/new.rs\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.dirty_count, 1);
        assert_eq!(info.dirty_file.as_deref(), Some("src/new.rs"));
    }

    #[test]
    fn parse_untracked_captures_filename() {
        let s = "## main...origin/main\n?? new_file.txt\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.dirty_count, 1);
        assert_eq!(info.dirty_file.as_deref(), Some("new_file.txt"));
    }

    #[test]
    fn parse_skips_dirty_filename_when_multiple_files() {
        let s = "## main...origin/main\n M src/foo.rs\n?? new.txt\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.dirty_count, 2);
        assert!(info.dirty_file.is_none());
    }

    #[test]
    fn parse_ahead_behind() {
        let s = "## feat/x...origin/feat/x [ahead 3, behind 1]\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.branch, "feat/x");
        assert_eq!(info.ahead, 3);
        assert_eq!(info.behind, 1);
    }

    #[test]
    fn parse_detached() {
        let s = "## HEAD (no branch)\n";
        let info = parse_status(s).unwrap();
        assert!(info.detached);
    }

    #[test]
    fn parse_clean_no_upstream() {
        let s = "## main\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.branch, "main");
        assert_eq!(info.dirty_count, 0);
    }

    #[test]
    fn parse_new_repo_no_commits() {
        // `git status --porcelain --branch` on a brand-new repo with no
        // commits yet prefixes the branch with "No commits yet on ".
        let s = "## No commits yet on main\n?? Cargo.toml\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.branch, "main");
        assert_eq!(info.dirty_count, 1);
    }

    #[test]
    fn parse_new_repo_custom_branch() {
        let s = "## No commits yet on feature/init\n";
        let info = parse_status(s).unwrap();
        assert_eq!(info.branch, "feature/init");
    }
}
