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
        .stdout(predicate::str::contains("FF"));
}

#[test]
fn test_statusline_with_model() {
    flowforge()
        .arg("statusline")
        .write_stdin(r#"{"model":"opus-4"}"#)
        .assert()
        .success()
        .stdout(predicate::str::contains("FF"));
}

// ── Session subcommands ──

#[test]
fn test_session_history_no_session() {
    flowforge().args(["session", "history"]).assert().success();
}

#[test]
fn test_session_checkpoints_no_session() {
    flowforge()
        .args(["session", "checkpoints"])
        .assert()
        .success();
}

#[test]
fn test_session_forks_no_session() {
    flowforge().args(["session", "forks"]).assert().success();
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
