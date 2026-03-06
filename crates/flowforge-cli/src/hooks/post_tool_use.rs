use chrono::Utc;
use flowforge_core::hook::PostToolUseInput;
use flowforge_core::work_tracking;
use flowforge_core::{EditRecord, Result};
use flowforge_memory::MemoryDb;
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let input = PostToolUseInput::from_value(&ctx.raw)?;

    if ctx.db.is_none() {
        return Ok(());
    }

    // Record edits for Write, Edit, MultiEdit operations
    if ctx.config.hooks.edit_tracking {
        match input.tool_name.as_str() {
            "Write" | "Edit" | "MultiEdit" => {
                ctx.with_db("record_edit", |db| record_edit(&input, db));
            }
            _ => {}
        }
    }

    // Sync Claude Tasks → FlowForge work items
    match input.tool_name.as_str() {
        "TaskCreate" => {
            ctx.with_db("sync_task_create", |db| {
                sync_claude_task_create(&input, db, &ctx.config.work_tracking)
            });
        }
        "TaskUpdate" => {
            ctx.with_db("sync_task_update", |db| {
                sync_claude_task_update(&input, db, &ctx.config.work_tracking)
            });
        }
        _ => {}
    }

    // Record trajectory step
    ctx.with_db("record_trajectory_step", |db| {
        if let Some(session) = db.get_current_session()? {
            if let Some(trajectory) = db.get_active_trajectory(&session.id)? {
                // Hash tool_input for privacy
                let input_str = serde_json::to_string(&input.tool_input).unwrap_or_default();
                let input_hash = format!("{:x}", Sha256::digest(input_str.as_bytes()));
                db.record_trajectory_step(
                    &trajectory.id,
                    &input.tool_name,
                    Some(&input_hash),
                    flowforge_core::trajectory::StepOutcome::Success,
                    None,
                )?;
            }
        }
        Ok(())
    });

    // Record test co-occurrences: when a test command runs after file edits, link them
    if input.tool_name == "Bash" {
        if let Some(cmd) = input.tool_input.get("command").and_then(|v| v.as_str()) {
            if is_test_command(cmd) {
                ctx.with_db("record_test_co_occurrence", |db| {
                    if let Some(session) = db.get_current_session()? {
                        let edits = db.get_edits_for_session(&session.id)?;
                        let mut seen = std::collections::HashSet::new();
                        let recent_files: Vec<String> = edits
                            .iter()
                            .rev()
                            .filter(|e| seen.insert(e.file_path.clone()))
                            .take(10)
                            .map(|e| e.file_path.clone())
                            .collect();

                        let test_file = extract_test_target(cmd);
                        for file in &recent_files {
                            db.record_test_co_occurrence(file, &test_file, Some(cmd))?;
                        }
                    }
                    Ok(())
                });
            }
        }
    }

    // Check recent tool sequence against failure patterns (observational logging)
    ctx.with_db("check_failure_patterns", |db| {
        if let Some(session) = db.get_current_session()? {
            if let Some(trajectory) = db.get_active_trajectory(&session.id)? {
                let steps = db.get_trajectory_steps(&trajectory.id)?;
                let recent_tools: Vec<&str> = steps
                    .iter()
                    .rev()
                    .take(5)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .map(|s| s.tool_name.as_str())
                    .collect();

                let matches = db.check_failure_pattern(&recent_tools)?;
                for m in &matches {
                    // Only log high-frequency patterns to avoid flooding hook-errors.log
                    if m.occurrence_count > 2 {
                        if let Ok(project_dir) = std::env::current_dir() {
                            let log_path = project_dir.join(".flowforge").join("hook-errors.log");
                            if let Ok(mut file) = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(&log_path)
                            {
                                use std::io::Write;
                                let _ = writeln!(
                                    file,
                                    "[{}] WARN failure_pattern_triggered: {} -- {} (hint: {})",
                                    Utc::now().to_rfc3339(),
                                    m.pattern_name,
                                    m.description,
                                    m.prevention_hint,
                                );
                            }
                        }
                    }
                    db.record_failure_pattern(
                        &m.pattern_name,
                        &m.description,
                        &m.trigger_tools,
                        &m.prevention_hint,
                    )?;
                }
            }
        }
        Ok(())
    });

    Ok(())
}

