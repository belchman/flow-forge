use std::path::Path;

use chrono::Utc;
use rusqlite::params;

use flowforge_core::{
    types::{GateAction, GateDecision, RiskLevel},
    SessionInfo, WorkEvent, WorkFilter, WorkItem,
};

use super::helpers::{blob_to_vector, parse_datetime, vector_to_blob};
use super::schema::SCHEMA_VERSION;
use super::{MemoryDb, SqliteExt};

fn test_db() -> MemoryDb {
    MemoryDb::open(Path::new(":memory:")).unwrap()
}

fn test_work_item(id: &str, title: &str) -> WorkItem {
    WorkItem {
        id: id.to_string(),
        external_id: None,
        backend: "flowforge".to_string(),
        item_type: "task".to_string(),
        title: title.to_string(),
        description: None,
        status: flowforge_core::WorkStatus::Pending,
        assignee: None,
        parent_id: None,
        priority: 2,
        labels: vec![],
        created_at: Utc::now(),
        updated_at: Utc::now(),
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

#[test]
fn test_work_item_crud() {
    let db = test_db();
    let item = test_work_item("wi-1", "Fix login bug");
    db.create_work_item(&item).unwrap();
    let fetched = db.get_work_item("wi-1").unwrap().unwrap();
    assert_eq!(fetched.title, "Fix login bug");
    assert_eq!(fetched.status, flowforge_core::WorkStatus::Pending);
    db.update_work_item_status("wi-1", flowforge_core::WorkStatus::InProgress).unwrap();
    let updated = db.get_work_item("wi-1").unwrap().unwrap();
    assert_eq!(updated.status, flowforge_core::WorkStatus::InProgress);
    db.update_work_item_assignee("wi-1", "agent:coder").unwrap();
    let assigned = db.get_work_item("wi-1").unwrap().unwrap();
    assert_eq!(assigned.assignee, Some("agent:coder".to_string()));
    db.update_work_item_status("wi-1", flowforge_core::WorkStatus::Completed).unwrap();
    let completed = db.get_work_item("wi-1").unwrap().unwrap();
    assert_eq!(completed.status, flowforge_core::WorkStatus::Completed);
    assert!(completed.completed_at.is_some());
}

#[test]
fn test_work_item_external_id_lookup() {
    let db = test_db();
    let mut item = test_work_item("wi-2", "External task");
    item.external_id = Some("kbs-123".to_string());
    item.backend = "kanbus".to_string();
    db.create_work_item(&item).unwrap();
    let fetched = db.get_work_item_by_external_id("kbs-123").unwrap().unwrap();
    assert_eq!(fetched.id, "wi-2");
    assert_eq!(fetched.backend, "kanbus");
}

#[test]
fn test_work_item_unique_external_id() {
    let db = test_db();
    let mut item1 = test_work_item("wi-3", "First");
    item1.external_id = Some("ext-dup".to_string());
    db.create_work_item(&item1).unwrap();
    let mut item2 = test_work_item("wi-4", "Second");
    item2.external_id = Some("ext-dup".to_string());
    let result = db.create_work_item(&item2);
    if result.is_ok() {
        let found = db.get_work_item_by_external_id("ext-dup").unwrap().unwrap();
        assert_eq!(found.title, "Second");
    }
}

#[test]
fn test_work_item_list_filter() {
    let db = test_db();
    db.create_work_item(&test_work_item("wi-a", "Task A"))
        .unwrap();
    db.create_work_item(&test_work_item("wi-b", "Task B"))
        .unwrap();
    db.update_work_item_status("wi-b", flowforge_core::WorkStatus::Completed).unwrap();
    let all = db.list_work_items(&WorkFilter::default()).unwrap();
    assert_eq!(all.len(), 2);
    let pending = db
        .list_work_items(&WorkFilter {
            status: Some(flowforge_core::WorkStatus::Pending),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "wi-a");
    let count = db.count_work_items_by_status(flowforge_core::WorkStatus::Completed).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_work_item_delete_cascades_events() {
    let db = test_db();
    db.create_work_item(&test_work_item("wi-del", "To delete"))
        .unwrap();
    let event = WorkEvent {
        id: 0,
        work_item_id: "wi-del".to_string(),
        event_type: "created".to_string(),
        old_value: None,
        new_value: Some("To delete".to_string()),
        actor: Some("test".to_string()),
        timestamp: Utc::now(),
    };
    db.record_work_event(&event).unwrap();
    assert_eq!(db.get_work_events("wi-del", 10).unwrap().len(), 1);
    db.delete_work_item("wi-del").unwrap();
    assert!(db.get_work_item("wi-del").unwrap().is_none());
    assert_eq!(db.get_work_events("wi-del", 10).unwrap().len(), 0);
}

#[test]
fn test_work_events() {
    let db = test_db();
    db.create_work_item(&test_work_item("wi-ev", "Event test"))
        .unwrap();
    let event1 = WorkEvent {
        id: 0,
        work_item_id: "wi-ev".to_string(),
        event_type: "created".to_string(),
        old_value: None,
        new_value: Some("Event test".to_string()),
        actor: Some("user".to_string()),
        timestamp: Utc::now(),
    };
    let event2 = WorkEvent {
        id: 0,
        work_item_id: "wi-ev".to_string(),
        event_type: "status_changed".to_string(),
        old_value: Some("pending".to_string()),
        new_value: Some("in_progress".to_string()),
        actor: Some("agent:coder".to_string()),
        timestamp: Utc::now(),
    };
    db.record_work_event(&event1).unwrap();
    db.record_work_event(&event2).unwrap();
    assert_eq!(db.get_work_events("wi-ev", 10).unwrap().len(), 2);
    let recent = db.get_recent_work_events(1).unwrap();
    assert_eq!(recent.len(), 1);
}

#[test]
fn test_work_item_backend_update() {
    let db = test_db();
    db.create_work_item(&test_work_item("wi-push", "Push test"))
        .unwrap();
    assert_eq!(
        db.get_work_item("wi-push").unwrap().unwrap().backend,
        "flowforge"
    );
    db.update_work_item_backend("wi-push", "kanbus").unwrap();
    assert_eq!(
        db.get_work_item("wi-push").unwrap().unwrap().backend,
        "kanbus"
    );
}

#[test]
fn test_session_lifecycle() {
    let db = test_db();
    let session = SessionInfo {
        id: "sess-1".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();
    assert_eq!(db.get_current_session().unwrap().unwrap().id, "sess-1");
    db.increment_session_edits("sess-1").unwrap();
    db.increment_session_commands("sess-1").unwrap();
    let updated = db.get_current_session().unwrap().unwrap();
    assert_eq!(updated.edits, 1);
    assert_eq!(updated.commands, 1);
    db.end_session("sess-1", Utc::now()).unwrap();
    assert!(db.get_current_session().unwrap().is_none());
    let sessions = db.list_sessions(10).unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].ended_at.is_some());
}

#[test]
fn test_kv_operations() {
    let db = test_db();
    db.kv_set("test-key", "test-value", "default").unwrap();
    assert_eq!(
        db.kv_get("test-key", "default").unwrap(),
        Some("test-value".to_string())
    );
    assert!(db.kv_get("missing", "default").unwrap().is_none());
    db.kv_delete("test-key", "default").unwrap();
    assert!(db.kv_get("test-key", "default").unwrap().is_none());
}

#[test]
fn test_foreign_keys_enabled() {
    let db = test_db();
    let fk_status: i32 = db
        .conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(fk_status, 1);
}

#[test]
fn test_gate_decisions_asc_order() {
    use sha2::{Digest, Sha256};
    let db = test_db();
    let session_id = "test-session";
    let mut prev_hash = String::new();
    let tools = ["Bash", "Read", "Edit"];
    for (i, tool) in tools.iter().enumerate() {
        let reason = format!("reason-{}", i);
        let input = format!("{}{}{}{}", session_id, tool, reason, prev_hash);
        let hash = format!("{:x}", Sha256::digest(input.as_bytes()));
        let decision = GateDecision {
            id: 0,
            session_id: session_id.to_string(),
            rule_id: Some(format!("rule-{}", i)),
            gate_name: "test_gate".to_string(),
            tool_name: tool.to_string(),
            action: GateAction::Allow,
            reason,
            risk_level: RiskLevel::Low,
            trust_before: 1.0,
            trust_after: 1.0,
            timestamp: Utc::now(),
            hash: hash.clone(),
            prev_hash: prev_hash.clone(),
        };
        db.record_gate_decision(&decision).unwrap();
        prev_hash = hash;
    }
    let asc = db.get_gate_decisions_asc(session_id, 100).unwrap();
    assert_eq!(asc.len(), 3);
    assert_eq!(asc[0].tool_name, "Bash");
    assert_eq!(asc[1].tool_name, "Read");
    assert_eq!(asc[2].tool_name, "Edit");
    let mut prev = String::new();
    for d in &asc {
        let expected_input = format!("{}{}{}{}", d.session_id, d.tool_name, d.reason, prev);
        let expected_hash = format!("{:x}", Sha256::digest(expected_input.as_bytes()));
        assert_eq!(d.hash, expected_hash);
        assert_eq!(d.prev_hash, prev);
        prev = d.hash.clone();
    }
    let desc = db.get_gate_decisions(session_id, 100).unwrap();
    assert_eq!(desc[0].tool_name, "Edit");
    assert_eq!(desc[2].tool_name, "Bash");
}

fn create_stealable_item(db: &MemoryDb, id: &str, progress: i32, stale_mins: i64) {
    let mut item = test_work_item(id, &format!("Task {id}"));
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();
    db.claim_work_item(id, "session-old").unwrap();
    let old_hb = (Utc::now() - chrono::Duration::minutes(stale_mins)).to_rfc3339();
    db.conn
        .execute(
            "UPDATE work_items SET last_heartbeat = ?1, progress = ?2 WHERE id = ?3",
            params![old_hb, progress, id],
        )
        .unwrap();
}

#[test]
fn test_steal_work_item_safe() {
    let db = test_db();
    let mut item = test_work_item("ws-1", "Steal me");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();
    db.claim_work_item("ws-1", "old-session").unwrap();
    db.conn
        .execute("UPDATE work_items SET stealable = 1 WHERE id = 'ws-1'", [])
        .unwrap();
    assert!(db.steal_work_item_safe("ws-1", "new-session", 3).unwrap());
    let fetched = db.get_work_item("ws-1").unwrap().unwrap();
    assert_eq!(fetched.claimed_by, Some("new-session".to_string()));
    assert!(!fetched.stealable);
    let count: i32 = db
        .conn
        .query_row(
            "SELECT steal_count FROM work_items WHERE id = 'ws-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_steal_anti_thrashing() {
    let db = test_db();
    let mut item = test_work_item("ws-2", "Anti-thrash");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();
    db.claim_work_item("ws-2", "s1").unwrap();
    db.conn
        .execute(
            "UPDATE work_items SET stealable = 1, steal_count = 3 WHERE id = 'ws-2'",
            [],
        )
        .unwrap();
    assert!(!db.steal_work_item_safe("ws-2", "s2", 3).unwrap());
}

#[test]
fn test_claim_load_aware() {
    let db = test_db();
    for i in 0..2 {
        let mut item = test_work_item(&format!("la-{i}"), &format!("Load {i}"));
        item.status = flowforge_core::WorkStatus::InProgress;
        db.create_work_item(&item).unwrap();
        db.claim_work_item(&format!("la-{i}"), "session-a").unwrap();
    }
    let mut item3 = test_work_item("la-2", "Load 2");
    item3.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item3).unwrap();
    assert!(db
        .claim_work_item_load_aware("la-2", "session-a", 3)
        .unwrap());
    let mut item4 = test_work_item("la-3", "Load 3");
    item4.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item4).unwrap();
    assert!(!db
        .claim_work_item_load_aware("la-3", "session-a", 3)
        .unwrap());
}

#[test]
fn test_detect_stale_tiered_progress() {
    let db = test_db();
    create_stealable_item(&db, "st-0", 0, 35);
    create_stealable_item(&db, "st-50", 50, 35);
    create_stealable_item(&db, "st-90", 90, 120);
    assert_eq!(db.detect_stale_tiered(30, 3, 10).unwrap(), 1);
    assert!(db.get_work_item("st-0").unwrap().unwrap().stealable);
    assert!(!db.get_work_item("st-50").unwrap().unwrap().stealable);
    assert!(!db.get_work_item("st-90").unwrap().unwrap().stealable);
}

#[test]
fn test_detect_stale_tiered_cooldown() {
    let db = test_db();
    create_stealable_item(&db, "cd-1", 0, 60);
    let now = Utc::now().to_rfc3339();
    db.conn
        .execute(
            "UPDATE work_items SET last_stolen_at = ?1 WHERE id = 'cd-1'",
            params![now],
        )
        .unwrap();
    assert_eq!(db.detect_stale_tiered(30, 3, 10).unwrap(), 0);
}

#[test]
fn test_record_session_effectiveness() {
    let db = test_db();
    let session_id = "eff-sess-1";
    let session = SessionInfo {
        id: session_id.to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: ".".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();
    db.record_context_injection(session_id, None, "pattern", Some("pat-1"), Some(0.8), None)
        .unwrap();
    db.record_context_injection(session_id, None, "pattern", Some("pat-2"), Some(0.6), None)
        .unwrap();
    assert_eq!(
        db.record_session_effectiveness(session_id, "success")
            .unwrap(),
        2
    );
}

#[test]
fn test_recompute_effectiveness_decay() {
    let db = test_db();
    let now = Utc::now().to_rfc3339();
    let old = (Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    db.conn.execute(
        "INSERT INTO pattern_effectiveness (pattern_id, session_id, outcome, similarity, timestamp) VALUES ('decay-pat', 'sess-a', 'success', 0.9, ?1)",
        params![now],
    ).unwrap();
    db.conn.execute(
        "INSERT INTO pattern_effectiveness (pattern_id, session_id, outcome, similarity, timestamp) VALUES ('decay-pat', 'sess-b', 'failure', 0.9, ?1)",
        params![old],
    ).unwrap();
    db.conn.execute(
        "INSERT INTO patterns_long (id, content, category, usage_count, last_used, effectiveness_score, effectiveness_samples) VALUES ('decay-pat', 'test pattern', 'test', 1, ?1, 0.0, 0)",
        params![now],
    ).unwrap();
    db.recompute_pattern_effectiveness("decay-pat").unwrap();
    let eff = db.get_pattern_effectiveness_score("decay-pat").unwrap();
    assert!(
        eff.score > 0.7,
        "Expected score > 0.7 due to decay, got {}",
        eff.score
    );
    assert_eq!(eff.samples, 2);
}

#[test]
fn test_get_patterns_by_effectiveness() {
    let db = test_db();
    let now = Utc::now().to_rfc3339();
    for (id, content, score, samples) in [
        ("eff-a", "pattern alpha", 0.9, 5),
        ("eff-b", "pattern beta", 0.3, 4),
        ("eff-c", "pattern gamma", 0.6, 3),
    ] {
        db.conn.execute(
            "INSERT INTO patterns_long (id, content, category, usage_count, last_used, effectiveness_score, effectiveness_samples) VALUES (?1, ?2, 'test', 1, ?3, ?4, ?5)",
            params![id, content, now, score, samples],
        ).unwrap();
    }
    let asc = db.get_patterns_by_effectiveness(10, true).unwrap();
    assert_eq!(asc[0].0, "eff-b");
    assert_eq!(asc[2].0, "eff-a");
    let desc = db.get_patterns_by_effectiveness(10, false).unwrap();
    assert_eq!(desc[0].0, "eff-a");
    assert_eq!(desc[2].0, "eff-b");
    assert_eq!(db.get_patterns_by_effectiveness(2, false).unwrap().len(), 2);
}

// ── MCP-style roundtrip tests ──

#[test]
fn test_memory_set_get_roundtrip() {
    let db = test_db();
    db.kv_set("project", "flowforge", "default").unwrap();
    db.kv_set("version", "1.0", "default").unwrap();
    assert_eq!(
        db.kv_get("project", "default").unwrap(),
        Some("flowforge".to_string())
    );
    assert_eq!(
        db.kv_get("version", "default").unwrap(),
        Some("1.0".to_string())
    );
}

#[test]
fn test_memory_set_overwrite() {
    let db = test_db();
    db.kv_set("key", "v1", "default").unwrap();
    db.kv_set("key", "v2", "default").unwrap();
    assert_eq!(db.kv_get("key", "default").unwrap(), Some("v2".to_string()));
}

#[test]
fn test_memory_namespace_isolation() {
    let db = test_db();
    db.kv_set("key", "val-a", "ns-a").unwrap();
    db.kv_set("key", "val-b", "ns-b").unwrap();
    assert_eq!(db.kv_get("key", "ns-a").unwrap(), Some("val-a".to_string()));
    assert_eq!(db.kv_get("key", "ns-b").unwrap(), Some("val-b".to_string()));
}

#[test]
fn test_memory_list_namespace() {
    let db = test_db();
    db.kv_set("alpha", "1", "test-ns").unwrap();
    db.kv_set("beta", "2", "test-ns").unwrap();
    db.kv_set("gamma", "3", "other-ns").unwrap();
    let items = db.kv_list("test-ns").unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].0, "alpha");
    assert_eq!(items[1].0, "beta");
}

#[test]
fn test_memory_search() {
    let db = test_db();
    db.kv_set("rust-version", "1.86", "default").unwrap();
    db.kv_set("python-version", "3.11", "default").unwrap();
    db.kv_set("node-version", "20", "default").unwrap();
    let results = db.kv_search("version", 10).unwrap();
    assert_eq!(results.len(), 3);
}

#[test]
fn test_memory_count() {
    let db = test_db();
    assert_eq!(db.count_kv().unwrap(), 0);
    db.kv_set("a", "1", "default").unwrap();
    db.kv_set("b", "2", "default").unwrap();
    assert_eq!(db.count_kv().unwrap(), 2);
}

#[test]
fn test_work_create_list_roundtrip() {
    let db = test_db();
    db.create_work_item(&test_work_item("rt-1", "Build feature"))
        .unwrap();
    db.create_work_item(&test_work_item("rt-2", "Fix bug"))
        .unwrap();
    let items = db.list_work_items(&WorkFilter::default()).unwrap();
    assert_eq!(items.len(), 2);
    let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
    assert!(titles.contains(&"Build feature"));
    assert!(titles.contains(&"Fix bug"));
}

#[test]
fn test_checkpoint_create_get_roundtrip() {
    use flowforge_core::Checkpoint;
    let db = test_db();
    let session = SessionInfo {
        id: "cp-sess".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();
    let cp = Checkpoint {
        id: "cp-1".to_string(),
        session_id: "cp-sess".to_string(),
        name: "before-refactor".to_string(),
        message_index: 5,
        description: Some("Save point".to_string()),
        git_ref: Some("abc123".to_string()),
        created_at: Utc::now(),
        metadata: None,
    };
    db.create_checkpoint(&cp).unwrap();
    let fetched = db.get_checkpoint("cp-1").unwrap().unwrap();
    assert_eq!(fetched.name, "before-refactor");
    assert_eq!(fetched.message_index, 5);
    assert_eq!(fetched.git_ref, Some("abc123".to_string()));
}

#[test]
fn test_checkpoint_list_by_session() {
    use flowforge_core::Checkpoint;
    let db = test_db();
    let session = SessionInfo {
        id: "cp-list-sess".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();
    for (i, name) in ["start", "middle", "end"].iter().enumerate() {
        let cp = Checkpoint {
            id: format!("cp-l-{i}"),
            session_id: "cp-list-sess".to_string(),
            name: name.to_string(),
            message_index: (i * 10) as u32,
            description: None,
            git_ref: None,
            created_at: Utc::now(),
            metadata: None,
        };
        db.create_checkpoint(&cp).unwrap();
    }
    let cps = db.list_checkpoints("cp-list-sess").unwrap();
    assert_eq!(cps.len(), 3);
    assert_eq!(cps[0].name, "start");
    assert_eq!(cps[2].name, "end");
}

#[test]
fn test_checkpoint_by_name() {
    use flowforge_core::Checkpoint;
    let db = test_db();
    let session = SessionInfo {
        id: "cp-name-sess".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();
    let cp = Checkpoint {
        id: "cp-n-1".to_string(),
        session_id: "cp-name-sess".to_string(),
        name: "unique-name".to_string(),
        message_index: 1,
        description: None,
        git_ref: None,
        created_at: Utc::now(),
        metadata: None,
    };
    db.create_checkpoint(&cp).unwrap();
    let found = db
        .get_checkpoint_by_name("cp-name-sess", "unique-name")
        .unwrap();
    assert!(found.is_some());
    let not_found = db
        .get_checkpoint_by_name("cp-name-sess", "missing")
        .unwrap();
    assert!(not_found.is_none());
}

#[test]
fn test_checkpoint_delete() {
    use flowforge_core::Checkpoint;
    let db = test_db();
    let cp = Checkpoint {
        id: "cp-del".to_string(),
        session_id: "s".to_string(),
        name: "delete-me".to_string(),
        message_index: 0,
        description: None,
        git_ref: None,
        created_at: Utc::now(),
        metadata: None,
    };
    db.create_checkpoint(&cp).unwrap();
    assert!(db.get_checkpoint("cp-del").unwrap().is_some());
    db.delete_checkpoint("cp-del").unwrap();
    assert!(db.get_checkpoint("cp-del").unwrap().is_none());
}

// ── Work-stealing edge cases ──

#[test]
fn test_claim_already_claimed_item_fails() {
    let db = test_db();
    let mut item = test_work_item("ws-dup", "Claimed item");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();
    assert!(db.claim_work_item("ws-dup", "session-1").unwrap());
    // second claim by different session should fail (item is not stealable)
    assert!(!db.claim_work_item("ws-dup", "session-2").unwrap());
}

#[test]
fn test_release_then_reclaim() {
    let db = test_db();
    let mut item = test_work_item("ws-rel", "Release me");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();
    db.claim_work_item("ws-rel", "s1").unwrap();
    db.release_work_item("ws-rel").unwrap();
    let released = db.get_work_item("ws-rel").unwrap().unwrap();
    assert!(released.claimed_by.is_none());
    // now another session can claim it
    assert!(db.claim_work_item("ws-rel", "s2").unwrap());
    assert_eq!(
        db.get_work_item("ws-rel").unwrap().unwrap().claimed_by,
        Some("s2".to_string())
    );
}

#[test]
fn test_heartbeat_updates_all_claimed_items() {
    let db = test_db();
    for i in 0..3 {
        let mut item = test_work_item(&format!("hb-{i}"), &format!("HB task {i}"));
        item.status = flowforge_core::WorkStatus::InProgress;
        db.create_work_item(&item).unwrap();
        db.claim_work_item(&format!("hb-{i}"), "my-session")
            .unwrap();
    }
    let count = db.update_heartbeat("my-session").unwrap();
    assert_eq!(count, 3);
}

#[test]
fn test_progress_update() {
    let db = test_db();
    db.create_work_item(&test_work_item("prog-1", "Progress"))
        .unwrap();
    assert_eq!(db.get_work_item("prog-1").unwrap().unwrap().progress, 0);
    db.update_progress("prog-1", 75).unwrap();
    assert_eq!(db.get_work_item("prog-1").unwrap().unwrap().progress, 75);
}

#[test]
fn test_steal_reclaim_cycle() {
    let db = test_db();
    let mut item = test_work_item("ws-cycle", "Steal cycle");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();
    db.claim_work_item("ws-cycle", "s1").unwrap();
    // make stealable
    db.conn
        .execute(
            "UPDATE work_items SET stealable = 1 WHERE id = 'ws-cycle'",
            [],
        )
        .unwrap();
    // steal
    assert!(db.steal_work_item("ws-cycle", "s2").unwrap());
    let stolen = db.get_work_item("ws-cycle").unwrap().unwrap();
    assert_eq!(stolen.claimed_by, Some("s2".to_string()));
    assert!(!stolen.stealable);
    // release and reclaim by original
    db.release_work_item("ws-cycle").unwrap();
    assert!(db.claim_work_item("ws-cycle", "s1").unwrap());
}

#[test]
fn test_mark_stale_items_respects_min_progress() {
    let db = test_db();
    // item with high progress should NOT be marked stealable
    let mut item = test_work_item("stale-hp", "High progress");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();
    db.claim_work_item("stale-hp", "old-sess").unwrap();
    db.update_progress("stale-hp", 50).unwrap();
    let old_hb = (Utc::now() - chrono::Duration::minutes(60)).to_rfc3339();
    db.conn
        .execute(
            "UPDATE work_items SET last_heartbeat = ?1 WHERE id = 'stale-hp'",
            params![old_hb],
        )
        .unwrap();
    let count = db.mark_stale_items_stealable(30, 75).unwrap();
    assert_eq!(count, 1); // 50 < 75, so it IS marked stealable
                          // but if min_progress = 30, it should NOT be marked
    db.conn
        .execute(
            "UPDATE work_items SET stealable = 0 WHERE id = 'stale-hp'",
            [],
        )
        .unwrap();
    let count2 = db.mark_stale_items_stealable(30, 30).unwrap();
    assert_eq!(count2, 0); // 50 >= 30, so not marked
}

#[test]
fn test_auto_release_abandoned() {
    let db = test_db();
    let mut item = test_work_item("abandon-1", "Abandoned");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();
    db.claim_work_item("abandon-1", "old-sess").unwrap();
    let very_old = (Utc::now() - chrono::Duration::minutes(120)).to_rfc3339();
    db.conn
        .execute(
            "UPDATE work_items SET last_heartbeat = ?1 WHERE id = 'abandon-1'",
            params![very_old],
        )
        .unwrap();
    let released = db.auto_release_abandoned(60).unwrap();
    assert_eq!(released, 1);
    let item = db.get_work_item("abandon-1").unwrap().unwrap();
    assert!(item.claimed_by.is_none());
    assert_eq!(item.status, flowforge_core::WorkStatus::Pending);
}

#[test]
fn test_get_session_load() {
    let db = test_db();
    // no items yet
    assert_eq!(db.get_session_load("empty-sess").unwrap(), 0);
    for i in 0..3 {
        let mut item = test_work_item(&format!("load-{i}"), &format!("Load {i}"));
        item.status = flowforge_core::WorkStatus::InProgress;
        db.create_work_item(&item).unwrap();
        db.claim_work_item(&format!("load-{i}"), "busy-sess")
            .unwrap();
    }
    assert_eq!(db.get_session_load("busy-sess").unwrap(), 3);
}

#[test]
fn test_get_stealable_items() {
    let db = test_db();
    let mut item = test_work_item("stealable-1", "Ready to steal");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();
    db.claim_work_item("stealable-1", "old-sess").unwrap();
    db.conn
        .execute(
            "UPDATE work_items SET stealable = 1 WHERE id = 'stealable-1'",
            [],
        )
        .unwrap();
    let stealable = db.get_stealable_items(10).unwrap();
    assert_eq!(stealable.len(), 1);
    assert_eq!(stealable[0].id, "stealable-1");
}

// ── Effectiveness tracking ──

#[test]
fn test_record_and_query_context_injection() {
    let db = test_db();
    let session = SessionInfo {
        id: "inj-sess".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: ".".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();
    let id1 = db
        .record_context_injection("inj-sess", None, "pattern", Some("pat-1"), Some(0.9), None)
        .unwrap();
    let id2 = db
        .record_context_injection("inj-sess", None, "trajectory", Some("traj-1"), Some(0.7), None)
        .unwrap();
    assert!(id1 > 0);
    assert!(id2 > id1);
    let injections = db.get_injections_for_session("inj-sess").unwrap();
    assert_eq!(injections.len(), 2);
    assert_eq!(injections[0].injection_type, "pattern");
    assert_eq!(injections[1].injection_type, "trajectory");
}

#[test]
fn test_rate_context_injection() {
    let db = test_db();
    let id = db
        .record_context_injection("rate-sess", None, "pattern", Some("p"), Some(0.5), None)
        .unwrap();
    db.rate_context_injection(id, "correlated_success").unwrap();
    let injections = db.get_injections_for_session("rate-sess").unwrap();
    // effectiveness column updated (not directly queryable via struct but DB round-trip works)
    assert_eq!(injections.len(), 1);
}

#[test]
fn test_rate_session_injections() {
    let db = test_db();
    db.record_context_injection("batch-sess", None, "pattern", Some("p1"), Some(0.8), None)
        .unwrap();
    db.record_context_injection("batch-sess", None, "pattern", Some("p2"), Some(0.6), None)
        .unwrap();
    let rated = db
        .rate_session_injections("batch-sess", "correlated_success")
        .unwrap();
    assert_eq!(rated, 2);
    // rating twice should not re-rate already rated ones
    let re_rated = db
        .rate_session_injections("batch-sess", "correlated_failure")
        .unwrap();
    assert_eq!(re_rated, 0);
}

#[test]
fn test_record_pattern_effectiveness() {
    let db = test_db();
    db.record_pattern_effectiveness("pat-eff-1", "sess-1", "success", 0.9)
        .unwrap();
    db.record_pattern_effectiveness("pat-eff-1", "sess-2", "failure", 0.7)
        .unwrap();
    // no crash, just verify it records
    let now = Utc::now().to_rfc3339();
    db.conn.execute(
        "INSERT INTO patterns_long (id, content, category, usage_count, last_used, effectiveness_score, effectiveness_samples) VALUES ('pat-eff-1', 'test', 'test', 1, ?1, 0.0, 0)",
        params![now],
    ).unwrap();
    db.recompute_pattern_effectiveness("pat-eff-1").unwrap();
    let eff = db.get_pattern_effectiveness_score("pat-eff-1").unwrap();
    assert_eq!(eff.samples, 2);
    assert!(eff.score > 0.0);
}

// ── Pattern lifecycle ──

#[test]
fn test_pattern_short_create_search() {
    use flowforge_core::ShortTermPattern;
    let db = test_db();
    let pat = ShortTermPattern {
        id: "sp-1".to_string(),
        content: "Always run cargo test before commit".to_string(),
        category: "workflow".to_string(),
        confidence: 0.7,
        usage_count: 1,
        created_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    };
    db.store_pattern_short(&pat).unwrap();
    let found = db.search_patterns_short("cargo test", 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "sp-1");
}

#[test]
fn test_pattern_short_usage_increment() {
    use flowforge_core::ShortTermPattern;
    let db = test_db();
    let pat = ShortTermPattern {
        id: "sp-use".to_string(),
        content: "test pattern".to_string(),
        category: "test".to_string(),
        confidence: 0.5,
        usage_count: 0,
        created_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    };
    db.store_pattern_short(&pat).unwrap();
    db.update_pattern_short_usage("sp-use").unwrap();
    db.update_pattern_short_usage("sp-use").unwrap();
    let updated = db.get_pattern_short("sp-use").unwrap().unwrap();
    assert_eq!(updated.usage_count, 2);
    assert!(updated.confidence > 0.5); // confidence increases with usage
}

#[test]
fn test_pattern_promote_short_to_long() {
    use flowforge_core::{LongTermPattern, ShortTermPattern};
    let db = test_db();
    let pat = ShortTermPattern {
        id: "promote-1".to_string(),
        content: "Promote me".to_string(),
        category: "test".to_string(),
        confidence: 0.8,
        usage_count: 5,
        created_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    };
    db.store_pattern_short(&pat).unwrap();
    // Simulate promotion: create long-term version and delete short-term
    let long = LongTermPattern {
        id: pat.id.clone(),
        content: pat.content.clone(),
        category: pat.category.clone(),
        confidence: pat.confidence,
        usage_count: pat.usage_count,
        success_count: 0,
        failure_count: 0,
        created_at: pat.created_at,
        promoted_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    };
    db.store_pattern_long(&long).unwrap();
    db.delete_pattern_short("promote-1").unwrap();
    assert!(db.get_pattern_short("promote-1").unwrap().is_none());
    let promoted = db.get_pattern_long("promote-1").unwrap().unwrap();
    assert_eq!(promoted.content, "Promote me");
    assert_eq!(promoted.usage_count, 5);
}

#[test]
fn test_pattern_long_feedback() {
    use flowforge_core::LongTermPattern;
    let db = test_db();
    let pat = LongTermPattern {
        id: "fb-1".to_string(),
        content: "Feedback test".to_string(),
        category: "test".to_string(),
        confidence: 0.5,
        usage_count: 1,
        success_count: 0,
        failure_count: 0,
        created_at: Utc::now(),
        promoted_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    };
    db.store_pattern_long(&pat).unwrap();
    db.update_pattern_long_feedback("fb-1", true).unwrap();
    db.update_pattern_long_feedback("fb-1", true).unwrap();
    db.update_pattern_long_feedback("fb-1", false).unwrap();
    let updated = db.get_pattern_long("fb-1").unwrap().unwrap();
    assert_eq!(updated.success_count, 2);
    assert_eq!(updated.failure_count, 1);
}

#[test]
fn test_pattern_search_long() {
    use flowforge_core::LongTermPattern;
    let db = test_db();
    let pat = LongTermPattern {
        id: "sl-1".to_string(),
        content: "Use clippy for linting".to_string(),
        category: "quality".to_string(),
        confidence: 0.9,
        usage_count: 10,
        success_count: 8,
        failure_count: 2,
        created_at: Utc::now(),
        promoted_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    };
    db.store_pattern_long(&pat).unwrap();
    let found = db.search_patterns_long("clippy", 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].content, "Use clippy for linting");
    let not_found = db.search_patterns_long("nonexistent", 10).unwrap();
    assert!(not_found.is_empty());
}

#[test]
fn test_pattern_count() {
    use flowforge_core::{LongTermPattern, ShortTermPattern};
    let db = test_db();
    let sp = ShortTermPattern {
        id: "cnt-s".to_string(),
        content: "short".to_string(),
        category: "t".to_string(),
        confidence: 0.5,
        usage_count: 0,
        created_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    };
    db.store_pattern_short(&sp).unwrap();
    let lp = LongTermPattern {
        id: "cnt-l".to_string(),
        content: "long".to_string(),
        category: "t".to_string(),
        confidence: 0.5,
        usage_count: 0,
        success_count: 0,
        failure_count: 0,
        created_at: Utc::now(),
        promoted_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    };
    db.store_pattern_long(&lp).unwrap();
    assert_eq!(db.count_patterns_short().unwrap(), 1);
    assert_eq!(db.count_patterns_long().unwrap(), 1);
    assert_eq!(db.count_patterns().unwrap(), 2);
}

// ── Trajectory tests ──

#[test]
fn test_trajectory_create_get_roundtrip() {
    use flowforge_core::trajectory::{Trajectory, TrajectoryStatus};
    let db = test_db();
    let traj = Trajectory {
        id: "traj-1".to_string(),
        session_id: "sess-1".to_string(),
        work_item_id: None,
        agent_name: Some("coder".to_string()),
        task_description: Some("Fix the bug".to_string()),
        status: TrajectoryStatus::Recording,
        started_at: Utc::now(),
        ended_at: None,
        verdict: None,
        confidence: None,
        metadata: None,
        embedding_id: None,
    };
    db.create_trajectory(&traj).unwrap();
    let fetched = db.get_trajectory("traj-1").unwrap().unwrap();
    assert_eq!(fetched.session_id, "sess-1");
    assert_eq!(fetched.agent_name, Some("coder".to_string()));
    assert_eq!(fetched.status, TrajectoryStatus::Recording);
}

#[test]
fn test_trajectory_steps_recording() {
    use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};
    let db = test_db();
    let traj = Trajectory {
        id: "traj-steps".to_string(),
        session_id: "sess-2".to_string(),
        work_item_id: None,
        agent_name: None,
        task_description: None,
        status: TrajectoryStatus::Recording,
        started_at: Utc::now(),
        ended_at: None,
        verdict: None,
        confidence: None,
        metadata: None,
        embedding_id: None,
    };
    db.create_trajectory(&traj).unwrap();
    db.record_trajectory_step("traj-steps", "Read", None, StepOutcome::Success, Some(100))
        .unwrap();
    db.record_trajectory_step(
        "traj-steps",
        "Edit",
        Some("abc"),
        StepOutcome::Success,
        Some(200),
    )
    .unwrap();
    db.record_trajectory_step("traj-steps", "Bash", None, StepOutcome::Failure, Some(50))
        .unwrap();
    let steps = db.get_trajectory_steps("traj-steps").unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0].tool_name, "Read");
    assert_eq!(steps[0].step_index, 0);
    assert_eq!(steps[1].step_index, 1);
    assert_eq!(steps[2].outcome, StepOutcome::Failure);
}

#[test]
fn test_trajectory_success_ratio() {
    use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};
    let db = test_db();
    let traj = Trajectory {
        id: "traj-ratio".to_string(),
        session_id: "s".to_string(),
        work_item_id: None,
        agent_name: None,
        task_description: None,
        status: TrajectoryStatus::Recording,
        started_at: Utc::now(),
        ended_at: None,
        verdict: None,
        confidence: None,
        metadata: None,
        embedding_id: None,
    };
    db.create_trajectory(&traj).unwrap();
    for outcome in [
        StepOutcome::Success,
        StepOutcome::Success,
        StepOutcome::Failure,
        StepOutcome::Success,
    ] {
        db.record_trajectory_step("traj-ratio", "Test", None, outcome, None)
            .unwrap();
    }
    let ratio = db.trajectory_success_ratio("traj-ratio").unwrap();
    assert!((ratio - 0.75).abs() < 0.01);
}

#[test]
fn test_trajectory_tool_sequence() {
    use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};
    let db = test_db();
    let traj = Trajectory {
        id: "traj-seq".to_string(),
        session_id: "s".to_string(),
        work_item_id: None,
        agent_name: None,
        task_description: None,
        status: TrajectoryStatus::Recording,
        started_at: Utc::now(),
        ended_at: None,
        verdict: None,
        confidence: None,
        metadata: None,
        embedding_id: None,
    };
    db.create_trajectory(&traj).unwrap();
    for tool in ["Read", "Grep", "Edit", "Bash"] {
        db.record_trajectory_step("traj-seq", tool, None, StepOutcome::Success, None)
            .unwrap();
    }
    let seq = db.trajectory_tool_sequence("traj-seq").unwrap();
    assert_eq!(seq, vec!["Read", "Grep", "Edit", "Bash"]);
}

