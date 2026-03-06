use flowforge_core::{
    trajectory::{Trajectory, TrajectoryStatus, TrajectoryVerdict},
    AgentSession, AgentSessionStatus, Checkpoint, ConversationMessage, MailboxMessage, SessionFork,
    WorkItem, WorkStatus,
};

use super::parse_datetime;

pub(crate) fn parse_conversation_message_row(row: &rusqlite::Row) -> ConversationMessage {
    ConversationMessage {
        id: row.get(0).unwrap_or(0),
        session_id: row.get(1).unwrap_or_default(),
        message_index: row.get(2).unwrap_or(0),
        message_type: row.get(3).unwrap_or_default(),
        role: row.get(4).unwrap_or_default(),
        content: row.get(5).unwrap_or_default(),
        model: row.get(6).ok().flatten(),
        message_id: row.get(7).ok().flatten(),
        parent_uuid: row.get(8).ok().flatten(),
        timestamp: parse_datetime(row.get::<_, String>(9).unwrap_or_default()),
        metadata: row.get(10).ok().flatten(),
        source: row.get(11).unwrap_or_else(|_| "transcript".to_string()),
    }
}

pub(crate) fn parse_checkpoint_row(row: &rusqlite::Row) -> Checkpoint {
    Checkpoint {
        id: row.get(0).unwrap_or_default(),
        session_id: row.get(1).unwrap_or_default(),
        name: row.get(2).unwrap_or_default(),
        message_index: row.get(3).unwrap_or(0),
        description: row.get(4).ok().flatten(),
        git_ref: row.get(5).ok().flatten(),
        created_at: parse_datetime(row.get::<_, String>(6).unwrap_or_default()),
        metadata: row.get(7).ok().flatten(),
    }
}

pub(crate) fn parse_session_fork_row(row: &rusqlite::Row) -> SessionFork {
    SessionFork {
        id: row.get(0).unwrap_or_default(),
        source_session_id: row.get(1).unwrap_or_default(),
        target_session_id: row.get(2).unwrap_or_default(),
        fork_message_index: row.get(3).unwrap_or(0),
        checkpoint_id: row.get(4).ok().flatten(),
        reason: row.get(5).ok().flatten(),
        created_at: parse_datetime(row.get::<_, String>(6).unwrap_or_default()),
    }
}

pub(crate) fn parse_mailbox_message_row(row: &rusqlite::Row) -> MailboxMessage {
    MailboxMessage {
        id: row.get(0).unwrap_or(0),
        work_item_id: row.get(1).unwrap_or_default(),
        from_session_id: row.get(2).unwrap_or_default(),
        from_agent_name: row.get(3).unwrap_or_default(),
        to_session_id: row.get(4).ok().flatten(),
        to_agent_name: row.get(5).ok().flatten(),
        message_type: row.get(6).unwrap_or_else(|_| "text".to_string()),
        content: row.get(7).unwrap_or_default(),
        priority: row.get(8).unwrap_or(2),
        read_at: row
            .get::<_, Option<String>>(9)
            .ok()
            .flatten()
            .map(parse_datetime),
        created_at: parse_datetime(row.get::<_, String>(10).unwrap_or_default()),
        metadata: row.get(11).ok().flatten(),
    }
}

pub(crate) fn parse_work_item_row(row: &rusqlite::Row) -> WorkItem {
    let labels_str: String = row
        .get::<_, String>(10)
        .unwrap_or_else(|_| "[]".to_string());
    let labels: Vec<String> = serde_json::from_str(&labels_str).unwrap_or_default();
    WorkItem {
        id: row.get(0).unwrap_or_default(),
        external_id: row.get(1).unwrap_or_default(),
        backend: row.get(2).unwrap_or_default(),
        item_type: row.get(3).unwrap_or_else(|_| "task".to_string()),
        title: row.get(4).unwrap_or_default(),
        description: row.get(5).unwrap_or_default(),
        status: row
            .get::<_, String>(6)
            .unwrap_or_else(|_| "pending".to_string())
            .parse()
            .unwrap_or(WorkStatus::Pending),
        assignee: row.get(7).unwrap_or_default(),
        parent_id: row.get(8).unwrap_or_default(),
        priority: row.get(9).unwrap_or(2),
        labels,
        created_at: parse_datetime(row.get::<_, String>(11).unwrap_or_default()),
        updated_at: parse_datetime(row.get::<_, String>(12).unwrap_or_default()),
        completed_at: row
            .get::<_, Option<String>>(13)
            .unwrap_or_default()
            .map(parse_datetime),
        session_id: row.get(14).unwrap_or_default(),
        metadata: row.get(15).unwrap_or_default(),
        claimed_by: row.get(16).ok().flatten(),
        claimed_at: row
            .get::<_, Option<String>>(17)
            .ok()
            .flatten()
            .map(parse_datetime),
        last_heartbeat: row
            .get::<_, Option<String>>(18)
            .ok()
            .flatten()
            .map(parse_datetime),
        progress: row.get(19).unwrap_or(0),
        stealable: row.get::<_, i32>(20).unwrap_or(0) != 0,
    }
}

pub(crate) fn parse_trajectory_row(row: &rusqlite::Row) -> Trajectory {
    Trajectory {
        id: row.get(0).unwrap_or_default(),
        session_id: row.get(1).unwrap_or_default(),
        work_item_id: row.get(2).ok().flatten(),
        agent_name: row.get(3).ok().flatten(),
        task_description: row.get(4).ok().flatten(),
        status: row
            .get::<_, String>(5)
            .unwrap_or_default()
            .parse()
            .unwrap_or(TrajectoryStatus::Recording),
        started_at: parse_datetime(row.get::<_, String>(6).unwrap_or_default()),
        ended_at: row
            .get::<_, Option<String>>(7)
            .ok()
            .flatten()
            .map(parse_datetime),
        verdict: row
            .get::<_, Option<String>>(8)
            .ok()
            .flatten()
            .and_then(|s| s.parse::<TrajectoryVerdict>().ok()),
        confidence: row.get(9).ok(),
        metadata: row.get(10).ok().flatten(),
        embedding_id: row.get(11).ok().flatten(),
    }
}

pub(crate) fn parse_agent_session_row(row: &rusqlite::Row) -> AgentSession {
    AgentSession {
        id: row.get(0).unwrap_or_default(),
        parent_session_id: row.get(1).unwrap_or_default(),
        agent_id: row.get(2).unwrap_or_default(),
        agent_type: row.get(3).unwrap_or_default(),
        status: row
            .get::<_, String>(4)
            .unwrap_or_default()
            .parse()
            .unwrap_or(AgentSessionStatus::Active),
        started_at: parse_datetime(row.get::<_, String>(5).unwrap_or_default()),
        ended_at: row
            .get::<_, Option<String>>(6)
            .ok()
            .flatten()
            .map(parse_datetime),
        edits: row.get(7).unwrap_or(0),
        commands: row.get(8).unwrap_or(0),
        task_id: row.get(9).ok().flatten(),
        transcript_path: row.get(10).ok().flatten(),
    }
}
