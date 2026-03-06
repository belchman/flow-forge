//! Error recovery intelligence types.
//! Tracks error patterns across sessions and remembers what fixed them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A normalized error pattern with occurrence tracking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorFingerprint {
    pub id: String,
    /// SHA-256 of the normalized error text.
    pub fingerprint: String,
    pub category: ErrorCategory,
    pub tool_name: Option<String>,
    /// First ~200 chars of the original error for display.
    pub error_preview: String,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub occurrence_count: u32,
}

/// A recorded resolution for an error pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResolution {
    pub id: String,
    pub fingerprint_id: String,
    /// Human-readable summary of the fix.
    pub resolution_summary: String,
    /// Tool names used in the fix sequence (JSON array in DB).
    pub tool_sequence: Vec<String>,
    /// Files that were modified to resolve the error (JSON array in DB).
    pub files_changed: Vec<String>,
    pub success_count: u32,
    pub failure_count: u32,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
}

impl ErrorResolution {
    /// Confidence that this resolution works (success / total).
    pub fn confidence(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            return 0.5;
        }
        self.success_count as f64 / total as f64
    }
}

/// Category of error for filtering and display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Compile,
    Test,
    Runtime,
    Lint,
    Permission,
    Network,
    Unknown,
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compile => write!(f, "compile"),
            Self::Test => write!(f, "test"),
            Self::Runtime => write!(f, "runtime"),
            Self::Lint => write!(f, "lint"),
            Self::Permission => write!(f, "permission"),
            Self::Network => write!(f, "network"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl std::str::FromStr for ErrorCategory {
    type Err = ();
    fn from_str(s: &str) -> std::result::Result<Self, ()> {
        match s {
            "compile" => Ok(Self::Compile),
            "test" => Ok(Self::Test),
            "runtime" => Ok(Self::Runtime),
            "lint" => Ok(Self::Lint),
            "permission" => Ok(Self::Permission),
            "network" => Ok(Self::Network),
            _ => Ok(Self::Unknown),
        }
    }
}

/// Normalize an error message for fingerprinting.
/// Strips volatile content (paths, line numbers, timestamps, ANSI codes)
/// so structurally identical errors get the same fingerprint.
pub fn normalize_error(error: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    static ANSI: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());
    static ABS_PATH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(/[a-zA-Z0-9._\-]+){3,}").unwrap());
    static LINE_COL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r":\d+:\d+").unwrap());
    static TIMESTAMP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}").unwrap());
    static HEX_HASH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b[0-9a-f]{8,}\b").unwrap());
    static WHITESPACE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\s+").unwrap());

    let s = ANSI.replace_all(error, "");
    let s = ABS_PATH.replace_all(&s, "<PATH>");
    let s = TIMESTAMP.replace_all(&s, "<TS>");
    let s = LINE_COL.replace_all(&s, ":<LINE>");
    let s = HEX_HASH.replace_all(&s, "<HASH>");
    let s = WHITESPACE.replace_all(&s, " ");
    s.trim().to_lowercase()
}

