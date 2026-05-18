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

    for line in lines {
        if !line.trim().is_empty() {
            info.dirty_count += 1;
        }
    }
    Some(info)
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
