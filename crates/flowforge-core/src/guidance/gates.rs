//! Individual gate check functions for the guidance engine.

use serde_json::Value;

use crate::config::GuidanceConfig;
use crate::types::{GateAction, RiskLevel, RuleScope};

use super::patterns::{DESTRUCTIVE_PATTERNS, SECRET_PATTERNS};
use super::CompiledRule;

/// Check destructive operations gate.
pub(super) fn check_destructive(
    tool_name: &str,
    tool_input: &Value,
) -> Option<(GateAction, String)> {
    // Check bash commands
    if tool_name == "Bash" {
        if let Some(cmd) = tool_input.get("command").and_then(|v| v.as_str()) {
            let cmd_lower = cmd.to_lowercase();
            for (regex, desc, risk) in DESTRUCTIVE_PATTERNS.iter() {
                if regex.is_match(&cmd_lower) {
                    let action = match risk {
                        RiskLevel::Critical => GateAction::Deny,
                        RiskLevel::High => GateAction::Deny,
                        RiskLevel::Medium => GateAction::Ask,
                        RiskLevel::Low => GateAction::Ask,
                    };
                    return Some((action, format!("[destructive_ops] {desc}")));
                }
            }
        }
    }

    // SQL injection patterns (only for Bash commands, word-boundary aware)
    if tool_name == "Bash" {
        if let Some(cmd) = tool_input.get("command").and_then(|v| v.as_str()) {
            use regex::Regex;
            use std::sync::LazyLock;
            static SQL_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
                vec![
                    (Regex::new(r"(?i)\bdrop\s+table\b").unwrap(), "SQL DROP TABLE detected"),
                    (Regex::new(r"(?i)\bdrop\s+database\b").unwrap(), "SQL DROP DATABASE detected"),
                    (Regex::new(r"(?i)\bdelete\s+from\b").unwrap(), "SQL DELETE FROM detected"),
                    (Regex::new(r"(?i)\btruncate\s+table\b").unwrap(), "SQL TRUNCATE TABLE detected"),
                ]
            });
            for (regex, desc) in SQL_PATTERNS.iter() {
                if regex.is_match(cmd) {
                    return Some((GateAction::Ask, format!("[destructive_ops] {desc}")));
                }
            }
        }
    }

    None
}

/// Check secrets detection gate.
/// Scans only string-valued fields (not field names) to reduce false positives.
/// Skips inputs larger than 10KB to avoid performance issues on large payloads.
pub(super) fn check_secrets(tool_input: &Value) -> Option<(GateAction, String)> {
    const MAX_SCAN_SIZE: usize = 10 * 1024; // 10KB

    let strings = collect_string_values(tool_input);
    for s in &strings {
        if s.len() > MAX_SCAN_SIZE {
            continue;
        }
        for regex in SECRET_PATTERNS.iter() {
            if regex.is_match(s) {
                return Some((
                    GateAction::Deny,
                    "[secrets] Potential secret/credential detected in tool input".to_string(),
                ));
            }
        }
    }
    None
}

/// Recursively collect all string values from a JSON value (ignoring keys).
fn collect_string_values(value: &Value) -> Vec<&str> {
    let mut result = Vec::new();
    match value {
        Value::String(s) => result.push(s.as_str()),
        Value::Array(arr) => {
            for v in arr {
                result.extend(collect_string_values(v));
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                result.extend(collect_string_values(v));
            }
        }
        _ => {}
    }
    result
}

/// Check file scope gate.
pub(super) fn check_file_scope(
    tool_name: &str,
    tool_input: &Value,
    protected_paths: &[String],
) -> Option<(GateAction, String)> {
    if !matches!(tool_name, "Write" | "Edit" | "MultiEdit") {
        return None;
    }

    let file_path = tool_input
        .get("file_path")
        .or_else(|| tool_input.get("filePath"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if file_path.is_empty() {
        return None;
    }

    for protected in protected_paths {
        if glob_match(protected, file_path) {
            return Some((
                GateAction::Deny,
                format!("[file_scope] Write to protected path: {file_path} (matches {protected})"),
            ));
        }
    }

    None
}

/// Check custom rules gate.
pub(super) fn check_custom_rule(
    rule: &CompiledRule,
    tool_name: &str,
    tool_input: &Value,
) -> Option<(GateAction, String)> {
    let text = match rule.scope {
        RuleScope::Tool => tool_name.to_string(),
        RuleScope::Command => tool_input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        RuleScope::File => tool_input
            .get("file_path")
            .or_else(|| tool_input.get("filePath"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    };

    if rule.regex.is_match(&text) {
        Some((
            rule.action,
            format!("[custom:{}] {}", rule.id, rule.description),
        ))
    } else {
        None
    }
}

/// Check diff size gate.
pub(super) fn check_diff_size(
    tool_name: &str,
    tool_input: &Value,
    config: &GuidanceConfig,
) -> Option<(GateAction, String)> {
    if !matches!(tool_name, "Write" | "Edit" | "MultiEdit") {
        return None;
    }

    // Estimate lines from content/new_string
    let content = tool_input
        .get("content")
        .or_else(|| tool_input.get("new_string"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let lines = content.lines().count();
    if lines > config.max_diff_lines {
        return Some((
            GateAction::Ask,
            format!(
                "[diff_size] Edit changes ~{lines} lines (max: {})",
                config.max_diff_lines
            ),
        ));
    }

    None
}

/// Proper glob matching for protected paths.
/// Matches against both the full path and the filename component.
pub(super) fn glob_match(pattern: &str, path: &str) -> bool {
    let path_lower = path.to_lowercase();
    let pattern_lower = pattern.to_lowercase();

    // Extract filename from path
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Exact match (full path or filename)
    if path_lower == pattern_lower || filename == pattern_lower {
        return true;
    }

    // *.ext pattern (e.g., *.key, *.pem) — match file extension
    if let Some(ext) = pattern_lower.strip_prefix("*.") {
        if !ext.contains('*') {
            return filename.ends_with(&format!(".{ext}"));
        }
    }

    // *keyword* pattern (e.g., *credentials*, *secret*) — match against filename only
    if pattern_lower.starts_with('*') && pattern_lower.ends_with('*') && pattern_lower.len() > 2 {
        let keyword = &pattern_lower[1..pattern_lower.len() - 1];
        return filename.contains(keyword);
    }

    // dir/* pattern (e.g., .ssh/*) — match if any path segment matches
    if let Some(dir) = pattern_lower.strip_suffix("/*") {
        // Check if the path contains this directory as a component
        return path_lower.contains(&format!("/{dir}/"))
            || path_lower.contains(&format!("{dir}/"))
            || path_lower.starts_with(&format!("{dir}/"));
    }

    // prefix* pattern (e.g., .env.*) — match against filename
    if let Some(prefix) = pattern_lower.strip_suffix('*') {
        return filename.starts_with(prefix);
    }
    if pattern_lower.contains('*') {
        let parts: Vec<&str> = pattern_lower.split('*').collect();
        if parts.len() == 2 && !parts[0].is_empty() {
            // e.g. .env.* — check filename starts with prefix
            return filename.starts_with(parts[0]);
        }
    }

    false
}
