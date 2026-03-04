use colored::Colorize;
use serde_json::json;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Instant;

struct HookTest {
    event: &'static str,
    cli_arg: &'static str,
    must_have_stdout: bool,
    stdout_must_be_json: bool,
    max_ms: u128,
}

const HOOK_TESTS: &[HookTest] = &[
    HookTest {
        event: "PreToolUse",
        cli_arg: "pre-tool-use",
        must_have_stdout: false,
        stdout_must_be_json: false,
        max_ms: 500,
    },
    HookTest {
        event: "PostToolUse",
        cli_arg: "post-tool-use",
        must_have_stdout: false,
        stdout_must_be_json: false,
        max_ms: 500,
    },
    HookTest {
        event: "PostToolUseFailure",
        cli_arg: "post-tool-use-failure",
        must_have_stdout: false,
        stdout_must_be_json: false,
        max_ms: 500,
    },
    HookTest {
        event: "Notification",
        cli_arg: "notification",
        must_have_stdout: false,
        stdout_must_be_json: false,
        max_ms: 500,
    },
    HookTest {
        event: "UserPromptSubmit",
        cli_arg: "user-prompt-submit",
        must_have_stdout: true,
        stdout_must_be_json: true,
        max_ms: 500,
    },
    HookTest {
        event: "SessionStart",
        cli_arg: "session-start",
        must_have_stdout: true,
        stdout_must_be_json: true,
        max_ms: 500,
    },
    HookTest {
        event: "SessionEnd",
        cli_arg: "session-end",
        must_have_stdout: false,
        stdout_must_be_json: false,
        max_ms: 500,
    },
    HookTest {
        event: "Stop",
        cli_arg: "stop",
        must_have_stdout: false,
        stdout_must_be_json: false,
        max_ms: 500,
    },
    HookTest {
        event: "PreCompact",
        cli_arg: "pre-compact",
        must_have_stdout: true,
        stdout_must_be_json: true,
        max_ms: 500,
    },
    HookTest {
        event: "SubagentStart",
        cli_arg: "subagent-start",
        must_have_stdout: true,
        stdout_must_be_json: true,
        max_ms: 500,
    },
    HookTest {
        event: "SubagentStop",
        cli_arg: "subagent-stop",
        must_have_stdout: false,
        stdout_must_be_json: false,
        max_ms: 500,
    },
    HookTest {
        event: "TeammateIdle",
        cli_arg: "teammate-idle",
        must_have_stdout: false,
        stdout_must_be_json: false,
        max_ms: 500,
    },
    HookTest {
        event: "TaskCompleted",
        cli_arg: "task-completed",
        must_have_stdout: false,
        stdout_must_be_json: false,
        max_ms: 500,
    },
];

/// Build a realistic Claude Code payload for a given hook event.
/// Uses the actual cwd and a temp path for transcript — no hardcoded paths.
fn build_payload(event: &str) -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/tmp".to_string());
    let transcript = std::env::temp_dir()
        .join("flowforge-test-transcript.jsonl")
        .to_string_lossy()
        .to_string();

    // Common fields that Claude Code always sends
    let mut payload = json!({
        "session_id": "test-session-hooks",
        "transcript_path": transcript,
        "cwd": cwd,
        "hook_event_name": event,
    });

    // Event-specific fields + extra fields Claude Code sends
    match event {
        "PreToolUse" => {
            payload["permission_mode"] = json!("bypassPermissions");
            payload["tool_name"] = json!("Bash");
            payload["tool_input"] = json!({"command": "ls -la"});
            payload["tool_use_id"] = json!("toolu_test_pre_tool_use_001");
        }
        "PostToolUse" => {
            payload["permission_mode"] = json!("bypassPermissions");
            payload["tool_name"] = json!("Read");
            payload["tool_input"] = json!({"file_path": "/tmp/test.txt"});
            payload["tool_response"] = json!({"content": "file contents here"});
            payload["tool_use_id"] = json!("toolu_test_post_tool_use_001");
        }
        "PostToolUseFailure" => {
            payload["permission_mode"] = json!("bypassPermissions");
            payload["tool_name"] = json!("Bash");
            payload["tool_input"] = json!({"command": "false"});
            payload["error"] = json!("Command failed with exit code 1");
            payload["tool_use_id"] = json!("toolu_test_post_fail_001");
        }
        "Notification" => {
            payload["message"] = json!("Task completed successfully");
            payload["level"] = json!("info");
        }
        "UserPromptSubmit" => {
            payload["permission_mode"] = json!("bypassPermissions");
            payload["prompt"] = json!("fix the login bug in auth.rs");
        }
        "SessionStart" => {
            payload["source"] = json!("resume");
        }
        "SessionEnd" => {
            payload["reason"] = json!("user_exit");
        }
        "Stop" => {
            payload["stop_hook_active"] = json!(true);
        }
        "PreCompact" => {
            payload["trigger"] = json!("auto");
        }
        "SubagentStart" => {
            payload["agent_id"] = json!("agent-test-001");
            payload["agent_type"] = json!("general-purpose");
        }
        "SubagentStop" => {
            payload["agent_id"] = json!("agent-test-001");
            payload["last_assistant_message"] = json!("I have completed the task.");
        }
        "TeammateIdle" => {
            payload["teammate_name"] = json!("researcher");
            payload["team_name"] = json!("test-team");
        }
        "TaskCompleted" => {
            payload["task_id"] = json!("task-test-001");
            payload["task_subject"] = json!("Fix authentication bug");
            payload["teammate_name"] = json!("coder");
        }
        _ => {}
    }

    serde_json::to_string(&payload).expect("payload serialization cannot fail")
}

