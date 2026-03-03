//! Hook I/O types for Claude Code integration.
//! Each hook receives JSON on stdin and may return JSON on stdout.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Hook Input Types ──

#[derive(Debug, Clone, Deserialize)]
pub struct PreToolUseInput {
    pub tool_name: String,
    pub tool_input: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PostToolUseInput {
    pub tool_name: String,
    pub tool_input: Value,
    #[serde(default)]
    pub tool_response: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserPromptSubmitInput {
    pub prompt: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionStartInput {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionEndInput {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StopInput {
    #[serde(default)]
    pub stop_hook_active: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreCompactInput {
    #[serde(default)]
    pub trigger: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubagentStartInput {
    pub agent_id: String,
    #[serde(default)]
    pub agent_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubagentStopInput {
    pub agent_id: String,
    #[serde(default)]
    pub last_assistant_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeammateIdleInput {
    pub teammate_name: String,
    #[serde(default)]
    pub team_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskCompletedInput {
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub task_subject: Option<String>,
    #[serde(default)]
    pub teammate_name: Option<String>,
}

// ── Hook Output Types ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreToolUseOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl PreToolUseOutput {
    pub fn allow() -> Self {
        Self {
            permission_decision: None,
            reason: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            permission_decision: Some("deny".to_string()),
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

impl ContextOutput {
    pub fn none() -> Self {
        Self {
            additional_context: None,
        }
    }

    pub fn with_context(context: impl Into<String>) -> Self {
        Self {
            additional_context: Some(context.into()),
        }
    }
}

// ── Dangerous Command Patterns ──

/// Patterns that should be blocked or warned about in Bash commands
pub const DANGEROUS_PATTERNS: &[(&str, &str)] = &[
    ("rm -rf /", "Recursive delete of root filesystem"),
    ("rm -rf ~", "Recursive delete of home directory"),
    ("rm -rf /*", "Recursive delete of all root contents"),
    (":(){:|:&};:", "Fork bomb"),
    ("mkfs.", "Filesystem formatting"),
    ("dd if=/dev/zero", "Disk overwrite with zeros"),
    ("dd if=/dev/random", "Disk overwrite with random data"),
    ("> /dev/sda", "Direct disk overwrite"),
    ("chmod -R 777 /", "Remove all permissions from root"),
    ("wget|sh", "Pipe download to shell"),
    ("curl|sh", "Pipe download to shell"),
    ("curl|bash", "Pipe download to bash"),
    ("wget|bash", "Pipe download to bash"),
    ("--no-preserve-root", "Bypasses root protection"),
    ("sudo rm -rf", "Sudo recursive force delete"),
];

/// Check if a bash command matches any dangerous pattern
pub fn check_dangerous_command(command: &str) -> Option<&'static str> {
    let cmd_lower = command.to_lowercase();
    let cmd_normalized = cmd_lower.replace('\\', "").replace('\n', " ");

    for (pattern, reason) in DANGEROUS_PATTERNS {
        if cmd_normalized.contains(&pattern.to_lowercase()) {
            return Some(reason);
        }
    }
    None
}

/// Read hook input from stdin
pub fn read_stdin() -> crate::Result<String> {
    use std::io::Read;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

/// Parse hook input from stdin as JSON
pub fn parse_stdin<T: serde::de::DeserializeOwned>() -> crate::Result<T> {
    let input = read_stdin()?;
    if input.trim().is_empty() {
        return Err(crate::Error::Hook("Empty stdin input".to_string()));
    }
    serde_json::from_str(&input).map_err(|e| crate::Error::Hook(format!("Invalid JSON input: {e}")))
}

/// Write hook output as JSON to stdout
pub fn write_stdout<T: Serialize>(output: &T) -> crate::Result<()> {
    let json = serde_json::to_string(output)?;
    println!("{json}");
    Ok(())
}
