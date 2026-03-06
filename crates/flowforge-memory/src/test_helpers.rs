//! Shared test helpers for the flowforge-memory crate.
//! These are behind `#[cfg(test)]` so they won't compile in production.

use std::path::Path;

use chrono::Utc;

use flowforge_core::{Checkpoint, SessionInfo, ShortTermPattern, WorkItem};

use crate::db::MemoryDb;

/// Create an in-memory database for tests.
pub fn memory_db() -> MemoryDb {
    MemoryDb::open(Path::new(":memory:")).unwrap()
}

/// Create a work item with sensible defaults.
pub fn work_item(id: &str, title: &str) -> WorkItem {
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

/// Create a session with sensible defaults.
pub fn session(id: &str) -> SessionInfo {
    SessionInfo {
        id: id.to_string(),
        started_at: Utc::now(),
        ended_at: None,
        cwd: "/tmp/test".to_string(),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: None,
    }
}

/// Create a checkpoint with sensible defaults.
pub fn checkpoint(id: &str, session_id: &str, name: &str, index: u32) -> Checkpoint {
    Checkpoint {
        id: id.to_string(),
        session_id: session_id.to_string(),
        name: name.to_string(),
        message_index: index,
        description: None,
        git_ref: None,
        created_at: Utc::now(),
        metadata: None,
    }
}

/// Create a short-term pattern with sensible defaults.
pub fn short_pattern(id: &str, content: &str, category: &str) -> ShortTermPattern {
    ShortTermPattern {
        id: id.to_string(),
        content: content.to_string(),
        category: category.to_string(),
        confidence: 0.5,
        usage_count: 0,
        created_at: Utc::now(),
        last_used: Utc::now(),
        embedding_id: None,
    }
}

/// Create a database seeded with a session, some work items, and patterns.
pub fn seeded_db() -> MemoryDb {
    let db = memory_db();
    db.create_session(&session("test-session")).unwrap();
    db.create_work_item(&work_item("wi-seed-1", "Seeded task 1"))
        .unwrap();
    db.create_work_item(&work_item("wi-seed-2", "Seeded task 2"))
        .unwrap();
    db.store_pattern_short(&short_pattern("pat-1", "Use cargo test", "testing"))
        .unwrap();
    db.store_pattern_short(&short_pattern("pat-2", "Run clippy", "quality"))
        .unwrap();
    db
}