/// Compute SHA-256 fingerprint of normalized error text.
pub fn fingerprint_error(error: &str) -> String {
    use sha2::{Digest, Sha256};
    let normalized = normalize_error(error);
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

/// Classify an error into a category based on content heuristics.
pub fn classify_error(error: &str, tool_name: &str) -> ErrorCategory {
    let lower = error.to_lowercase();

    // Tool-based classification
    if tool_name == "Bash" {
        if lower.contains("cargo build") || lower.contains("cargo check")
            || lower.contains("error[e") || lower.contains("cannot find")
            || lower.contains("expected") || lower.contains("mismatched types")
            || lower.contains("unresolved import") || lower.contains("no such")
        {
            return ErrorCategory::Compile;
        }
        if lower.contains("cargo test") || lower.contains("test result: failed")
            || lower.contains("assertion") || lower.contains("thread '") && lower.contains("panicked")
        {
            return ErrorCategory::Test;
        }
        if lower.contains("cargo clippy") || lower.contains("warning:") {
            return ErrorCategory::Lint;
        }
        if lower.contains("permission denied") || lower.contains("operation not permitted") {
            return ErrorCategory::Permission;
        }
        if lower.contains("connection refused") || lower.contains("network")
            || lower.contains("timeout") || lower.contains("dns")
        {
            return ErrorCategory::Network;
        }
    }

    // Content-based fallback
    if lower.contains("error[e") || lower.contains("cannot find") {
        ErrorCategory::Compile
    } else if lower.contains("assertion") || lower.contains("test failed") {
        ErrorCategory::Test
    } else if lower.contains("permission denied") {
        ErrorCategory::Permission
    } else {
        ErrorCategory::Unknown
    }
}

/// Context about the previous session for continuity injection.
#[derive(Debug, Clone, Serialize)]
pub struct PreviousSessionContext {
    pub session_id: String,
    pub task_description: Option<String>,
    pub verdict: Option<String>,
    pub files_modified: Vec<String>,
    pub edits_count: u64,
    pub commands_count: u64,
    pub duration_minutes: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_strips_paths() {
        let error = "error: cannot find `/Users/matt/Projects/foo/src/main.rs`";
        let normalized = normalize_error(error);
        assert!(!normalized.contains("/Users/matt"));
        assert!(normalized.contains("<path>"));
    }

    #[test]
    fn test_normalize_strips_line_numbers() {
        let error = "error at src/main.rs:42:10: expected semicolon";
        let normalized = normalize_error(error);
        assert!(!normalized.contains(":42:10"));
        assert!(normalized.contains(":<line>"));
    }

    #[test]
    fn test_normalize_strips_timestamps() {
        let error = "2026-03-05T12:30:45 ERROR: something failed";
        let normalized = normalize_error(error);
        assert!(!normalized.contains("2026"));
        assert!(normalized.contains("<ts>"));
    }

    #[test]
    fn test_normalize_strips_ansi() {
        let error = "\x1b[31merror\x1b[0m: something broke";
        let normalized = normalize_error(error);
        assert!(!normalized.contains("\x1b"));
    }

    #[test]
    fn test_fingerprint_same_error_same_hash() {
        let e1 = "error at /foo/bar/baz.rs:10:5: cannot find value `x`";
        let e2 = "error at /other/path/baz.rs:20:3: cannot find value `x`";
        assert_eq!(fingerprint_error(e1), fingerprint_error(e2));
    }

    #[test]
    fn test_fingerprint_different_errors_different_hash() {
        let e1 = "cannot find value `x` in this scope";
        let e2 = "mismatched types: expected u32, found String";
        assert_ne!(fingerprint_error(e1), fingerprint_error(e2));
    }

    #[test]
    fn test_classify_compile_error() {
        assert_eq!(
            classify_error("error[E0425]: cannot find value `foo`", "Bash"),
            ErrorCategory::Compile
        );
    }

    #[test]
    fn test_classify_test_error() {
        assert_eq!(
            classify_error("test result: FAILED. 1 passed; 2 failed", "Bash"),
            ErrorCategory::Test
        );
    }

    #[test]
    fn test_classify_permission_error() {
        assert_eq!(
            classify_error("permission denied: /etc/shadow", "Bash"),
            ErrorCategory::Permission
        );
    }

    #[test]
    fn test_classify_lint_error() {
        assert_eq!(
            classify_error("warning: unused variable `x` in cargo clippy output", "Bash"),
            ErrorCategory::Lint
        );
    }

    #[test]
    fn test_error_resolution_confidence() {
        let r = ErrorResolution {
            id: "r1".into(),
            fingerprint_id: "f1".into(),
            resolution_summary: "fix".into(),
            tool_sequence: vec![],
            files_changed: vec![],
            success_count: 8,
            failure_count: 2,
            created_at: Utc::now(),
            last_used: Utc::now(),
        };
        assert!((r.confidence() - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_error_resolution_confidence_zero_total() {
        let r = ErrorResolution {
            id: "r1".into(),
            fingerprint_id: "f1".into(),
            resolution_summary: "fix".into(),
            tool_sequence: vec![],
            files_changed: vec![],
            success_count: 0,
            failure_count: 0,
            created_at: Utc::now(),
            last_used: Utc::now(),
        };
        assert!((r.confidence() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_normalize_strips_hex_hashes() {
        let error = "error: commit abcdef0123456789 not found in tree deadbeef01";
        let normalized = normalize_error(error);
        assert!(!normalized.contains("abcdef0123456789"));
        assert!(!normalized.contains("deadbeef01"));
        assert!(normalized.contains("<hash>"));
    }

    #[test]
    fn test_normalize_idempotent() {
        let error = "error at /Users/matt/project/src/main.rs:42:10: cannot find 2026-03-05T12:00:00 abcdef0123456789";
        let once = normalize_error(error);
        let twice = normalize_error(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn test_fingerprint_ignores_whitespace() {
        let e1 = "cannot   find   value  `x`  in  this  scope";
        let e2 = "cannot find value `x` in this scope";
        assert_eq!(fingerprint_error(e1), fingerprint_error(e2));
    }

    #[test]
    fn test_error_category_roundtrip() {
        for cat in [
            ErrorCategory::Compile,
            ErrorCategory::Test,
            ErrorCategory::Runtime,
            ErrorCategory::Lint,
            ErrorCategory::Permission,
            ErrorCategory::Network,
            ErrorCategory::Unknown,
        ] {
            let s = cat.to_string();
            let parsed: ErrorCategory = s.parse().unwrap();
            assert_eq!(parsed, cat);
        }
    }
}