/// Sync a Claude TaskCreate tool call to a FlowForge work item.
/// Does NOT create new work items — Claude tasks are sub-steps of an existing
/// kanbus/FlowForge item. Instead, links the Claude task ID to the active parent.
fn sync_claude_task_create(
    input: &PostToolUseInput,
    db: &MemoryDb,
    _config: &flowforge_core::config::WorkTrackingConfig,
) -> Result<()> {
    let subject = input
        .tool_input
        .get("subject")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if subject.is_empty() {
        return Ok(());
    }

    // Try to extract the task ID from the tool_response
    let claude_task_id = input
        .tool_response
        .as_ref()
        .and_then(|r| r.get("id").or_else(|| r.get("taskId")))
        .and_then(|v| v.as_str());

    // Store the mapping: Claude task ID → subject, so TaskUpdate can resolve it later.
    // We store as metadata in the DB's KV store for lightweight lookup.
    if let Some(tid) = claude_task_id {
        db.set_meta(&format!("claude_task:{}", tid), subject)?;
    }

    Ok(())
}

/// Sync a Claude TaskUpdate tool call to an existing FlowForge work item.
/// When all session tasks are completed, updates the parent kanbus/FlowForge item.
fn sync_claude_task_update(
    input: &PostToolUseInput,
    db: &MemoryDb,
    config: &flowforge_core::config::WorkTrackingConfig,
) -> Result<()> {
    let task_id = input
        .tool_input
        .get("taskId")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let new_status = input
        .tool_input
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    // Only sync status changes
    if new_status.is_empty() || new_status == "deleted" {
        return Ok(());
    }

    // Track the Claude task status in KV for roll-up checks
    if !task_id.is_empty() {
        db.set_meta(&format!("claude_task_status:{}", task_id), new_status)?;
    }

    let ff_status: flowforge_core::WorkStatus = match new_status.parse() {
        Ok(s) => s,
        Err(_) => return Ok(()), // Unknown status (e.g. "deleted"), skip
    };

    // Find the parent work item: by title match first, then any in-progress item
    let subject = input.tool_input.get("subject").and_then(|v| v.as_str());
    let work_item = subject
        .and_then(|s| db.get_work_item_by_title(s).ok().flatten())
        .or_else(|| {
            // Fall back to any in-progress work item (the active kanbus item)
            let filter = flowforge_core::WorkFilter {
                status: Some(flowforge_core::WorkStatus::InProgress),
                ..Default::default()
            };
            db.list_work_items(&filter)
                .ok()
                .and_then(|items| items.into_iter().next())
        });

    // Only propagate "completed" to the parent when it's a direct title match.
    // For sub-tasks (fallback match), log progress but don't auto-complete the parent.
    if let Some(item) = work_item {
        let is_direct_match = subject.map(|s| s == item.title).unwrap_or(false);

        if is_direct_match && item.status != ff_status {
            let _ =
                work_tracking::update_status(db, config, &item.id, ff_status, "hook:post_tool_use");
        } else if !is_direct_match {
            // Log progress event against the parent item
            let task_subject = if !task_id.is_empty() {
                db.get_meta(&format!("claude_task:{}", task_id))
                    .ok()
                    .flatten()
            } else {
                subject.map(String::from)
            };
            let event = flowforge_core::WorkEvent {
                id: 0,
                work_item_id: item.id.clone(),
                event_type: "subtask_status".to_string(),
                old_value: task_subject,
                new_value: Some(ff_status.to_string()),
                actor: Some("hook:post_tool_use".to_string()),
                timestamp: chrono::Utc::now(),
            };
            let _ = db.record_work_event(&event);
        }
    }

    Ok(())
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use flowforge_core::config::WorkTrackingConfig;
    use flowforge_core::hook::CommonHookFields;
    use flowforge_core::WorkItem;

    fn test_db() -> MemoryDb {
        MemoryDb::open(std::path::Path::new(":memory:")).unwrap()
    }

    fn test_config() -> WorkTrackingConfig {
        WorkTrackingConfig {
            backend: "flowforge".to_string(),
            ..Default::default()
        }
    }

    fn make_work_item(id: &str, title: &str, status: flowforge_core::WorkStatus) -> WorkItem {
        WorkItem {
            id: id.to_string(),
            external_id: None,
            backend: "kanbus".to_string(),
            item_type: "task".to_string(),
            title: title.to_string(),
            description: None,
            status,
            assignee: None,
            parent_id: None,
            priority: 2,
            labels: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
            session_id: None,
            metadata: None,
            claimed_by: None,
            claimed_at: None,
            last_heartbeat: None,
            progress: 0,
            stealable: false,
        }
    }

    fn make_input(tool_name: &str, tool_input: serde_json::Value) -> PostToolUseInput {
        PostToolUseInput {
            tool_name: tool_name.to_string(),
            tool_input,
            tool_response: None,
            common: CommonHookFields {
                session_id: None,
                transcript_path: None,
                cwd: None,
            },
        }
    }

    // ── TaskCreate tests ──

    #[test]
    fn test_task_create_stores_kv_mapping() {
        let db = test_db();
        let config = test_config();
        let mut input = make_input(
            "TaskCreate",
            serde_json::json!({"subject": "Fix the parser", "description": "Details here"}),
        );
        input.tool_response = Some(serde_json::json!({"id": "task-42"}));

        sync_claude_task_create(&input, &db, &config).unwrap();

        // Verify KV mapping was stored
        let stored = db.get_meta("claude_task:task-42").unwrap();
        assert_eq!(stored.as_deref(), Some("Fix the parser"));
    }

    #[test]
    fn test_task_create_skips_empty_subject() {
        let db = test_db();
        let config = test_config();
        let input = make_input("TaskCreate", serde_json::json!({"subject": ""}));

        // Should not panic or error
        sync_claude_task_create(&input, &db, &config).unwrap();
    }

    #[test]
    fn test_task_create_no_response_id_skips_kv() {
        let db = test_db();
        let config = test_config();
        let input = make_input("TaskCreate", serde_json::json!({"subject": "Some task"}));
        // tool_response is None — no task ID to store

        sync_claude_task_create(&input, &db, &config).unwrap();

        // No KV entry should exist (we can't check a specific key without an ID)
        // Just verify it didn't error
    }

    // ── TaskUpdate tests ──

    #[test]
    fn test_task_update_direct_title_match_syncs_status() {
        let db = test_db();
        let config = test_config();

        // Create a work item with a specific title
        let item = make_work_item("wi-1", "Deploy new API", flowforge_core::WorkStatus::InProgress);
        db.create_work_item(&item).unwrap();

        // TaskUpdate with matching subject
        let input = make_input(
            "TaskUpdate",
            serde_json::json!({"taskId": "7", "status": "completed", "subject": "Deploy new API"}),
        );

        sync_claude_task_update(&input, &db, &config).unwrap();

        // Work item should now be completed
        let updated = db.get_work_item("wi-1").unwrap().unwrap();
        assert_eq!(updated.status, flowforge_core::WorkStatus::Completed);
    }

    #[test]
    fn test_task_update_subtask_logs_event_not_complete() {
        let db = test_db();
        let config = test_config();

        // Parent kanbus item
        let item = make_work_item("wi-parent", "Big feature implementation", flowforge_core::WorkStatus::InProgress);
        db.create_work_item(&item).unwrap();

        // First, simulate TaskCreate to store the KV mapping
        let create_input = PostToolUseInput {
            tool_name: "TaskCreate".to_string(),
            tool_input: serde_json::json!({"subject": "Write unit tests"}),
            tool_response: Some(serde_json::json!({"id": "task-99"})),
            common: CommonHookFields {
                session_id: None,
                transcript_path: None,
                cwd: None,
            },
        };
        sync_claude_task_create(&create_input, &db, &config).unwrap();

        // Now TaskUpdate the sub-task to completed (different title from parent)
        let update_input = make_input(
            "TaskUpdate",
            serde_json::json!({"taskId": "task-99", "status": "completed"}),
        );

        sync_claude_task_update(&update_input, &db, &config).unwrap();

        // Parent should NOT be completed — it's a subtask
        let parent = db.get_work_item("wi-parent").unwrap().unwrap();
        assert_eq!(parent.status, flowforge_core::WorkStatus::InProgress);

        // But a subtask_status event should be logged against the parent
        let events = db.get_work_events("wi-parent", 10).unwrap();
        let subtask_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "subtask_status")
            .collect();
        assert_eq!(subtask_events.len(), 1);
        assert_eq!(
            subtask_events[0].old_value.as_deref(),
            Some("Write unit tests")
        );
        assert_eq!(subtask_events[0].new_value.as_deref(), Some("completed"));
    }

    #[test]
    fn test_task_update_skips_empty_status() {
        let db = test_db();
        let config = test_config();

        let input = make_input(
            "TaskUpdate",
            serde_json::json!({"taskId": "1", "subject": "Updated title"}),
        );
        // No "status" field — should return early
        sync_claude_task_update(&input, &db, &config).unwrap();
    }

    #[test]
    fn test_task_update_skips_deleted() {
        let db = test_db();
        let config = test_config();

        let item = make_work_item("wi-del", "Some task", flowforge_core::WorkStatus::InProgress);
        db.create_work_item(&item).unwrap();

        let input = make_input(
            "TaskUpdate",
            serde_json::json!({"taskId": "1", "status": "deleted"}),
        );

        sync_claude_task_update(&input, &db, &config).unwrap();

        // Item should remain in_progress — deleted is not synced
        let fetched = db.get_work_item("wi-del").unwrap().unwrap();
        assert_eq!(fetched.status, flowforge_core::WorkStatus::InProgress);
    }

    #[test]
    fn test_task_update_no_work_items_is_noop() {
        let db = test_db();
        let config = test_config();

        // Empty DB — no work items
        let input = make_input(
            "TaskUpdate",
            serde_json::json!({"taskId": "1", "status": "completed"}),
        );

        // Should not panic
        sync_claude_task_update(&input, &db, &config).unwrap();
    }

    #[test]
    fn test_task_update_stores_status_in_kv() {
        let db = test_db();
        let config = test_config();

        let input = make_input(
            "TaskUpdate",
            serde_json::json!({"taskId": "task-5", "status": "in_progress"}),
        );

        sync_claude_task_update(&input, &db, &config).unwrap();

        // Verify status was tracked in KV
        let stored = db.get_meta("claude_task_status:task-5").unwrap();
        assert_eq!(stored.as_deref(), Some("in_progress"));
    }

    // ── Test detection tests ──

    #[test]
    fn test_is_test_command_detects_cargo_test() {
        assert!(is_test_command("cargo test --workspace"));
        assert!(is_test_command("cargo test -p flowforge-memory"));
        assert!(is_test_command("  cargo test"));
    }

    #[test]
    fn test_is_test_command_detects_other_runners() {
        assert!(is_test_command("npm test"));
        assert!(is_test_command("pytest tests/"));
        assert!(is_test_command("go test ./..."));
        assert!(is_test_command("npx jest"));
        assert!(is_test_command("npx vitest run"));
    }

    #[test]
    fn test_is_test_command_rejects_non_test() {
        assert!(!is_test_command("cargo build"));
        assert!(!is_test_command("git status"));
        assert!(!is_test_command("echo 'cargo test'"));
        assert!(!is_test_command("ls tests/"));
    }

    #[test]
    fn test_extract_test_target_package() {
        let target = extract_test_target("cargo test --package flowforge-memory");
        assert_eq!(target, "test:flowforge-memory");

        let target = extract_test_target("cargo test -p flowforge-cli");
        assert_eq!(target, "test:flowforge-cli");
    }

    #[test]
    fn test_extract_test_target_fallback() {
        let target = extract_test_target("cargo test --workspace");
        assert!(target.starts_with("test:"));
    }
}

