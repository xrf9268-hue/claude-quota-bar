//! Read the last meaningful `usage` block from a Claude Code transcript
//! JSONL and return the model's view of "context tokens used".
//!
//! Claude Code's stdin `context_window.total_input_tokens` under-counts by
//! ~10–15k because it excludes system prompt, tools, and CLAUDE.md
//! (anthropics/claude-code#22955). The transcript carries the true number
//! the API saw — sum `input_tokens + cache_read_input_tokens +
//! cache_creation_input_tokens` from the last non-sidechain, non-error line.

use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Debug, Deserialize, Default)]
struct TranscriptLine {
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
    #[serde(default, rename = "isApiErrorMessage")]
    is_api_error_message: bool,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Debug, Deserialize, Default)]
struct Message {
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

/// Sum the three input-side token fields from the last qualifying transcript
/// line. Returns `None` when the file is absent, empty, no qualifying line
/// is found, or every qualifying line sums to zero — a zero total means
/// "no data captured yet", not "context usage is genuinely zero".
pub fn last_usage_tokens(path: &str) -> Option<u64> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut last: Option<u64> = None;
    // Skip-and-continue on per-line decode errors: a single bad-UTF-8 row
    // shouldn't truncate the scan and silently lose a later valid line.
    // (Written as `let-else` rather than `lines().filter_map(Result::ok)`
    // to make the error-swallowing explicit — clippy lints the latter.)
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(parsed) = serde_json::from_str::<TranscriptLine>(&line) else {
            continue;
        };
        if parsed.is_sidechain || parsed.is_api_error_message {
            continue;
        }
        if let Some(msg) = parsed.message
            && let Some(u) = msg.usage
        {
            // Saturating arithmetic is essentially free here and removes a
            // theoretical panic / wraparound on adversarial input.
            let sum = u
                .input_tokens
                .saturating_add(u.cache_read_input_tokens)
                .saturating_add(u.cache_creation_input_tokens);
            if sum > 0 {
                last = Some(sum);
            }
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_jsonl(lines: &[&str]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(f, "{line}").unwrap();
        }
        f
    }

    #[test]
    fn sums_three_input_fields_from_single_line() {
        let f = write_jsonl(&[
            r#"{"message":{"usage":{"input_tokens":1000,"cache_read_input_tokens":50000,"cache_creation_input_tokens":2000}}}"#,
        ]);
        assert_eq!(last_usage_tokens(f.path().to_str().unwrap()), Some(53_000));
    }

    #[test]
    fn all_zero_usage_returns_none() {
        // A line with zero usage isn't "no context used" — it's "no data
        // captured yet". Treat as None so the renderer falls back to stdin
        // or the baseline rather than displaying a misleading 0.0k.
        let f = write_jsonl(&[
            r#"{"message":{"usage":{"input_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
        ]);
        assert_eq!(last_usage_tokens(f.path().to_str().unwrap()), None);
    }

    #[test]
    fn missing_file_returns_none() {
        assert_eq!(
            last_usage_tokens("/tmp/__nope__/does_not_exist.jsonl"),
            None
        );
    }

    #[test]
    fn empty_path_returns_none() {
        assert_eq!(last_usage_tokens(""), None);
    }

    #[test]
    fn empty_file_returns_none() {
        let f = write_jsonl(&[]);
        assert_eq!(last_usage_tokens(f.path().to_str().unwrap()), None);
    }

    #[test]
    fn invalid_utf8_line_does_not_truncate_scan() {
        // A mid-file UTF-8 decode error must not stop us reading the rest
        // of the transcript — we want the *last* qualifying record, not
        // "the last before the first IO error".
        let mut f = NamedTempFile::new().unwrap();
        let valid_first = br#"{"message":{"usage":{"input_tokens":50,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
        let valid_last = br#"{"message":{"usage":{"input_tokens":700,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
        f.write_all(valid_first).unwrap();
        f.write_all(b"\n").unwrap();
        f.write_all(&[0xff, 0xfe, 0x80]).unwrap();
        f.write_all(b"\n").unwrap();
        f.write_all(valid_last).unwrap();
        f.write_all(b"\n").unwrap();
        assert_eq!(last_usage_tokens(f.path().to_str().unwrap()), Some(700));
    }

    #[test]
    fn malformed_line_is_skipped_others_still_work() {
        let f = write_jsonl(&[
            r#"{"message":{"usage":{"input_tokens":50,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
            r#"not json at all"#,
        ]);
        assert_eq!(last_usage_tokens(f.path().to_str().unwrap()), Some(50));
    }

    #[test]
    fn line_without_usage_is_skipped() {
        let f = write_jsonl(&[
            r#"{"message":{"usage":{"input_tokens":42,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
            r#"{"message":{"role":"user"}}"#,
        ]);
        assert_eq!(last_usage_tokens(f.path().to_str().unwrap()), Some(42));
    }

    #[test]
    fn skips_api_error_message_lines() {
        let f = write_jsonl(&[
            r#"{"message":{"usage":{"input_tokens":200,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
            r#"{"isApiErrorMessage":true,"message":{"usage":{"input_tokens":7777,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
        ]);
        assert_eq!(last_usage_tokens(f.path().to_str().unwrap()), Some(200));
    }

    #[test]
    fn skips_sidechain_lines() {
        let f = write_jsonl(&[
            r#"{"message":{"usage":{"input_tokens":100,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
            r#"{"isSidechain":true,"message":{"usage":{"input_tokens":999999,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
        ]);
        assert_eq!(last_usage_tokens(f.path().to_str().unwrap()), Some(100));
    }

    #[test]
    fn returns_usage_from_last_qualifying_line() {
        let f = write_jsonl(&[
            r#"{"message":{"usage":{"input_tokens":100,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
            r#"{"message":{"usage":{"input_tokens":500,"cache_read_input_tokens":1000,"cache_creation_input_tokens":2000}}}"#,
        ]);
        assert_eq!(last_usage_tokens(f.path().to_str().unwrap()), Some(3_500));
    }
}
