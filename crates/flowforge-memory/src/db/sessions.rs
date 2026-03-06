use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};

use flowforge_core::{Result, SessionInfo};

use super::{parse_datetime, MemoryDb, SqliteExt};

impl MemoryDb {
    pub fn create_session(&self, session: &SessionInfo) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO sessions (id, started_at, ended_at, cwd, edits, commands, summary, transcript_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    session.id,
                    session.started_at.to_rfc3339(),
                    session.ended_at.map(|t| t.to_rfc3339()),
                    session.cwd,
                    session.edits,
                    session.commands,
                    session.summary,
                    session.transcript_path,
                ],
            )
            .sq()?;
        Ok(())
    }

    /// Reopen a previously-ended session (for resume scenarios).
    /// Sets ended_at back to NULL so it appears active again.
    pub fn reopen_session(&self, id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET ended_at = NULL WHERE id = ?1 AND ended_at IS NOT NULL",
                params![id],
            )
            .sq()?;
        Ok(())
    }

    pub fn end_session(&self, id: &str, ended_at: DateTime<Utc>) -> Result<()> {
        let ts = ended_at.to_rfc3339();
        self.with_transaction(|| {
            self.conn
                .execute(
                    "UPDATE sessions SET ended_at = ?1 WHERE id = ?2",
                    params![ts, id],
                )
                .sq()?;
            // Cascade: end all child agent sessions that are still open
            self.conn
                .execute(
                    "UPDATE agent_sessions SET ended_at = ?1, status = 'Completed'
                     WHERE parent_session_id = ?2 AND ended_at IS NULL",
                    params![ts, id],
                )
                .sq()?;
            // Finalize any trajectories still in recording status
            self.conn
                .execute(
                    "UPDATE trajectories SET status = 'completed', ended_at = ?1
                     WHERE session_id = ?2 AND status = 'recording'",
                    params![ts, id],
                )
                .sq()?;
            Ok(())
        })
    }

    /// Get a specific session by ID (regardless of ended_at status).
    pub fn get_session_by_id(&self, id: &str) -> Result<Option<SessionInfo>> {
        self.conn
            .query_row(
                "SELECT id, started_at, ended_at, cwd, edits, commands, summary, transcript_path
                 FROM sessions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(SessionInfo {
                        id: row.get(0)?,
                        started_at: parse_datetime(row.get::<_, String>(1)?),
                        ended_at: row.get::<_, Option<String>>(2)?.map(parse_datetime),
                        cwd: row.get(3)?,
                        edits: row.get(4)?,
                        commands: row.get(5)?,
                        summary: row.get(6)?,
                        transcript_path: row.get(7).ok().flatten(),
                    })
                },
            )
            .optional()
            .sq()
    }

    pub fn get_current_session(&self) -> Result<Option<SessionInfo>> {
        self.conn
            .query_row(
                "SELECT id, started_at, ended_at, cwd, edits, commands, summary, transcript_path
                 FROM sessions WHERE ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
                [],
                |row| {
                    Ok(SessionInfo {
                        id: row.get(0)?,
                        started_at: parse_datetime(row.get::<_, String>(1)?),
                        ended_at: row.get::<_, Option<String>>(2)?.map(parse_datetime),
                        cwd: row.get(3)?,
                        edits: row.get(4)?,
                        commands: row.get(5)?,
                        summary: row.get(6)?,
                        transcript_path: row.get(7).ok().flatten(),
                    })
                },
            )
            .optional()
            .sq()
    }

    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionInfo>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, started_at, ended_at, cwd, edits, commands, summary, transcript_path
                 FROM sessions ORDER BY started_at DESC LIMIT ?1",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(SessionInfo {
                    id: row.get(0)?,
                    started_at: parse_datetime(row.get::<_, String>(1)?),
                    ended_at: row.get::<_, Option<String>>(2)?.map(parse_datetime),
                    cwd: row.get(3)?,
                    edits: row.get(4)?,
                    commands: row.get(5)?,
                    summary: row.get(6)?,
                    transcript_path: row.get(7).ok().flatten(),
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn increment_session_edits(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET edits = edits + 1 WHERE id = ?1",
                params![session_id],
            )
            .sq()?;
        Ok(())
    }

    pub fn increment_session_commands(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET commands = commands + 1 WHERE id = ?1",
                params![session_id],
            )
            .sq()?;
        Ok(())
    }

    pub fn update_session_transcript_path(&self, session_id: &str, path: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET transcript_path = ?1 WHERE id = ?2",
                params![path, session_id],
            )
            .sq()?;
        Ok(())
    }
}