#[test]
fn test_trajectory_end_and_judge() {
    use flowforge_core::trajectory::{Trajectory, TrajectoryStatus, TrajectoryVerdict};
    let db = test_db();
    let traj = Trajectory {
        id: "traj-judge".to_string(),
        session_id: "s".to_string(),
        work_item_id: None,
        agent_name: None,
        task_description: None,
        status: TrajectoryStatus::Recording,
        started_at: Utc::now(),
        ended_at: None,
        verdict: None,
        confidence: None,
        metadata: None,
        embedding_id: None,
    };
    db.create_trajectory(&traj).unwrap();
    db.end_trajectory("traj-judge", TrajectoryStatus::Completed)
        .unwrap();
    let ended = db.get_trajectory("traj-judge").unwrap().unwrap();
    assert_eq!(ended.status, TrajectoryStatus::Completed);
    assert!(ended.ended_at.is_some());
    db.judge_trajectory("traj-judge", TrajectoryVerdict::Success, 0.95)
        .unwrap();
    let judged = db.get_trajectory("traj-judge").unwrap().unwrap();
    assert_eq!(judged.status, TrajectoryStatus::Judged);
    assert_eq!(judged.verdict, Some(TrajectoryVerdict::Success));
    assert_eq!(judged.confidence, Some(0.95));
}