struct TestResult {
    passed: bool,
    elapsed_ms: u128,
    errors: Vec<String>,
}

fn run_hook_test(test: &HookTest, binary: &str, verbose: bool) -> TestResult {
    let payload = build_payload(test.event);
    let start = Instant::now();
    let mut errors = Vec::new();

    let mut child = match Command::new(binary)
        .args(["hook", test.cli_arg])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return TestResult {
                passed: false,
                elapsed_ms: start.elapsed().as_millis(),
                errors: vec![format!("Failed to spawn: {e}")],
            };
        }
    };

    // Write payload to stdin then close it
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.as_bytes());
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            return TestResult {
                passed: false,
                elapsed_ms: start.elapsed().as_millis(),
                errors: vec![format!("Wait error: {e}")],
            };
        }
    };

    let elapsed_ms = start.elapsed().as_millis();

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Validate wall time
    if elapsed_ms > test.max_ms {
        errors.push(format!(
            "Too slow: {}ms (max {}ms)",
            elapsed_ms, test.max_ms
        ));
    }

    // Validate exit code
    if let Some(code) = output.status.code() {
        if code != 0 {
            errors.push(format!("Exit code: {code} (expected 0)"));
        }
    } else {
        errors.push("No exit code (signal killed)".to_string());
    }

    // Validate stderr is empty
    if !stderr.trim().is_empty() {
        errors.push(format!("Stderr not empty: {}", stderr.trim()));
    }

    // Validate stdout requirements
    if test.must_have_stdout && stdout.trim().is_empty() {
        errors.push("Expected stdout output but got none".to_string());
    }

    if test.stdout_must_be_json
        && !stdout.trim().is_empty()
        && serde_json::from_str::<serde_json::Value>(stdout.trim()).is_err()
    {
        errors.push(format!(
            "Stdout is not valid JSON: {}",
            stdout.trim().chars().take(200).collect::<String>()
        ));
    }

    if verbose {
        eprintln!("  stdin:  {}", payload);
        if !stdout.is_empty() {
            eprintln!("  stdout: {}", stdout.trim());
        }
        if !stderr.is_empty() {
            eprintln!("  stderr: {}", stderr.trim());
        }
        eprintln!("  time:   {}ms", elapsed_ms);
    }

    TestResult {
        passed: errors.is_empty(),
        elapsed_ms,
        errors,
    }
}

fn find_binary() -> String {
    // Use the currently running binary — flowforge test-hooks calls flowforge hook <event>
    std::env::args()
        .next()
        .unwrap_or_else(|| "flowforge".to_string())
}

pub fn run(event_filter: Option<&str>, verbose: bool) -> flowforge_core::Result<()> {
    let binary = find_binary();

    println!(
        "{} {} hooks with realistic Claude Code payloads\n",
        "Testing".bold(),
        if event_filter.is_some() {
            "filtered"
        } else {
            "all 13"
        }
    );

    let tests: Vec<&HookTest> = if let Some(filter) = event_filter {
        let filter_lower = filter.to_lowercase().replace(['-', '_'], "");
        HOOK_TESTS
            .iter()
            .filter(|t| {
                let event_lower = t.event.to_lowercase();
                let cli_lower = t.cli_arg.to_lowercase().replace('-', "");
                event_lower.contains(&filter_lower)
                    || cli_lower.contains(&filter_lower)
                    || filter_lower.contains(&event_lower)
            })
            .collect()
    } else {
        HOOK_TESTS.iter().collect()
    };

    if tests.is_empty() {
        eprintln!("No hooks matched filter: {}", event_filter.unwrap_or(""));
        std::process::exit(1);
    }

    let mut passed = 0;
    let mut failed = 0;

    for test in &tests {
        print!("  {} {} ... ", "hook".dimmed(), test.event);
        std::io::stdout().flush().ok();

        let result = run_hook_test(test, &binary, verbose);

        if result.passed {
            println!("{} ({}ms)", "PASS".green().bold(), result.elapsed_ms);
            passed += 1;
        } else {
            println!("{} ({}ms)", "FAIL".red().bold(), result.elapsed_ms);
            for err in &result.errors {
                println!("    {} {}", "->".red(), err);
            }
            failed += 1;
        }
    }

    println!();
    if failed == 0 {
        println!(
            "  {} {}/{} hooks passed",
            "OK".green().bold(),
            passed,
            passed + failed
        );
    } else {
        println!(
            "  {} {}/{} hooks failed",
            "FAILED".red().bold(),
            failed,
            passed + failed
        );
    }

    if failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}