/// Detect common test commands from various ecosystems.
fn is_test_command(cmd: &str) -> bool {
    let trimmed = cmd.trim();
    trimmed.starts_with("cargo test")
        || trimmed.starts_with("npm test")
        || trimmed.starts_with("npx jest")
        || trimmed.starts_with("npx vitest")
        || trimmed.starts_with("pytest")
        || trimmed.starts_with("python -m pytest")
        || trimmed.starts_with("go test")
        || trimmed.starts_with("make test")
        || trimmed.starts_with("./gradlew test")
        || trimmed.starts_with("bundle exec rspec")
        || trimmed.starts_with("mix test")
        || trimmed.starts_with("dotnet test")
}

/// Extract a meaningful test target from a test command.
/// Returns the specific test file/module if specified, or a generic label.
fn extract_test_target(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    // Look for --package, -p, or specific test file paths
    for (i, part) in parts.iter().enumerate() {
        if (*part == "--package" || *part == "-p") && i + 1 < parts.len() {
            return format!("test:{}", parts[i + 1]);
        }
        // Detect test file paths (e.g., tests/foo.rs, test_*.py)
        if part.contains("test") && (part.contains('/') || part.contains('.')) {
            return format!("test:{}", part);
        }
    }
    // Fall back to the full command (truncated)
    let target: String = cmd.chars().take(100).collect();
    format!("test:{}", target)
}

fn record_edit(input: &PostToolUseInput, db: &MemoryDb) -> Result<()> {
    let file_path = input
        .tool_input
        .get("file_path")
        .or_else(|| input.tool_input.get("filePath"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let extension = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_string());

    let session_id = db
        .get_current_session()?
        .map(|s| s.id)
        .unwrap_or_else(|| "unknown".to_string());

    let edit = EditRecord {
        session_id: session_id.clone(),
        timestamp: Utc::now(),
        file_path: file_path.to_string(),
        operation: input.tool_name.clone(),
        file_extension: extension,
    };

    db.record_edit(&edit)?;
    db.increment_session_edits(&session_id)?;

    Ok(())
}