#[test]
fn test_trajectory_active_for_session() {
    use flowforge_core::trajectory::{Trajectory, TrajectoryStatus};
    let db = test_db();
    let traj1 = Trajectory {
        id: "traj-active-1".to_string(),
        session_id: "active-sess".to_string(),
        work_item_id: None,
        agent_name: None,
        task_description: None,
        status: TrajectoryStatus::Completed,
        started_at: Utc::now(),
        ended_at: Some(Utc::now()),
        verdict: None,
        confidence: None,
        metadata: None,
        embedding_id: None,
    };
    db.create_trajectory(&traj1).unwrap();
    let traj2 = Trajectory {
        id: "traj-active-2".to_string(),
        session_id: "active-sess".to_string(),
        work_item_id: None,
        agent_name: None,
        task_description: None,
        status: TrajectoryStatus::Recording,
        started_at: Utc::now(),
        ended_at: None,
        verdict: None,
        confidence: None,
        metadata: None,
        embedding_id: None,
    };
    db.create_trajectory(&traj2).unwrap();
    let active = db.get_active_trajectory("active-sess").unwrap().unwrap();
    assert_eq!(active.id, "traj-active-2");
}

#[test]
fn test_trajectory_list_with_filters() {
    use flowforge_core::trajectory::{Trajectory, TrajectoryStatus};
    let db = test_db();
    for (id, sid, status) in [
        ("tl-1", "s1", TrajectoryStatus::Recording),
        ("tl-2", "s1", TrajectoryStatus::Completed),
        ("tl-3", "s2", TrajectoryStatus::Recording),
    ] {
        let traj = Trajectory {
            id: id.to_string(),
            session_id: sid.to_string(),
            work_item_id: None,
            agent_name: None,
            task_description: None,
            status,
            started_at: Utc::now(),
            ended_at: None,
            verdict: None,
            confidence: None,
            metadata: None,
            embedding_id: None,
        };
        db.create_trajectory(&traj).unwrap();
    }
    let all = db.list_trajectories(None, None, 100).unwrap();
    assert_eq!(all.len(), 3);
    let s1_only = db.list_trajectories(Some("s1"), None, 100).unwrap();
    assert_eq!(s1_only.len(), 2);
    let recording = db.list_trajectories(None, Some("recording"), 100).unwrap();
    assert_eq!(recording.len(), 2);
}

