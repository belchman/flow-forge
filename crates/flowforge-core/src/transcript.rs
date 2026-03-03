//! JSONL transcript parser for Claude Code conversation files.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::types::ConversationMessage;
use crate::Result;

/// Message types we skip when parsing transcripts
const SKIP_TYPES: &[&str] = &[
    "progress",
    "file-history-snapshot",
    "queue-operation",
    "result",
    "login",
];

/// Parse a JSONL transcript file into conversation messages.
pub fn parse_transcript(path: &str, session_id: &str) -> Result<Vec<ConversationMessage>> {
    let content =
        std::fs::read_to_string(path).map_err(|e| crate::Error::Conversation(e.to_string()))?;

    let mut messages = Vec::new();
    let mut message_index: u32 = 0;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let entry: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Skip non-message types
        let msg_type = entry
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if SKIP_TYPES.contains(&msg_type) {
            continue;
        }

        let role = entry
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // We only care about user/assistant/system messages
        if role.is_empty() && msg_type != "system" {
            // Some entries use "type" as the indicator
            if msg_type != "user"
                && msg_type != "assistant"
                && msg_type != "tool_use"
                && msg_type != "tool_result"
            {
                continue;
            }
        }

        let effective_role = if !role.is_empty() {
            role.clone()
        } else {
            msg_type.to_string()
        };

        let content_val = if let Some(c) = entry.get("content") {
            serde_json::to_string(c).unwrap_or_default()
        } else if let Some(m) = entry.get("message") {
            serde_json::to_string(m).unwrap_or_default()
        } else {
            continue;
        };

        let model = entry
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let message_id = entry
            .get("uuid")
            .or_else(|| entry.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let parent_uuid = entry
            .get("parentUuid")
            .or_else(|| entry.get("parent_uuid"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let timestamp = entry
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        // Build metadata from extra fields
        let mut meta = serde_json::Map::new();
        if let Some(cwd) = entry.get("cwd").and_then(|v| v.as_str()) {
            meta.insert("cwd".to_string(), Value::String(cwd.to_string()));
        }
        if let Some(tool) = entry.get("tool_name").and_then(|v| v.as_str()) {
            meta.insert("tool_name".to_string(), Value::String(tool.to_string()));
        }
        let metadata = if meta.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&meta).unwrap_or_default())
        };

        messages.push(ConversationMessage {
            id: 0,
            session_id: session_id.to_string(),
            message_index,
            message_type: msg_type.to_string(),
            role: effective_role,
            content: content_val,
            model,
            message_id,
            parent_uuid,
            timestamp,
            metadata,
            source: "transcript".to_string(),
        });

        message_index += 1;
    }

    Ok(messages)
}
