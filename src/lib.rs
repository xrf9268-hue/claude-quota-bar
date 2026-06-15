//! claude-quota-bar — fast Claude Code statusline.
//!
//! Library surface exists so integration tests can call into render paths
//! without spawning the binary; the binary itself is a thin shell in
//! `main.rs` that does stdin/file IO and delegates to these modules.

pub mod ansi;
pub mod cache;
pub mod git;
pub mod input;
pub mod progress;
pub mod render;
pub mod theme;
pub mod time_fmt;

#[cfg(test)]
pub(crate) mod test_env {
    use std::sync::Mutex;
    use tempfile::TempDir;

    // HOME is process-global state: every test that swaps it must
    // serialize through this ONE lock, no matter which module it lives in.
    // Per-module locks (which cache.rs and the former session ledger each
    // briefly had) still race each other under the parallel test runner and
    // fail intermittently.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    pub fn with_temp_home<F: FnOnce(&TempDir)>(f: F) {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = TempDir::new().unwrap();
        unsafe { std::env::set_var("HOME", home.path()) };
        f(&home);
        unsafe { std::env::remove_var("HOME") };
    }
}