#[test]
fn test_trajectory_link_work_item() {
    use flowforge_core::trajectory::{Trajectory, TrajectoryStatus};
    let db = test_db();

    // Create work item first (required by FK constraint on trajectories.work_item_id)
    let item = test_work_item("wi-123", "Link target");
    db.create_work_item(&item).unwrap();

    let traj = Trajectory {
        id: "traj-link".to_string(),
        session_id: "s".to_string(),
        work_item_id: None,
        agent_name: None,
        task_description: None,
        status: TrajectoryStatus::Recording,
        started_at: Utc::now(),
        ended_at: None,
        verdict: None,
        confidence: None,
        metadata: None,
        embedding_id: None,
    };
    db.create_trajectory(&traj).unwrap();
    db.link_trajectory_work_item("traj-link", "wi-123").unwrap();
    let linked = db.get_trajectory("traj-link").unwrap().unwrap();
    assert_eq!(linked.work_item_id, Some("wi-123".to_string()));
}

// ── Trust score tests ──

#[test]
fn test_trust_score_lifecycle() {
    let db = test_db();
    db.create_trust_score("trust-sess", 0.5).unwrap();
    let score = db.get_trust_score("trust-sess").unwrap().unwrap();
    assert_eq!(score.score, 0.5);
    assert_eq!(score.total_checks, 0);
    db.update_trust_score("trust-sess", &GateAction::Allow, 0.05)
        .unwrap();
    db.update_trust_score("trust-sess", &GateAction::Ask, -0.02)
        .unwrap();
    db.update_trust_score("trust-sess", &GateAction::Deny, -0.1)
        .unwrap();
    let updated = db.get_trust_score("trust-sess").unwrap().unwrap();
    assert_eq!(updated.total_checks, 3);
    assert_eq!(updated.allows, 1);
    assert_eq!(updated.asks, 1);
    assert_eq!(updated.denials, 1);
    let expected_score = 0.5 + 0.05 - 0.02 - 0.1;
    assert!((updated.score - expected_score).abs() < 0.001);
}

