use chrono::Utc;
use rusqlite::params;

use flowforge_core::{AgentSession, AgentSessionStatus, Result};

use super::row_parsers::parse_agent_session_row;
use super::{MemoryDb, SqliteExt};

impl MemoryDb {
    pub fn create_agent_session(&self, session: &AgentSession) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO agent_sessions
                 (id, parent_session_id, agent_id, agent_type, status, started_at, ended_at, edits, commands, task_id, transcript_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    session.id,
                    session.parent_session_id,
                    session.agent_id,
                    session.agent_type,
                    session.status.to_string(),
                    session.started_at.to_rfc3339(),
                    session.ended_at.map(|t| t.to_rfc3339()),
                    session.edits,
                    session.commands,
                    session.task_id,
                    session.transcript_path,
                ],
            )
            .sq()?;
        Ok(())
    }

    pub fn end_agent_session(&self, agent_id: &str, status: AgentSessionStatus) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE agent_sessions SET ended_at = ?1, status = ?2
                 WHERE agent_id = ?3 AND ended_at IS NULL",
                params![now, status.to_string(), agent_id],
            )
            .sq()?;
        Ok(())
    }

    pub fn update_agent_session_status(
        &self,
        agent_id: &str,
        status: AgentSessionStatus,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_sessions SET status = ?1
                 WHERE agent_id = ?2 AND ended_at IS NULL",
                params![status.to_string(), agent_id],
            )
            .sq()?;
        Ok(())
    }

    pub fn get_agent_sessions(&self, parent_session_id: &str) -> Result<Vec<AgentSession>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, parent_session_id, agent_id, agent_type, status,
                        started_at, ended_at, edits, commands, task_id, transcript_path
                 FROM agent_sessions WHERE parent_session_id = ?1
                 ORDER BY started_at DESC",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![parent_session_id], |row| {
                Ok(parse_agent_session_row(row))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Get agent sessions recursively — traverses the full agent tree using
    /// a recursive CTE, so it handles arbitrarily deep nesting (not just 2 levels).
    pub fn get_agent_sessions_recursive(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<AgentSession>> {
        let mut stmt = self
            .conn
            .prepare(
                "WITH RECURSIVE agent_tree AS (
                    SELECT id, parent_session_id, agent_id, agent_type, status,
                           started_at, ended_at, edits, commands, task_id, transcript_path
                    FROM agent_sessions WHERE parent_session_id = ?1
                    UNION ALL
                    SELECT a.id, a.parent_session_id, a.agent_id, a.agent_type, a.status,
                           a.started_at, a.ended_at, a.edits, a.commands, a.task_id, a.transcript_path
                    FROM agent_sessions a
                    JOIN agent_tree t ON a.parent_session_id = t.agent_id
                )
                SELECT * FROM agent_tree
                ORDER BY started_at DESC",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![parent_session_id], |row| {
                Ok(parse_agent_session_row(row))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn get_active_agent_sessions(&self) -> Result<Vec<AgentSession>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, parent_session_id, agent_id, agent_type, status,
                        started_at, ended_at, edits, commands, task_id, transcript_path
                 FROM agent_sessions WHERE ended_at IS NULL
                 ORDER BY started_at DESC",
            )
            .sq()?;
        let rows = stmt
            .query_map([], |row| Ok(parse_agent_session_row(row)))
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn increment_agent_edits(&self, agent_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_sessions SET edits = edits + 1
                 WHERE agent_id = ?1 AND ended_at IS NULL",
                params![agent_id],
            )
            .sq()?;
        Ok(())
    }

    pub fn increment_agent_commands(&self, agent_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_sessions SET commands = commands + 1
                 WHERE agent_id = ?1 AND ended_at IS NULL",
                params![agent_id],
            )
            .sq()?;
        Ok(())
    }

    pub fn update_agent_session_transcript_path(&self, agent_id: &str, path: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_sessions SET transcript_path = ?1
                 WHERE agent_id = ?2 AND ended_at IS NULL",
                params![path, agent_id],
            )
            .sq()?;
        Ok(())
    }

    pub fn get_agents_on_work_item(&self, work_item_id: &str) -> Result<Vec<AgentSession>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, parent_session_id, agent_id, agent_type, status,
                        started_at, ended_at, edits, commands, task_id, transcript_path
                 FROM agent_sessions WHERE task_id = ?1
                 ORDER BY started_at DESC",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![work_item_id], |row| {
                Ok(parse_agent_session_row(row))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn update_agent_session_work_item(&self, agent_id: &str, work_item_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_sessions SET task_id = ?1
                 WHERE agent_id = ?2 AND ended_at IS NULL",
                params![work_item_id, agent_id],
            )
            .sq()?;
        Ok(())
    }

    /// Roll up an agent's edits/commands to its parent session.
    /// Call this after end_agent_session so the statusline reflects agent work.
    pub fn rollup_agent_stats_to_parent(&self, agent_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET
                    edits = edits + COALESCE(
                        (SELECT edits FROM agent_sessions WHERE agent_id = ?1 ORDER BY started_at DESC LIMIT 1), 0),
                    commands = commands + COALESCE(
                        (SELECT commands FROM agent_sessions WHERE agent_id = ?1 ORDER BY started_at DESC LIMIT 1), 0)
                 WHERE id = (SELECT parent_session_id FROM agent_sessions WHERE agent_id = ?1 ORDER BY started_at DESC LIMIT 1)",
                params![agent_id],
            )
            .sq()?;
        Ok(())
    }

    /// Clean up orphaned agent sessions — agents whose parent session has ended
    /// or whose parent session ID is empty/invalid (never properly linked).
    pub fn cleanup_orphaned_agent_sessions(&self) -> Result<u64> {
        let now = chrono::Utc::now().to_rfc3339();
        let count = self
            .conn
            .execute(
                "UPDATE agent_sessions SET ended_at = ?1, status = 'Completed'
                 WHERE ended_at IS NULL
                   AND (
                     parent_session_id IN (SELECT id FROM sessions WHERE ended_at IS NOT NULL)
                     OR parent_session_id = ''
                     OR parent_session_id NOT IN (SELECT id FROM sessions)
                   )",
                params![now],
            )
            .sq()?;
        Ok(count as u64)
    }
}
