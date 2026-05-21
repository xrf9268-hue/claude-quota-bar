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
pub mod transcript;