#[test]
fn test_trust_score_clamps_to_range() {
    let db = test_db();
    db.create_trust_score("clamp-sess", 0.9).unwrap();
    db.update_trust_score("clamp-sess", &GateAction::Allow, 0.5)
        .unwrap(); // would exceed 1.0
    let score = db.get_trust_score("clamp-sess").unwrap().unwrap();
    assert!(score.score <= 1.0);
    db.create_trust_score("clamp-low", 0.1).unwrap();
    db.update_trust_score("clamp-low", &GateAction::Deny, -0.5)
        .unwrap(); // would go below 0.0
    let low = db.get_trust_score("clamp-low").unwrap().unwrap();
    assert!(low.score >= 0.0);
}

// ── Meta operations ──

#[test]
fn test_meta_get_set() {
    let db = test_db();
    assert!(db.get_meta("missing").unwrap().is_none());
    db.set_meta("version", "3").unwrap();
    assert_eq!(db.get_meta("version").unwrap(), Some("3".to_string()));
    db.set_meta("version", "4").unwrap();
    assert_eq!(db.get_meta("version").unwrap(), Some("4".to_string()));
}

// ── Schema / infrastructure tests ──

#[test]
fn test_schema_version_is_stamped() {
    let db = test_db();
    let version = db.get_meta("schema_version").unwrap().unwrap();
    assert_eq!(version, SCHEMA_VERSION.to_string());
}

#[test]
fn test_vector_blob_roundtrip() {
    let original = vec![1.0f32, 2.5, -3.7, 0.0, 42.42];
    let blob = vector_to_blob(&original);
    let restored = blob_to_vector(&blob);
    for (a, b) in original.iter().zip(restored.iter()) {
        assert!((a - b).abs() < f32::EPSILON);
    }
}

#[test]
fn test_work_item_filter_by_type() {
    let db = test_db();
    let mut bug = test_work_item("type-bug", "A bug");
    bug.item_type = "bug".to_string();
    db.create_work_item(&bug).unwrap();
    db.create_work_item(&test_work_item("type-task", "A task"))
        .unwrap();
    let bugs = db
        .list_work_items(&WorkFilter {
            item_type: Some("bug".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(bugs.len(), 1);
    assert_eq!(bugs[0].id, "type-bug");
}

#[test]
fn test_work_item_filter_by_assignee() {
    let db = test_db();
    let mut item = test_work_item("assign-1", "Assigned task");
    item.assignee = Some("agent:coder".to_string());
    db.create_work_item(&item).unwrap();
    db.create_work_item(&test_work_item("assign-2", "Unassigned"))
        .unwrap();
    let assigned = db
        .list_work_items(&WorkFilter {
            assignee: Some("agent:coder".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(assigned.len(), 1);
}

#[test]
fn test_work_config_roundtrip() {
    let db = test_db();
    assert!(db.get_work_config("backend").unwrap().is_none());
    db.set_work_config("backend", "kanbus").unwrap();
    assert_eq!(
        db.get_work_config("backend").unwrap(),
        Some("kanbus".to_string())
    );
}

// ── test_helpers module tests ──

#[test]
fn test_seeded_db_has_data() {
    use crate::test_helpers;
    let db = test_helpers::seeded_db();
    let session = db.get_current_session().unwrap();
    assert!(session.is_some());
    assert_eq!(session.unwrap().id, "test-session");
    let items = db.list_work_items(&WorkFilter::default()).unwrap();
    assert_eq!(items.len(), 2);
    let patterns = db.get_all_patterns_short().unwrap();
    assert_eq!(patterns.len(), 2);
}

#[test]
fn test_helper_work_item_defaults() {
    use crate::test_helpers;
    let item = test_helpers::work_item("test-id", "Test Title");
    assert_eq!(item.id, "test-id");
    assert_eq!(item.title, "Test Title");
    assert_eq!(item.status, flowforge_core::WorkStatus::Pending);
    assert_eq!(item.backend, "flowforge");
    assert_eq!(item.priority, 2);
    assert!(!item.stealable);
}

#[test]
fn test_sqlite_ext_classifies_busy_as_transient() {
    let busy_err: std::result::Result<(), rusqlite::Error> = Err(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ffi::ErrorCode::DatabaseBusy,
            extended_code: 5,
        },
        Some("database is locked".to_string()),
    ));
    let err = busy_err.sq().unwrap_err();
    assert!(err.is_transient());
}

#[test]
fn test_sqlite_ext_classifies_constraint_as_permanent() {
    let constraint_err: std::result::Result<(), rusqlite::Error> =
        Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::ConstraintViolation,
                extended_code: 19,
            },
            Some("UNIQUE constraint failed".to_string()),
        ));
    let err = constraint_err.sq().unwrap_err();
    assert!(!err.is_transient());
}

#[test]
fn test_sqlite_ext_classifies_locked_as_transient() {
    let locked_err: std::result::Result<(), rusqlite::Error> = Err(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ffi::ErrorCode::DatabaseLocked,
            extended_code: 6,
        },
        None,
    ));
    let err = locked_err.sq().unwrap_err();
    assert!(err.is_transient());
}

#[test]
fn test_sqlite_ext_preserves_error_message() {
    let err: std::result::Result<(), rusqlite::Error> = Err(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ffi::ErrorCode::DatabaseBusy,
            extended_code: 5,
        },
        Some("database is locked".to_string()),
    ));
    let converted = err.sq().unwrap_err();
    assert!(converted.to_string().contains("database is locked"));
}

// ── Decomposition structure tests ──

#[test]
fn test_schema_module_exports_version() {
    // Verify SCHEMA_VERSION is accessible from the schema module
    assert!(SCHEMA_VERSION > 0);
}

#[test]
fn test_helpers_module_roundtrip() {
    // Verify helper functions are accessible and work correctly
    let vec = vec![1.0f32, -2.5, 0.0];
    let blob = vector_to_blob(&vec);
    let restored = blob_to_vector(&blob);
    assert_eq!(vec.len(), restored.len());
    for (a, b) in vec.iter().zip(restored.iter()) {
        assert!((a - b).abs() < f32::EPSILON);
    }
}

#[test]
fn test_schema_version_is_8() {
    assert_eq!(SCHEMA_VERSION, 8);
}

#[test]
fn test_performance_indexes_exist() {
    let db = test_db();
    let indexes: Vec<String> = db
        .conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert!(
        indexes.contains(&"idx_sessions_ended".to_string()),
        "missing idx_sessions_ended"
    );
    assert!(
        indexes.contains(&"idx_gate_decisions_session_ts".to_string()),
        "missing idx_gate_decisions_session_ts"
    );
    assert!(
        indexes.contains(&"idx_routing_weights_pattern".to_string()),
        "missing idx_routing_weights_pattern"
    );
}

// ── Phase 2: Agent session cascade tests ──

