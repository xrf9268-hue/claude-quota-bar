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
use std::io::{BufRead, BufReader, Read};

/// Per-line read cap. Tool-result payloads can carry multi-MB base64 blobs
/// (screenshots, attachments); without a cap, `BufRead::read_line` grows
/// the line buffer to the full size and stalls the statusline render with
/// a multi-MB synchronous allocation. 1 MB is far above every realistic
/// `usage` line and still bounds the worst case.
const MAX_LINE_BYTES: u64 = 1 << 20;

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
    let mut reader = BufReader::new(file);
    let mut last: Option<u64> = None;
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        // `take(MAX + 1)` caps the per-line read; if the line is larger,
        // `read_until` stops short of `\n` and we discard the rest in
        // bounded chunks. This protects the statusline from multi-MB
        // tool-result payloads (e.g. base64 screenshots).
        let n = {
            let mut limited = reader.by_ref().take(MAX_LINE_BYTES + 1);
            limited.read_until(b'\n', &mut buf).unwrap_or(0)
        };
        if n == 0 {
            break;
        }
        let complete = buf.last() == Some(&b'\n');
        if !complete {
            // Oversized line — drain to next `\n` (or EOF) without
            // accumulating, then skip this record.
            skip_to_newline(&mut reader);
            continue;
        }
        buf.pop(); // strip the `\n`
        // Skip-and-continue on per-line decode errors: a single bad-UTF-8
        // row shouldn't truncate the scan and silently lose a later valid
        // line. Written explicitly with `let-else` rather than
        // `lines().filter_map(Result::ok)` so the error-swallowing is
        // visible at the call site (clippy lints the latter).
        let Ok(line) = std::str::from_utf8(&buf) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<TranscriptLine>(line) else {
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

/// Advance the reader past the next `\n` (or EOF) without buffering the
/// skipped bytes. Used to discard the tail of an oversized line.
fn skip_to_newline(reader: &mut impl BufRead) {
    let mut scratch: Vec<u8> = Vec::with_capacity(4096);
    loop {
        scratch.clear();
        let n = {
            let mut chunk = reader.by_ref().take(4096);
            chunk.read_until(b'\n', &mut scratch).unwrap_or(0)
        };
        if n == 0 || scratch.last() == Some(&b'\n') {
            break;
        }
    }
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
    fn oversized_line_is_skipped_without_blocking_later_lines() {
        // A single JSONL line carrying a large base64 tool_result blob can
        // be tens of MB on the wire. We refuse to allocate it into RAM —
        // skip past it and keep scanning. The post-oversize valid line
        // must still be picked up.
        let mut f = NamedTempFile::new().unwrap();
        let valid_first = br#"{"message":{"usage":{"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
        f.write_all(valid_first).unwrap();
        f.write_all(b"\n").unwrap();
        // Build an oversized but well-formed JSON line.
        f.write_all(br#"{"junk":""#).unwrap();
        let chunk = vec![b'a'; 64 * 1024];
        for _ in 0..40 {
            f.write_all(&chunk).unwrap(); // ~2.5 MB blob — exceeds the 1 MB cap.
        }
        f.write_all(br#""}"#).unwrap();
        f.write_all(b"\n").unwrap();
        let valid_last = br#"{"message":{"usage":{"input_tokens":777,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
        f.write_all(valid_last).unwrap();
        f.write_all(b"\n").unwrap();
        assert_eq!(last_usage_tokens(f.path().to_str().unwrap()), Some(777));
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
