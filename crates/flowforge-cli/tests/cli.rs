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
        .stderr(predicate::str::contains("--title"));
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
        .stdout(predicate::str::contains("\u{2B22}"));
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
        .stdout(predicate::str::contains("Statusline Legend"))
        .stdout(predicate::str::contains("SESSION"))
        .stdout(predicate::str::contains("TRAJECTORY"))
        .stdout(predicate::str::contains("TRUST"))
        .stdout(predicate::str::contains("AGENTS"))
        .stdout(predicate::str::contains("WORK ITEMS"))
        .stdout(predicate::str::contains("WARNINGS"));
}

// ── Session subcommands ──

#[test]
fn test_session_history_no_session() {
    // No active session → exits with error
    flowforge()
        .args(["session", "history"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No active session"));
}

#[test]
fn test_session_checkpoints_no_session() {
    // No active session → exits with error
    flowforge()
        .args(["session", "checkpoints"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No active session"));
}

#[test]
fn test_session_forks_no_session() {
    // No active session → exits with error
    flowforge()
        .args(["session", "forks"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No active session"));
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
        .stdout(predicate::str::contains("hookSpecificOutput"));
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
        .stdout(predicate::str::contains("hookSpecificOutput"));
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
        .stdout(predicate::str::contains("hookSpecificOutput"));
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

// ── test-hooks subcommand ──

#[test]
fn test_test_hooks_help() {
    flowforge()
        .args(["test-hooks", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("realistic Claude Code payloads"));
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