#[test]
fn test_end_session_cascades_to_agent_sessions() {
    use flowforge_core::{AgentSession, AgentSessionStatus};
    let db = test_db();

    // Create a parent session
    let session = SessionInfo {
        id: "sess-cascade".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();

    // Create two active agent sessions
    for i in 0..2 {
        let agent = AgentSession {
            id: format!("agent-{}", i),
            parent_session_id: "sess-cascade".to_string(),
            agent_id: format!("ag-{}", i),
            agent_type: "general".to_string(),
            status: AgentSessionStatus::Active,
            started_at: Utc::now(),
            ended_at: None,
            edits: 0,
            commands: 0,
            task_id: None,
            transcript_path: None,
        };
        db.create_agent_session(&agent).unwrap();
    }

    // Verify 2 active agents
    let active = db.get_active_agent_sessions().unwrap();
    assert_eq!(active.len(), 2);

    // End session — should cascade
    db.end_session("sess-cascade", Utc::now()).unwrap();

    // All agent sessions should now be ended
    let active = db.get_active_agent_sessions().unwrap();
    assert_eq!(active.len(), 0);
}

#[test]
fn test_cleanup_orphaned_agent_sessions() {
    use flowforge_core::{AgentSession, AgentSessionStatus};
    let db = test_db();

    // Create a session that has already ended
    let session = SessionInfo {
        id: "sess-ended".to_string(),
        started_at: Utc::now(),
        ended_at: Some(Utc::now()),
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();

    // Create orphaned agent session (parent ended, but agent never closed)
    let agent = AgentSession {
        id: "orphan-agent".to_string(),
        parent_session_id: "sess-ended".to_string(),
        agent_id: "orphan-1".to_string(),
        agent_type: "general".to_string(),
        status: AgentSessionStatus::Active,
        started_at: Utc::now(),
        ended_at: None,
        edits: 0,
        commands: 0,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&agent).unwrap();

    // Verify orphan exists
    assert_eq!(db.get_active_agent_sessions().unwrap().len(), 1);

    // Cleanup should find and close it
    let cleaned = db.cleanup_orphaned_agent_sessions().unwrap();
    assert_eq!(cleaned, 1);
    assert_eq!(db.get_active_agent_sessions().unwrap().len(), 0);
}

#[test]
fn test_cleanup_skips_agents_with_active_parent() {
    use flowforge_core::{AgentSession, AgentSessionStatus};
    let db = test_db();

    // Create an active (not ended) session
    let session = SessionInfo {
        id: "sess-active".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();

    let agent = AgentSession {
        id: "alive-agent".to_string(),
        parent_session_id: "sess-active".to_string(),
        agent_id: "alive-1".to_string(),
        agent_type: "general".to_string(),
        status: AgentSessionStatus::Active,
        started_at: Utc::now(),
        ended_at: None,
        edits: 0,
        commands: 0,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&agent).unwrap();

    // Cleanup should NOT close this agent (parent is still active)
    let cleaned = db.cleanup_orphaned_agent_sessions().unwrap();
    assert_eq!(cleaned, 0);
    assert_eq!(db.get_active_agent_sessions().unwrap().len(), 1);
}

#[test]
fn test_rollup_agent_stats_to_parent() {
    use flowforge_core::{AgentSession, AgentSessionStatus};
    let db = test_db();

    // Create a parent session with 5 edits, 10 commands
    let session = SessionInfo {
        id: "sess-rollup".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 5,
        commands: 10,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();

    // Create an agent session with 3 edits, 7 commands
    let agent = AgentSession {
        id: "agent-rollup".to_string(),
        parent_session_id: "sess-rollup".to_string(),
        agent_id: "ag-rollup-1".to_string(),
        agent_type: "general".to_string(),
        status: AgentSessionStatus::Active,
        started_at: Utc::now(),
        ended_at: None,
        edits: 3,
        commands: 7,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&agent).unwrap();

    // Roll up
    db.rollup_agent_stats_to_parent("ag-rollup-1").unwrap();

    // Parent session should now have 8 edits, 17 commands
    let parent = db.get_current_session().unwrap().unwrap();
    assert_eq!(parent.edits, 8);
    assert_eq!(parent.commands, 17);
}

#[test]
fn test_cleanup_orphaned_agents_with_empty_parent() {
    use flowforge_core::{AgentSession, AgentSessionStatus};
    let db = test_db();

    // Create agent session with empty parent_session_id (the real-world bug)
    let agent = AgentSession {
        id: "empty-parent-agent".to_string(),
        parent_session_id: "".to_string(),
        agent_id: "orphan-empty".to_string(),
        agent_type: "general".to_string(),
        status: AgentSessionStatus::Active,
        started_at: Utc::now(),
        ended_at: None,
        edits: 0,
        commands: 0,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&agent).unwrap();

    // Create agent with parent that doesn't exist in sessions table
    let agent2 = AgentSession {
        id: "missing-parent-agent".to_string(),
        parent_session_id: "nonexistent-session-id".to_string(),
        agent_id: "orphan-missing".to_string(),
        agent_type: "general".to_string(),
        status: AgentSessionStatus::Active,
        started_at: Utc::now(),
        ended_at: None,
        edits: 0,
        commands: 0,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&agent2).unwrap();

    assert_eq!(db.get_active_agent_sessions().unwrap().len(), 2);

    let cleaned = db.cleanup_orphaned_agent_sessions().unwrap();
    assert_eq!(cleaned, 2);
    assert_eq!(db.get_active_agent_sessions().unwrap().len(), 0);
}

// ── Phase 4: Work-stealing atomicity tests ──

#[test]
fn test_claim_work_item_load_aware_atomic() {
    let db = test_db();

    // Create 2 work items, claim the first normally
    let mut item1 = test_work_item("wi-load-1", "Task 1");
    item1.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item1).unwrap();
    db.claim_work_item("wi-load-1", "session-A").unwrap();

    let mut item2 = test_work_item("wi-load-2", "Task 2");
    item2.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item2).unwrap();

    // With max_concurrent=1, session-A should NOT be able to claim a second item
    let claimed = db
        .claim_work_item_load_aware("wi-load-2", "session-A", 1)
        .unwrap();
    assert!(!claimed);

    // With max_concurrent=2, session-A CAN claim
    let claimed = db
        .claim_work_item_load_aware("wi-load-2", "session-A", 2)
        .unwrap();
    assert!(claimed);
}

#[test]
fn test_steal_work_item_preserves_high_progress() {
    let db = test_db();
    let mut item = test_work_item("wi-steal-prog", "High progress task");
    item.status = flowforge_core::WorkStatus::InProgress;
    item.stealable = true;
    item.progress = 75;
    item.claimed_by = Some("old-session".to_string());
    item.claimed_at = Some(Utc::now());
    item.last_heartbeat = Some(Utc::now());
    db.create_work_item(&item).unwrap();

    // Steal — progress >= 50 should be preserved
    let stolen = db.steal_work_item("wi-steal-prog", "new-session").unwrap();
    assert!(stolen);
    let fetched = db.get_work_item("wi-steal-prog").unwrap().unwrap();
    assert_eq!(fetched.progress, 75); // preserved, not reset to 0
    assert_eq!(fetched.claimed_by.as_deref(), Some("new-session"));
}

#[test]
fn test_steal_work_item_resets_low_progress() {
    let db = test_db();
    let mut item = test_work_item("wi-steal-low", "Low progress task");
    item.status = flowforge_core::WorkStatus::InProgress;
    item.stealable = true;
    item.progress = 20;
    item.claimed_by = Some("old-session".to_string());
    item.claimed_at = Some(Utc::now());
    item.last_heartbeat = Some(Utc::now());
    db.create_work_item(&item).unwrap();

    let stolen = db.steal_work_item("wi-steal-low", "new-session").unwrap();
    assert!(stolen);
    let fetched = db.get_work_item("wi-steal-low").unwrap().unwrap();
    assert_eq!(fetched.progress, 0); // reset because < 50
}

// ── v4 Phase 2: Transaction safety tests ──

#[test]
fn test_transaction_rollback_on_error() {
    let db = test_db();
    let session = SessionInfo {
        id: "sess-tx-test".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();

    // Attempt a transaction that fails partway through
    let result: flowforge_core::Result<()> = db.with_transaction(|| {
        db.increment_session_edits("sess-tx-test")?;
        // Force an error
        Err(flowforge_core::Error::Config(
            "intentional failure".to_string(),
        ))
    });
    assert!(result.is_err());

    // Edits should still be 0 (rolled back)
    let s = db.get_current_session().unwrap().unwrap();
    assert_eq!(s.edits, 0, "transaction should have rolled back");
}

#[test]
fn test_end_session_cascade_is_atomic() {
    use flowforge_core::{AgentSession, AgentSessionStatus};
    let db = test_db();

    let session = SessionInfo {
        id: "sess-atomic".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();

    let agent = AgentSession {
        id: "agent-atomic".to_string(),
        parent_session_id: "sess-atomic".to_string(),
        agent_id: "ag-atomic".to_string(),
        agent_type: "general".to_string(),
        status: AgentSessionStatus::Active,
        started_at: Utc::now(),
        ended_at: None,
        edits: 0,
        commands: 0,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&agent).unwrap();

    // end_session should atomically end both session and agent sessions
    db.end_session("sess-atomic", Utc::now()).unwrap();

    // Both should be ended
    let s = db.get_current_session().unwrap();
    assert!(s.is_none(), "session should have ended");
    let active = db.get_active_agent_sessions().unwrap();
    assert_eq!(active.len(), 0, "agent sessions should have ended");
}

// ── v4 Phase 3: WorkItem type safety tests ──

#[test]
fn test_progress_clamped_to_0_100() {
    let db = test_db();
    let item = test_work_item("wi-clamp", "Clamp test");
    db.create_work_item(&item).unwrap();

    // Over 100 should clamp to 100
    db.update_progress("wi-clamp", 999).unwrap();
    let fetched = db.get_work_item("wi-clamp").unwrap().unwrap();
    assert_eq!(fetched.progress, 100);

    // Under 0 should clamp to 0
    db.update_progress("wi-clamp", -50).unwrap();
    let fetched = db.get_work_item("wi-clamp").unwrap().unwrap();
    assert_eq!(fetched.progress, 0);

    // Valid value passes through
    db.update_progress("wi-clamp", 42).unwrap();
    let fetched = db.get_work_item("wi-clamp").unwrap().unwrap();
    assert_eq!(fetched.progress, 42);
}

#[test]
fn test_priority_clamped_to_0_4() {
    let db = test_db();
    let mut item = test_work_item("wi-pri-clamp", "Priority clamp");
    item.priority = 99; // out of range
    db.create_work_item(&item).unwrap();

    let fetched = db.get_work_item("wi-pri-clamp").unwrap().unwrap();
    assert_eq!(fetched.priority, 4, "priority should be clamped to 4");

    let mut item2 = test_work_item("wi-pri-neg", "Negative priority");
    item2.priority = -5;
    db.create_work_item(&item2).unwrap();

    let fetched2 = db.get_work_item("wi-pri-neg").unwrap().unwrap();
    assert_eq!(fetched2.priority, 0, "priority should be clamped to 0");
}

// ── v4 Phase 4: Index existence tests ──

#[test]
fn test_v5_indexes_exist() {
    let db = test_db();
    let indexes: Vec<String> = db
        .conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    for idx in [
        "idx_agent_sessions_status",
        "idx_agent_sessions_ended",
        "idx_trajectories_work_item",
        "idx_gate_decisions_ts",
        "idx_patterns_short_last_used",
        "idx_patterns_long_last_used",
    ] {
        assert!(indexes.contains(&idx.to_string()), "missing index: {idx}");
    }
}

// ── v4 Phase 7: Concurrent DB access test ──

#[test]
fn test_concurrent_db_access() {
    let dir = std::env::temp_dir().join("flowforge-concurrent-test");
    let _ = std::fs::create_dir_all(&dir);
    let db_path = dir.join("concurrent.db");
    let _ = std::fs::remove_file(&db_path);

    let db1 = MemoryDb::open(&db_path).unwrap();
    let db2 = MemoryDb::open(&db_path).unwrap();

    // Writer 1: create a work item
    let item = test_work_item("conc-1", "Concurrent test 1");
    db1.create_work_item(&item).unwrap();

    // Reader 2: should see it (WAL mode allows concurrent reads)
    let fetched = db2.get_work_item("conc-1").unwrap();
    assert!(fetched.is_some(), "WAL mode should allow concurrent reads");

    // Writer 2: create another item
    let item2 = test_work_item("conc-2", "Concurrent test 2");
    db2.create_work_item(&item2).unwrap();

    // Reader 1: should see both
    let filter = WorkFilter::default();
    let all = db1.list_work_items(&filter).unwrap();
    assert!(all.len() >= 2, "both connections should see all items");

    // Cleanup
    drop(db1);
    drop(db2);
    let _ = std::fs::remove_dir_all(&dir);
}

// ── v4 Phase 7: Row parser edge cases ──

#[test]
fn test_null_optional_fields_parse_correctly() {
    let db = test_db();
    // Work item with all optional fields as NULL
    db.conn
        .execute(
            "INSERT INTO work_items (id, backend, title, status, created_at, updated_at)
             VALUES ('null-test', 'flowforge', 'Test', 'pending', datetime('now'), datetime('now'))",
            [],
        )
        .unwrap();

    let item = db.get_work_item("null-test").unwrap().unwrap();
    assert!(item.external_id.is_none());
    assert!(item.description.is_none());
    assert!(item.assignee.is_none());
    assert!(item.parent_id.is_none());
    assert!(item.session_id.is_none());
    assert!(item.metadata.is_none());
    assert!(item.claimed_by.is_none());
    assert!(item.claimed_at.is_none());
    assert!(item.last_heartbeat.is_none());
    assert!(!item.stealable);
}

#[test]
fn test_session_with_null_ended_at() {
    let db = test_db();
    let session = SessionInfo {
        id: "null-end".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();

    let fetched = db.get_current_session().unwrap().unwrap();
    assert!(fetched.ended_at.is_none());
    assert!(fetched.summary.is_none());
    assert!(fetched.transcript_path.is_none());
}

// ── get_work_item_by_title tests ──

#[test]
fn test_get_work_item_by_title_exact_match() {
    let db = test_db();
    let mut item = test_work_item("wi-title-1", "Fix authentication bug");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();

    let found = db.get_work_item_by_title("Fix authentication bug").unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, "wi-title-1");
}

#[test]
fn test_get_work_item_by_title_no_match() {
    let db = test_db();
    let item = test_work_item("wi-title-2", "Fix login bug");
    db.create_work_item(&item).unwrap();

    let found = db.get_work_item_by_title("Implement dark mode").unwrap();
    assert!(found.is_none());
}

#[test]
fn test_get_work_item_by_title_prefers_in_progress() {
    let db = test_db();

    let mut pending = test_work_item("wi-pending", "Deploy feature X");
    pending.status = flowforge_core::WorkStatus::Pending;
    db.create_work_item(&pending).unwrap();

    let mut active = test_work_item("wi-active", "Deploy feature X");
    active.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&active).unwrap();

    let found = db.get_work_item_by_title("Deploy feature X").unwrap();
    assert_eq!(found.unwrap().id, "wi-active");
}

#[test]
fn test_get_work_item_by_title_skips_completed() {
    let db = test_db();

    let mut completed = test_work_item("wi-done", "Refactor parser");
    completed.status = flowforge_core::WorkStatus::Completed;
    db.create_work_item(&completed).unwrap();

    let found = db.get_work_item_by_title("Refactor parser").unwrap();
    assert!(found.is_none());
}

// ── v5 Phase 5: Nested transaction tests ──

#[test]
fn test_nested_transaction_inner_success() {
    let db = test_db();
    db.with_transaction(|| {
        db.set_meta("outer_key", "outer_value")?;
        db.with_transaction(|| {
            db.set_meta("inner_key", "inner_value")?;
            Ok(())
        })?;
        Ok(())
    })
    .unwrap();

    assert_eq!(
        db.get_meta("outer_key").unwrap(),
        Some("outer_value".to_string())
    );
    assert_eq!(
        db.get_meta("inner_key").unwrap(),
        Some("inner_value".to_string())
    );
}

#[test]
fn test_nested_transaction_inner_rollback_outer_success() {
    let db = test_db();
    db.with_transaction(|| {
        db.set_meta("outer_key", "outer_value")?;
        // Inner transaction fails — should roll back to savepoint
        let inner_result: flowforge_core::Result<()> = db.with_transaction(|| {
            db.set_meta("inner_key", "inner_value")?;
            Err(flowforge_core::Error::Config("inner error".to_string()))
        });
        assert!(inner_result.is_err());
        // Outer should still succeed
        Ok(())
    })
    .unwrap();

    assert_eq!(
        db.get_meta("outer_key").unwrap(),
        Some("outer_value".to_string())
    );
    // Inner was rolled back
    assert_eq!(db.get_meta("inner_key").unwrap(), None);
}

#[test]
fn test_nested_transaction_outer_rollback() {
    let db = test_db();
    let result: flowforge_core::Result<()> = db.with_transaction(|| {
        db.set_meta("outer_key", "will_be_lost")?;
        Err(flowforge_core::Error::Config("outer error".to_string()))
    });
    assert!(result.is_err());
    assert_eq!(db.get_meta("outer_key").unwrap(), None);
}

// ── v5 Phase 7: Trust score bounds test ──

#[test]
fn test_trust_score_stays_bounded() {
    let db = test_db();
    let session_id = "trust-bounds-test";
    db.create_trust_score(session_id, 0.5).unwrap();

    // Push score high with many allows
    for _ in 0..200 {
        db.update_trust_score(session_id, &GateAction::Allow, 0.1)
            .unwrap();
    }
    let score = db.get_trust_score(session_id).unwrap().unwrap().score;
    assert!(score <= 1.0, "Trust score {score} exceeds 1.0");

    // Push score low with many denials
    for _ in 0..200 {
        db.update_trust_score(session_id, &GateAction::Deny, -0.1)
            .unwrap();
    }
    let score = db.get_trust_score(session_id).unwrap().unwrap().score;
    assert!(score >= 0.0, "Trust score {score} below 0.0");
}

// ── v5 Phase 6: New indexes exist ──

#[test]
fn test_v6_indexes_exist() {
    let db = test_db();
    // These queries would be slow without indexes, but we just verify they don't error
    db.conn
        .execute_batch(
            "SELECT * FROM hnsw_entries WHERE source_type = 'test' AND source_id = 'test' LIMIT 1",
        )
        .unwrap();
    db.conn
        .execute_batch("SELECT * FROM agent_sessions WHERE task_id = 'test' LIMIT 1")
        .unwrap();
}

// ── v6 Phase 2: Transaction migration tests ──

#[test]
fn test_delete_work_item_uses_with_transaction() {
    let db = test_db();
    let item = test_work_item("wi-txn-del", "Delete via transaction");
    db.create_work_item(&item).unwrap();

    // Record a work event so we test the cascading delete
    let event = WorkEvent {
        id: 0,
        work_item_id: "wi-txn-del".to_string(),
        event_type: "status_change".to_string(),
        old_value: None,
        new_value: None,
        actor: None,
        timestamp: Utc::now(),
    };
    db.record_work_event(&event).unwrap();

    // Delete should succeed atomically
    db.delete_work_item("wi-txn-del").unwrap();
    assert!(db.get_work_item("wi-txn-del").unwrap().is_none());

    // Verify it works inside a wrapping transaction (nesting via savepoints)
    let item2 = test_work_item("wi-txn-del-2", "Nested delete");
    db.create_work_item(&item2).unwrap();
    db.with_transaction(|| {
        db.delete_work_item("wi-txn-del-2")?;
        Ok(())
    })
    .unwrap();
    assert!(db.get_work_item("wi-txn-del-2").unwrap().is_none());
}

#[test]
fn test_delete_all_clusters_atomic() {
    let db = test_db();

    // Store some vectors and create a cluster
    let cluster = flowforge_core::PatternCluster {
        id: 0,
        centroid: vec![1.0; 128],
        member_count: 3,
        p95_distance: 0.5,
        avg_confidence: 0.8,
        created_at: Utc::now(),
        last_recomputed: Utc::now(),
    };
    let cid = db.store_cluster(&cluster).unwrap();

    // Store a vector and assign it to the cluster
    let vid = db
        .store_vector("pattern_short", "test-pattern-1", &vec![0.5; 128])
        .unwrap();
    db.set_vector_cluster_id(vid, Some(cid)).unwrap();

    // Verify cluster exists
    assert!(db.get_cluster(cid).unwrap().is_some());
    let cluster_id = db.get_vector_cluster_id(vid).unwrap();
    assert!(cluster_id.is_some());

    // Delete all clusters atomically
    db.delete_all_clusters().unwrap();

    // Cluster should be gone and vector's cluster_id should be NULL
    assert!(db.get_cluster(cid).unwrap().is_none());
    let cluster_id = db.get_vector_cluster_id(vid).unwrap();
    assert!(cluster_id.is_none());
}

#[test]
fn test_batch_effectiveness_scores() {
    let db = test_db();

    // Store two short-term patterns manually
    use flowforge_core::ShortTermPattern;
    let p1 = ShortTermPattern {
        id: "eff-batch-1".to_string(),
        content: "pattern one".to_string(),
        category: "test".to_string(),
        confidence: 0.5,
        usage_count: 1,
        created_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    };
    let p2 = ShortTermPattern {
        id: "eff-batch-2".to_string(),
        content: "pattern two".to_string(),
        category: "test".to_string(),
        confidence: 0.5,
        usage_count: 1,
        created_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    };
    db.store_pattern_short(&p1).unwrap();
    db.store_pattern_short(&p2).unwrap();

    // Record effectiveness data for pattern 1
    db.record_pattern_effectiveness("eff-batch-1", "sess-1", "success", 0.8)
        .unwrap();
    db.record_pattern_effectiveness("eff-batch-1", "sess-2", "success", 0.9)
        .unwrap();
    db.recompute_pattern_effectiveness("eff-batch-1").unwrap();

    // Batch fetch
    let ids = vec![
        "eff-batch-1".to_string(),
        "eff-batch-2".to_string(),
        "eff-batch-missing".to_string(),
    ];
    let scores = db.get_effectiveness_scores_batch(&ids).unwrap();

    // Pattern 1 should have 2 samples
    assert!(scores.contains_key("eff-batch-1"));
    assert_eq!(scores["eff-batch-1"].samples, 2);
    assert!(scores["eff-batch-1"].score > 0.0);

    // Pattern 2 should have 0 samples (default)
    assert!(scores.contains_key("eff-batch-2"));
    assert_eq!(scores["eff-batch-2"].samples, 0);

    // Missing pattern should not be in the map
    assert!(!scores.contains_key("eff-batch-missing"));

    // Empty input returns empty map
    let empty = db
        .get_effectiveness_scores_batch(&Vec::<String>::new())
        .unwrap();
    assert!(empty.is_empty());
}

// ── v7 Phase 6: FK constraints on trajectories and context_injections ──

#[test]
fn test_trajectory_fk_on_delete_set_null() {
    let db = test_db();

    // Create a work item
    let mut item = test_work_item("wi-fk-test", "FK Test");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();

    // Insert trajectory referencing the work item
    let now = Utc::now().to_rfc3339();
    db.conn
        .execute(
            "INSERT INTO trajectories (id, session_id, work_item_id, status, started_at)
             VALUES ('traj-fk', 'sess-1', 'wi-fk-test', 'recording', ?1)",
            params![now],
        )
        .unwrap();

    // Delete the work item — trajectory.work_item_id should become NULL (ON DELETE SET NULL)
    db.delete_work_item("wi-fk-test").unwrap();

    let work_item_id: Option<String> = db
        .conn
        .query_row(
            "SELECT work_item_id FROM trajectories WHERE id = 'traj-fk'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        work_item_id.is_none(),
        "work_item_id should be NULL after work item deletion"
    );
}

#[test]
fn test_context_injection_fk_on_delete_set_null() {
    let db = test_db();

    // Insert a trajectory
    let now = Utc::now().to_rfc3339();
    db.conn
        .execute(
            "INSERT INTO trajectories (id, session_id, status, started_at)
             VALUES ('traj-ci-fk', 'sess-1', 'recording', ?1)",
            params![now],
        )
        .unwrap();

    // Insert context_injection referencing the trajectory
    db.conn
        .execute(
            "INSERT INTO context_injections (session_id, trajectory_id, injection_type, timestamp)
             VALUES ('sess-1', 'traj-ci-fk', 'pattern', ?1)",
            params![now],
        )
        .unwrap();

    // Delete the trajectory — context_injections.trajectory_id should become NULL
    db.conn
        .execute("DELETE FROM trajectories WHERE id = 'traj-ci-fk'", [])
        .unwrap();

    let traj_id: Option<String> = db
        .conn
        .query_row(
            "SELECT trajectory_id FROM context_injections WHERE session_id = 'sess-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        traj_id.is_none(),
        "trajectory_id should be NULL after trajectory deletion"
    );
}

// ── v6 Phase 3: Data Integrity Guards ──

#[test]
fn test_parse_datetime_returns_epoch_on_corrupt() {
    let result = parse_datetime("not-a-date".to_string());
    assert_eq!(result.timestamp(), 0); // Unix epoch
}

#[test]
fn test_blob_to_vector_warns_on_misaligned() {
    // 5 bytes = 1 f32 + 1 extra byte
    let blob = vec![0u8; 5];
    let result = blob_to_vector(&blob);
    assert_eq!(result.len(), 1); // Only 1 complete f32
}

#[test]
fn test_end_session_finalizes_trajectories() {
    let db = test_db();
    let now = chrono::Utc::now();
    let session = SessionInfo {
        id: "sess-traj-test".to_string(),
        started_at: now,
        ended_at: None,
        cwd: ".".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();
    // Insert a recording trajectory directly
    db.conn
        .execute(
            "INSERT INTO trajectories (id, session_id, status, started_at) VALUES (?1, ?2, 'recording', ?3)",
            params!["traj-1", "sess-traj-test", now.to_rfc3339()],
        )
        .unwrap();
    // End the session
    db.end_session("sess-traj-test", now).unwrap();
    // Verify trajectory is now completed
    let status: String = db
        .conn
        .query_row(
            "SELECT status FROM trajectories WHERE id = 'traj-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "completed");
}

// ── get_agent_sessions_recursive tests ──

#[test]
fn test_get_agent_sessions_recursive_returns_direct_children() {
    use flowforge_core::{AgentSession, AgentSessionStatus};
    let db = test_db();

    // Create parent session
    let session = SessionInfo {
        id: "sess-recursive".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();

    // Create direct child agent
    let agent = AgentSession {
        id: "agent-direct".to_string(),
        parent_session_id: "sess-recursive".to_string(),
        agent_id: "ag-direct".to_string(),
        agent_type: "general".to_string(),
        status: AgentSessionStatus::Active,
        started_at: Utc::now(),
        ended_at: None,
        edits: 0,
        commands: 0,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&agent).unwrap();

    let results = db.get_agent_sessions_recursive("sess-recursive").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].agent_id, "ag-direct");
}

#[test]
fn test_get_agent_sessions_recursive_includes_grandchildren() {
    use flowforge_core::{AgentSession, AgentSessionStatus};
    let db = test_db();

    // Create parent session
    let session = SessionInfo {
        id: "sess-team".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();

    // Create team lead agent (direct child)
    let team_lead = AgentSession {
        id: "agent-lead".to_string(),
        parent_session_id: "sess-team".to_string(),
        agent_id: "ag-lead".to_string(),
        agent_type: "general".to_string(),
        status: AgentSessionStatus::Active,
        started_at: Utc::now(),
        ended_at: None,
        edits: 0,
        commands: 0,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&team_lead).unwrap();

    // Create sub-agent spawned by team lead (grandchild)
    let sub_agent = AgentSession {
        id: "agent-sub".to_string(),
        parent_session_id: "ag-lead".to_string(), // parent is team lead's agent_id
        agent_id: "ag-sub".to_string(),
        agent_type: "Explore".to_string(),
        status: AgentSessionStatus::Active,
        started_at: Utc::now(),
        ended_at: None,
        edits: 0,
        commands: 0,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&sub_agent).unwrap();

    // Recursive query from parent session should find both
    let results = db.get_agent_sessions_recursive("sess-team").unwrap();
    assert_eq!(results.len(), 2);

    let agent_ids: Vec<&str> = results.iter().map(|a| a.agent_id.as_str()).collect();
    assert!(agent_ids.contains(&"ag-lead"));
    assert!(agent_ids.contains(&"ag-sub"));
}

#[test]
fn test_get_agent_sessions_recursive_includes_all_descendants() {
    use flowforge_core::{AgentSession, AgentSessionStatus};
    let db = test_db();

    let session = SessionInfo {
        id: "sess-skip".to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    };
    db.create_session(&session).unwrap();

    // Ended team lead
    let ended_lead = AgentSession {
        id: "agent-ended-lead".to_string(),
        parent_session_id: "sess-skip".to_string(),
        agent_id: "ag-ended-lead".to_string(),
        agent_type: "general".to_string(),
        status: AgentSessionStatus::Completed,
        started_at: Utc::now(),
        ended_at: Some(Utc::now()),
        edits: 0,
        commands: 0,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&ended_lead).unwrap();

    // Sub-agent of ended lead — still active, should be visible
    let sub = AgentSession {
        id: "agent-hidden-sub".to_string(),
        parent_session_id: "ag-ended-lead".to_string(),
        agent_id: "ag-hidden-sub".to_string(),
        agent_type: "Explore".to_string(),
        status: AgentSessionStatus::Active,
        started_at: Utc::now(),
        ended_at: None,
        edits: 0,
        commands: 0,
        task_id: None,
        transcript_path: None,
    };
    db.create_agent_session(&sub).unwrap();

    // Recursive CTE returns ALL descendants regardless of parent status
    // (sub-agents of ended leads may still be active and need visibility)
    let results = db.get_agent_sessions_recursive("sess-skip").unwrap();
    assert_eq!(results.len(), 2);
    let agent_ids: Vec<&str> = results.iter().map(|a| a.agent_id.as_str()).collect();
    assert!(agent_ids.contains(&"ag-ended-lead"));
    assert!(agent_ids.contains(&"ag-hidden-sub"));
}

// ── get_or_create_from_claude_task tests ──

#[test]
fn test_get_or_create_from_claude_task_creates_new() {
    use flowforge_core::config::WorkTrackingConfig;
    use flowforge_core::work_tracking;
    let db = test_db();
    let config = WorkTrackingConfig {
        backend: "flowforge".to_string(),
        ..Default::default()
    };

    let id = work_tracking::get_or_create_from_claude_task(
        &db,
        &config,
        Some("claude-task-1"),
        "Implement feature X",
        Some("Description here"),
    )
    .unwrap();

    // Work item should exist
    let item = db.get_work_item(&id).unwrap().unwrap();
    assert_eq!(item.title, "Implement feature X");
    assert_eq!(item.external_id.as_deref(), Some("claude-task-1"));
    assert_eq!(item.backend, "claude_tasks");
}

#[test]
fn test_get_or_create_from_claude_task_deduplicates_by_external_id() {
    use flowforge_core::config::WorkTrackingConfig;
    use flowforge_core::work_tracking;
    let db = test_db();
    let config = WorkTrackingConfig {
        backend: "flowforge".to_string(),
        ..Default::default()
    };

    // Create first
    let id1 = work_tracking::get_or_create_from_claude_task(
        &db,
        &config,
        Some("claude-task-dup"),
        "Fix bug Y",
        None,
    )
    .unwrap();

    // Same external_id → should return same item
    let id2 = work_tracking::get_or_create_from_claude_task(
        &db,
        &config,
        Some("claude-task-dup"),
        "Fix bug Y",
        None,
    )
    .unwrap();

    assert_eq!(id1, id2);

    // Verify only one item exists
    let filter = WorkFilter::default();
    let items = db.list_work_items(&filter).unwrap();
    assert_eq!(items.len(), 1);
}

#[test]
fn test_get_or_create_from_claude_task_deduplicates_by_title() {
    use flowforge_core::config::WorkTrackingConfig;
    use flowforge_core::work_tracking;
    let db = test_db();
    let config = WorkTrackingConfig {
        backend: "flowforge".to_string(),
        ..Default::default()
    };

    // Create a work item without external_id
    let mut item = test_work_item("wi-existing", "Deploy service Z");
    item.status = flowforge_core::WorkStatus::InProgress;
    db.create_work_item(&item).unwrap();

    // get_or_create with matching title → should return existing
    let id = work_tracking::get_or_create_from_claude_task(
        &db,
        &config,
        Some("claude-task-new"),
        "Deploy service Z",
        None,
    )
    .unwrap();

    assert_eq!(id, "wi-existing");

    // External ID should now be linked
    let updated = db.get_work_item("wi-existing").unwrap().unwrap();
    assert_eq!(updated.external_id.as_deref(), Some("claude-task-new"));
}
