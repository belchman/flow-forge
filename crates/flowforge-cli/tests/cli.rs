use assert_cmd::Command;
use predicates::prelude::*;

fn flowforge() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("flowforge").unwrap()
}

// ── Basic CLI ──

#[test]
fn test_help() {
    flowforge()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Agent orchestration for Claude Code",
        ));
}

#[test]
fn test_version() {
    flowforge()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("flowforge"));
}

#[test]
fn test_unknown_subcommand() {
    flowforge()
        .arg("nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

// ── Agent commands ──

#[test]
fn test_agent_list() {
    flowforge()
        .args(["agent", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("agents loaded"));
}

#[test]
fn test_agent_info_coder() {
    flowforge()
        .args(["agent", "info", "coder"])
        .assert()
        .success()
        .stdout(predicate::str::contains("coder"));
}

#[test]
fn test_agent_info_nonexistent() {
    flowforge()
        .args(["agent", "info", "no-such-agent-xyz"])
        .assert()
        .failure();
}

// ── Route command ──

#[test]
fn test_route() {
    flowforge()
        .args(["route", "fix a bug"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Routing:"));
}

// ── Hook commands ──

#[test]
fn test_hook_pre_tool_use_blocks_dangerous() {
    flowforge()
        .args(["hook", "pre-tool-use"])
        .write_stdin(r#"{"tool_name":"Bash","tool_input":{"command":"rm -rf /"}}"#)
        .assert()
        .success()
        .stdout(predicate::str::contains("deny"));
}

#[test]
fn test_hook_pre_tool_use_allows_safe() {
    flowforge()
        .args(["hook", "pre-tool-use"])
        .write_stdin(r#"{"tool_name":"Bash","tool_input":{"command":"ls -la"}}"#)
        .assert()
        .success();
}

#[test]
fn test_hook_user_prompt_submit() {
    flowforge()
        .args(["hook", "user-prompt-submit"])
        .write_stdin(r#"{"prompt":"test prompt"}"#)
        .assert()
        .success();
}

#[test]
fn test_hook_session_start() {
    flowforge()
        .args(["hook", "session-start"])
        .write_stdin(r#"{"session_id":"test-integration"}"#)
        .assert()
        .success();
}

#[test]
fn test_hook_notification() {
    flowforge()
        .args(["hook", "notification"])
        .write_stdin(r#"{"message":"test","level":"info"}"#)
        .assert()
        .success();
}

#[test]
fn test_hook_empty_stdin_does_not_panic() {
    // Hooks should handle empty stdin gracefully (log error, exit 0)
    flowforge()
        .args(["hook", "pre-tool-use"])
        .write_stdin("")
        .assert()
        .success();
}

#[test]
fn test_hook_invalid_json_does_not_panic() {
    flowforge()
        .args(["hook", "pre-tool-use"])
        .write_stdin("not json")
        .assert()
        .success();
}

// ── Work commands ──

#[test]
fn test_work_create_requires_title() {
    flowforge()
        .args(["work", "create"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("TITLE"));
}

// ── Session agents ──

#[test]
fn test_session_agents() {
    // Without an active session, should output "No active session" or "not initialized"
    flowforge().args(["session", "agents"]).assert().success();
}

// ── Statusline ──

#[test]
fn test_statusline_empty_stdin() {
    flowforge()
        .arg("statusline")
        .write_stdin("{}")
        .assert()
        .success()
        .stdout(predicate::str::contains("proven"));
}

#[test]
fn test_statusline_with_model() {
    flowforge()
        .arg("statusline")
        .write_stdin(r#"{"model":"claude-opus-4-6"}"#)
        .assert()
        .success()
        .stdout(predicate::str::contains("op4.6"));
}

#[test]
fn test_statusline_legend() {
    flowforge()
        .arg("statusline")
        .arg("--legend")
        .assert()
        .success()
        .stdout(predicate::str::contains("Dashboard Legend"))
        .stdout(predicate::str::contains("HEADER LINE"))
        .stdout(predicate::str::contains("INTELLIGENCE + SESSION LINE"))
        .stdout(predicate::str::contains("WORK + AGENTS LINE"));
}

// ── Session subcommands ──

#[test]
fn test_session_history_no_session() {
    // Without DB/session, exits 0 with a friendly message or exits 1 with error
    let assert = flowforge().args(["session", "history"]).assert();
    let output = assert.get_output().clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("No conversation") || stderr.contains("No active session"),
        "Expected session-related message, got stdout={stdout:?} stderr={stderr:?}"
    );
}

#[test]
fn test_session_checkpoints_no_session() {
    // Without DB/session, exits 0 with a friendly message or exits 1 with error
    let assert = flowforge().args(["session", "checkpoints"]).assert();
    let output = assert.get_output().clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("No checkpoints")
            || stderr.contains("No active session")
            || stdout.contains("Name") // checkpoint table header (auto-checkpoints may exist)
            || stdout.contains("auto:"),
        "Expected session-related message, got stdout={stdout:?} stderr={stderr:?}"
    );
}

#[test]
fn test_session_forks_no_session() {
    // Without DB/session, exits 0 with a friendly message or exits 1 with error
    let assert = flowforge().args(["session", "forks"]).assert();
    let output = assert.get_output().clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("No forks") || stderr.contains("No active session"),
        "Expected session-related message, got stdout={stdout:?} stderr={stderr:?}"
    );
}

#[test]
fn test_session_ingest_missing_file() {
    flowforge()
        .args(["session", "ingest", "/nonexistent/transcript.jsonl"])
        .assert()
        .failure();
}

// ── Mailbox commands ──

#[test]
fn test_mailbox_read_no_session() {
    flowforge().args(["mailbox", "read"]).assert().success();
}

#[test]
fn test_mailbox_send_requires_args() {
    flowforge()
        .args(["mailbox", "send"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--work-item"));
}

#[test]
fn test_mailbox_history_requires_id() {
    flowforge().args(["mailbox", "history"]).assert().failure();
}

#[test]
fn test_mailbox_agents_requires_id() {
    flowforge().args(["mailbox", "agents"]).assert().failure();
}

// ── Realistic Claude Code hook payloads (extra fields) ──

#[test]
fn test_hook_pre_tool_use_claude_code_payload() {
    flowforge()
        .args(["hook", "pre-tool-use"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "permission_mode": "bypassPermissions",
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "ls -la"},
                "tool_use_id": "toolu_test123"
            }"#,
        )
        .assert()
        .success();
}

#[test]
fn test_hook_post_tool_use_claude_code_payload() {
    flowforge()
        .args(["hook", "post-tool-use"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "permission_mode": "bypassPermissions",
                "hook_event_name": "PostToolUse",
                "tool_name": "Read",
                "tool_input": {"file_path": "/tmp/test.txt"},
                "tool_response": {"content": "hello"},
                "tool_use_id": "toolu_test456"
            }"#,
        )
        .assert()
        .success();
}

#[test]
fn test_hook_post_tool_use_failure_claude_code_payload() {
    flowforge()
        .args(["hook", "post-tool-use-failure"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "permission_mode": "bypassPermissions",
                "hook_event_name": "PostToolUseFailure",
                "tool_name": "Bash",
                "tool_input": {"command": "false"},
                "error": "exit code 1",
                "tool_use_id": "toolu_test789"
            }"#,
        )
        .assert()
        .success();
}

#[test]
fn test_hook_user_prompt_submit_claude_code_payload() {
    // After enabling the full routing pipeline, the hook may output context
    // or nothing depending on config/DB state. Just verify it succeeds.
    flowforge()
        .args(["hook", "user-prompt-submit"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "permission_mode": "bypassPermissions",
                "hook_event_name": "UserPromptSubmit",
                "prompt": "fix the login bug"
            }"#,
        )
        .assert()
        .success();
}

#[test]
fn test_hook_session_start_claude_code_payload() {
    flowforge()
        .args(["hook", "session-start"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "SessionStart",
                "source": "resume"
            }"#,
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("[FlowForge] Ready."));
}

#[test]
fn test_hook_session_end_claude_code_payload() {
    flowforge()
        .args(["hook", "session-end"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "SessionEnd",
                "reason": "user_exit"
            }"#,
        )
        .assert()
        .success();
}

#[test]
fn test_hook_stop_claude_code_payload() {
    flowforge()
        .args(["hook", "stop"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "Stop",
                "stop_hook_active": true
            }"#,
        )
        .assert()
        .success();
}

#[test]
fn test_hook_pre_compact_claude_code_payload() {
    flowforge()
        .args(["hook", "pre-compact"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "PreCompact",
                "trigger": "auto"
            }"#,
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("FlowForge Compaction Guidance"));
}

#[test]
fn test_hook_notification_claude_code_payload() {
    flowforge()
        .args(["hook", "notification"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "Notification",
                "message": "Build completed",
                "level": "info"
            }"#,
        )
        .assert()
        .success();
}

#[test]
fn test_hook_subagent_start_claude_code_payload() {
    flowforge()
        .args(["hook", "subagent-start"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "SubagentStart",
                "agent_id": "agent-001",
                "agent_type": "general-purpose"
            }"#,
        )
        .assert()
        .success();
}

#[test]
fn test_hook_subagent_stop_claude_code_payload() {
    flowforge()
        .args(["hook", "subagent-stop"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "SubagentStop",
                "agent_id": "agent-001",
                "last_assistant_message": "Done."
            }"#,
        )
        .assert()
        .success();
}

#[test]
fn test_hook_teammate_idle_claude_code_payload() {
    flowforge()
        .args(["hook", "teammate-idle"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "TeammateIdle",
                "teammate_name": "researcher",
                "team_name": "my-team"
            }"#,
        )
        .assert()
        .success();
}

#[test]
fn test_hook_task_completed_claude_code_payload() {
    flowforge()
        .args(["hook", "task-completed"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "TaskCompleted",
                "task_id": "task-001",
                "task_subject": "Fix auth bug",
                "teammate_name": "coder"
            }"#,
        )
        .assert()
        .success();
}

// ── Context hook output format (plain text, no JSON wrapper) ──

#[test]
fn test_hook_session_start_outputs_plain_text_not_json() {
    // Claude Code context hooks must output plain text, not JSON.
    // The old format {"hookSpecificOutput":{}} caused UserPromptSubmit hook errors.
    flowforge()
        .args(["hook", "session-start"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "SessionStart",
                "source": "resume"
            }"#,
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("[FlowForge] Ready."))
        .stdout(predicate::str::contains("hookSpecificOutput").not())
        .stdout(predicate::str::starts_with("{").not());
}

#[test]
fn test_hook_user_prompt_submit_no_context_outputs_nothing() {
    // Without a DB/config, routing produces no context — stdout should be empty
    // or contain plain text (never JSON).
    flowforge()
        .args(["hook", "user-prompt-submit"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "permission_mode": "bypassPermissions",
                "hook_event_name": "UserPromptSubmit",
                "prompt": "fix the login bug"
            }"#,
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("hookSpecificOutput").not())
        .stdout(predicate::str::starts_with("{").not());
}

#[test]
fn test_hook_pre_compact_outputs_plain_text_not_json() {
    flowforge()
        .args(["hook", "pre-compact"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "PreCompact",
                "trigger": "auto"
            }"#,
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("FlowForge Compaction Guidance"))
        .stdout(predicate::str::contains("hookSpecificOutput").not())
        .stdout(predicate::str::starts_with("{").not());
}

#[test]
fn test_hook_subagent_start_no_context_outputs_nothing() {
    // SubagentStart with no matching agent should produce empty stdout.
    flowforge()
        .args(["hook", "subagent-start"])
        .write_stdin(
            r#"{
                "session_id": "test-session",
                "transcript_path": "/tmp/test-transcript.jsonl",
                "cwd": "/tmp/flowforge-test",
                "hook_event_name": "SubagentStart",
                "agent_id": "test-agent-999",
                "agent_type": "nonexistent-agent-type-xyz"
            }"#,
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("hookSpecificOutput").not());
}

// ── test-hooks subcommand ──

#[test]
fn test_test_hooks_help() {
    flowforge()
        .args(["test-hooks", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("realistic Claude Code payloads"));
}

// ── Statusline error count ──

#[test]
fn test_statusline_shows_hook_err_count() {
    // Create a temp dir with .flowforge/hook-errors.log containing multiple errors
    let dir = tempfile::tempdir().unwrap();
    let ff_dir = dir.path().join(".flowforge");
    std::fs::create_dir_all(&ff_dir).unwrap();
    std::fs::write(ff_dir.join("config.toml"), "").unwrap();
    std::fs::write(
        ff_dir.join("hook-errors.log"),
        "[2026-03-04T10:00:00Z] hook1: Error 1\n[2026-03-04T10:01:00Z] hook2: Error 2\n[2026-03-04T10:02:00Z] hook3: Error 3\n",
    ).unwrap();

    let assert = flowforge()
        .arg("statusline")
        .write_stdin("{}")
        .current_dir(dir.path())
        .assert();
    let output = assert.get_output().clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show "3 hook-err" or similar count
    assert!(
        stdout.contains("3") && stdout.contains("hook-err"),
        "Expected '3 hook-err' in statusline, got: {stdout}"
    );
}

#[test]
fn test_statusline_no_hook_err_when_log_empty() {
    let dir = tempfile::tempdir().unwrap();
    let ff_dir = dir.path().join(".flowforge");
    std::fs::create_dir_all(&ff_dir).unwrap();
    std::fs::write(ff_dir.join("config.toml"), "").unwrap();
    // No hook-errors.log file

    flowforge()
        .arg("statusline")
        .write_stdin("{}")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hook-err").not());
}

// ── MCP server ──

#[test]
fn test_mcp_serve_initialize() {
    flowforge()
        .args(["mcp", "serve"])
        .write_stdin(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#,
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("flowforge"));
}

// ── Config commands ──

#[test]
fn test_config_show() {
    flowforge()
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("FlowForge Configuration"));
}

#[test]
fn test_config_get_valid_key() {
    flowforge()
        .args(["config", "get", "patterns.short_term_max"])
        .assert()
        .success()
        .stdout(predicate::str::contains("500"));
}

#[test]
fn test_config_get_invalid_key() {
    flowforge()
        .args(["config", "get", "patterns.no_such_field"])
        .assert()
        .failure();
}

#[test]
fn test_config_set_writes_toml() {
    // Use a temp dir to avoid mutating the real config
    let dir = tempfile::tempdir().unwrap();
    let ff_dir = dir.path().join(".flowforge");
    std::fs::create_dir_all(&ff_dir).unwrap();
    // Write a minimal config so `set` has something to load
    std::fs::write(ff_dir.join("config.toml"), "").unwrap();

    flowforge()
        .args(["config", "set", "patterns.short_term_max", "999"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Set"));

    // Verify the file was written
    let content = std::fs::read_to_string(ff_dir.join("config.toml")).unwrap();
    assert!(content.contains("999"));
}

#[test]
fn test_config_set_invalid_value_rejects() {
    let dir = tempfile::tempdir().unwrap();
    let ff_dir = dir.path().join(".flowforge");
    std::fs::create_dir_all(&ff_dir).unwrap();
    std::fs::write(ff_dir.join("config.toml"), "").unwrap();

    flowforge()
        .args(["config", "set", "patterns.short_term_max", "not_a_number"])
        .current_dir(dir.path())
        .assert()
        .failure();
}

#[test]
fn test_config_roundtrip_set_then_get() {
    let dir = tempfile::tempdir().unwrap();
    let ff_dir = dir.path().join(".flowforge");
    std::fs::create_dir_all(&ff_dir).unwrap();
    std::fs::write(ff_dir.join("config.toml"), "").unwrap();

    // Set a value
    flowforge()
        .args(["config", "set", "general.log_level", "debug"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Get it back
    flowforge()
        .args(["config", "get", "general.log_level"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("debug"));
}

// ── Task ID → Work Item Mapping ──

/// End-to-end test: resolve_work_item_for_task matches by title,
/// falls back to in-progress, and returns None when DB is empty.
#[test]
fn test_task_to_work_item_mapping() {
    use flowforge_memory::MemoryDb;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let db = MemoryDb::open(tmp.path()).unwrap();

    // Create two work items with distinct titles
    let now = chrono::Utc::now();
    let kanbus_item = flowforge_core::WorkItem {
        id: "kanbus-abc123".to_string(),
        external_id: Some("abc123".to_string()),
        backend: "kanbus".to_string(),
        item_type: "task".to_string(),
        title: "Fix authentication bug".to_string(),
        description: None,
        status: flowforge_core::WorkStatus::InProgress,
        assignee: None,
        parent_id: None,
        priority: 2,
        labels: vec![],
        created_at: now,
        updated_at: now,
        completed_at: None,
        session_id: None,
        metadata: None,
        claimed_by: None,
        claimed_at: None,
        last_heartbeat: None,
        progress: 0,
        stealable: false,
    };
    db.create_work_item(&kanbus_item).unwrap();

    let claude_item = flowforge_core::WorkItem {
        id: "ff-uuid-456".to_string(),
        external_id: None,
        backend: "flowforge".to_string(),
        item_type: "task".to_string(),
        title: "Add dark mode support".to_string(),
        description: None,
        status: flowforge_core::WorkStatus::InProgress,
        assignee: None,
        parent_id: None,
        priority: 2,
        labels: vec![],
        created_at: now,
        updated_at: now,
        completed_at: None,
        session_id: None,
        metadata: None,
        claimed_by: None,
        claimed_at: None,
        last_heartbeat: None,
        progress: 0,
        stealable: false,
    };
    db.create_work_item(&claude_item).unwrap();

    // Simulate what HookContext::resolve_work_item_for_task does:

    // Case 1: Claude task subject matches the kanbus item title → finds it
    let found = db.get_work_item_by_title("Fix authentication bug").unwrap();
    assert_eq!(found.as_ref().map(|i| i.id.as_str()), Some("kanbus-abc123"));
    assert_eq!(found.as_ref().map(|i| i.backend.as_str()), Some("kanbus"));

    // Case 2: Claude task subject matches the flowforge item → finds it
    let found = db.get_work_item_by_title("Add dark mode support").unwrap();
    assert_eq!(found.as_ref().map(|i| i.id.as_str()), Some("ff-uuid-456"));

    // Case 3: No title match → fallback to any in_progress item
    let found = db.get_work_item_by_title("Some unrelated task").unwrap();
    assert!(found.is_none()); // title lookup returns None
                              // Fallback: find any in-progress item
    let filter = flowforge_core::WorkFilter {
        status: Some(flowforge_core::WorkStatus::InProgress),
        ..Default::default()
    };
    let in_progress = db.list_work_items(&filter).unwrap();
    assert_eq!(in_progress.len(), 2); // both items are in_progress

    // Case 4: Completed items are excluded from title match
    db.update_work_item_status("kanbus-abc123", flowforge_core::WorkStatus::Completed)
        .unwrap();
    let found = db.get_work_item_by_title("Fix authentication bug").unwrap();
    assert!(found.is_none()); // completed → not returned

    // Case 5: Work events can be recorded against the resolved item
    let event = flowforge_core::WorkEvent {
        id: 0,
        work_item_id: "ff-uuid-456".to_string(),
        event_type: "completed".to_string(),
        old_value: Some("in_progress".to_string()),
        new_value: Some("completed".to_string()),
        actor: Some("hook:task-completed".to_string()),
        timestamp: now,
    };
    let event_id = db.record_work_event(&event).unwrap();
    assert!(event_id > 0); // FK constraint passes — no error

    // Case 6: Recording against a non-existent ID would fail FK
    let bad_event = flowforge_core::WorkEvent {
        id: 0,
        work_item_id: "claude-task-99".to_string(), // not in work_items
        event_type: "completed".to_string(),
        old_value: None,
        new_value: None,
        actor: None,
        timestamp: now,
    };
    let result = db.record_work_event(&bad_event);
    assert!(result.is_err()); // FK violation
}
