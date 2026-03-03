//! Hook I/O types for Claude Code integration.
//! Each hook receives JSON on stdin and may return JSON on stdout.

use serde::Serialize;
use serde_json::Value;

// ── Common Hook Fields (B5) ──
// All hook inputs can optionally contain these common fields.

/// Common fields present in all hook inputs from Claude Code.
/// Extracted from raw JSON to avoid serde flatten+default bugs.
#[derive(Debug, Clone, Default)]
pub struct CommonHookFields {
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
}

impl CommonHookFields {
    /// Extract common fields from a raw JSON value.
    pub fn from_value(v: &Value) -> Self {
        Self {
            session_id: v
                .get("session_id")
                .and_then(|x| x.as_str())
                .map(String::from),
            transcript_path: v
                .get("transcript_path")
                .and_then(|x| x.as_str())
                .map(String::from),
            cwd: v.get("cwd").and_then(|x| x.as_str()).map(String::from),
        }
    }
}

// ── Hook Input Types ──
// All types use Value-based extraction to avoid serde #[flatten] + #[default]
// bugs that cause "missing field" errors when Claude Code sends extra fields.

#[derive(Debug, Clone)]
pub struct PreToolUseInput {
    pub tool_name: String,
    pub tool_input: Value,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct PostToolUseInput {
    pub tool_name: String,
    pub tool_input: Value,
    pub tool_response: Option<Value>,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct PostToolUseFailureInput {
    pub tool_name: String,
    pub tool_input: Value,
    pub error: Option<String>,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct NotificationInput {
    pub message: Option<String>,
    pub level: Option<String>,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct UserPromptSubmitInput {
    pub prompt: Option<String>,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct SessionStartInput {
    pub source: Option<String>,
    pub session_id: Option<String>,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct SessionEndInput {
    pub reason: Option<String>,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct StopInput {
    pub stop_hook_active: bool,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct PreCompactInput {
    pub trigger: Option<String>,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct SubagentStartInput {
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct SubagentStopInput {
    pub agent_id: Option<String>,
    pub last_assistant_message: Option<String>,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct TeammateIdleInput {
    pub teammate_name: Option<String>,
    pub team_name: Option<String>,
    pub common: CommonHookFields,
}

#[derive(Debug, Clone)]
pub struct TaskCompletedInput {
    pub task_id: Option<String>,
    pub task_subject: Option<String>,
    pub teammate_name: Option<String>,
    pub common: CommonHookFields,
}

// ── Hook Output Types ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PermissionInner {
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_input: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContextInner {
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_context: Option<String>,
}

/// Output for PreToolUse hooks with permission decisions (B4).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreToolUseOutput {
    hook_specific_output: PermissionInner,
}

impl PreToolUseOutput {
    /// Allow the tool use (skip output, no prompt).
    pub fn allow() -> Self {
        Self {
            hook_specific_output: PermissionInner {
                permission_decision: None,
                reason: None,
                updated_input: None,
            },
        }
    }

    /// Explicitly allow and skip user confirmation.
    pub fn allow_explicit() -> Self {
        Self {
            hook_specific_output: PermissionInner {
                permission_decision: Some("allow".to_string()),
                reason: None,
                updated_input: None,
            },
        }
    }

    /// Deny the tool use with a reason.
    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            hook_specific_output: PermissionInner {
                permission_decision: Some("deny".to_string()),
                reason: Some(reason.into()),
                updated_input: None,
            },
        }
    }

    /// Force user confirmation before proceeding.
    pub fn ask(reason: impl Into<String>) -> Self {
        Self {
            hook_specific_output: PermissionInner {
                permission_decision: Some("ask".to_string()),
                reason: Some(reason.into()),
                updated_input: None,
            },
        }
    }

    /// Allow but modify the tool input before execution (e.g., add --dry-run).
    pub fn allow_with_updated_input(updated_input: Value) -> Self {
        Self {
            hook_specific_output: PermissionInner {
                permission_decision: None,
                reason: None,
                updated_input: Some(updated_input),
            },
        }
    }
}

/// Output for hooks that provide additional context.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextOutput {
    hook_specific_output: ContextInner,
}

impl ContextOutput {
    pub fn none() -> Self {
        Self {
            hook_specific_output: ContextInner {
                additional_context: None,
            },
        }
    }

    pub fn with_context(context: impl Into<String>) -> Self {
        Self {
            hook_specific_output: ContextInner {
                additional_context: Some(context.into()),
            },
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

/// Parse hook input from stdin as raw JSON Value.
/// All hook types now use Value-based extraction to avoid serde flatten bugs.
pub fn parse_stdin_value() -> crate::Result<Value> {
    let input = read_stdin()?;
    if input.trim().is_empty() {
        return Err(crate::Error::Hook("Empty stdin input".to_string()));
    }
    serde_json::from_str(&input).map_err(|e| crate::Error::Hook(format!("Invalid JSON input: {e}")))
}

/// Helper: get a required string field from JSON, returning a hook error if missing.
pub fn require_str(v: &Value, field: &str) -> crate::Result<String> {
    v.get(field)
        .and_then(|x| x.as_str())
        .map(String::from)
        .ok_or_else(|| crate::Error::Hook(format!("Missing required field: {field}")))
}

/// Helper: get an optional string field from JSON.
pub fn opt_str(v: &Value, field: &str) -> Option<String> {
    v.get(field).and_then(|x| x.as_str()).map(String::from)
}

// ── Typed extractors for each hook input ──

impl PreToolUseInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            tool_name: require_str(v, "tool_name")?,
            tool_input: v
                .get("tool_input")
                .cloned()
                .unwrap_or(Value::Object(Default::default())),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl PostToolUseInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            tool_name: require_str(v, "tool_name")?,
            tool_input: v
                .get("tool_input")
                .cloned()
                .unwrap_or(Value::Object(Default::default())),
            tool_response: v.get("tool_response").cloned(),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl PostToolUseFailureInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            tool_name: require_str(v, "tool_name")?,
            tool_input: v
                .get("tool_input")
                .cloned()
                .unwrap_or(Value::Object(Default::default())),
            error: opt_str(v, "error"),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl NotificationInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            message: opt_str(v, "message"),
            level: opt_str(v, "level"),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl UserPromptSubmitInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            prompt: opt_str(v, "prompt"),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl SessionStartInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            source: opt_str(v, "source"),
            session_id: opt_str(v, "session_id"),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl SessionEndInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            reason: opt_str(v, "reason"),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl StopInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            stop_hook_active: v
                .get("stop_hook_active")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl PreCompactInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            trigger: opt_str(v, "trigger"),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl SubagentStartInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            agent_id: opt_str(v, "agent_id"),
            agent_type: opt_str(v, "agent_type"),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl SubagentStopInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            agent_id: opt_str(v, "agent_id"),
            last_assistant_message: opt_str(v, "last_assistant_message"),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl TeammateIdleInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            teammate_name: opt_str(v, "teammate_name"),
            team_name: opt_str(v, "team_name"),
            common: CommonHookFields::from_value(v),
        })
    }
}

impl TaskCompletedInput {
    pub fn from_value(v: &Value) -> crate::Result<Self> {
        Ok(Self {
            task_id: opt_str(v, "task_id"),
            task_subject: opt_str(v, "task_subject"),
            teammate_name: opt_str(v, "teammate_name"),
            common: CommonHookFields::from_value(v),
        })
    }
}

/// Write hook output as JSON to stdout
pub fn write_stdout<T: Serialize>(output: &T) -> crate::Result<()> {
    use std::io::Write;
    let json = serde_json::to_string(output)?;
    println!("{json}");
    std::io::stdout().flush()?;
    Ok(())
}
