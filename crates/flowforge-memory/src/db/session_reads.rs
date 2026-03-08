use rusqlite::{params, OptionalExtension};

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

impl MemoryDb {
    /// Record a file read in the current session. Uses UPSERT to update on re-read.
    pub fn record_file_read(
        &self,
        session_id: &str,
        file_path: &str,
        content_hash: &str,
        command_number: u32,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO session_reads (session_id, file_path, content_hash, command_number, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(session_id, file_path) DO UPDATE SET
                    content_hash = excluded.content_hash,
                    command_number = excluded.command_number,
                    timestamp = excluded.timestamp",
                params![session_id, file_path, content_hash, command_number, now],
            )
            .sq()?;
        Ok(())
    }

    /// Get the stored read info for a file in this session.
    /// Returns (content_hash, command_number) if found.
    pub fn get_file_read(
        &self,
        session_id: &str,
        file_path: &str,
    ) -> Result<Option<(String, u32)>> {
        self.conn
            .query_row(
                "SELECT content_hash, command_number FROM session_reads
                 WHERE session_id = ?1 AND file_path = ?2",
                params![session_id, file_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .sq()
    }

    /// Clear all session reads (called on compaction since Claude loses context).
    pub fn clear_session_reads(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM session_reads WHERE session_id = ?1",
                params![session_id],
            )
            .sq()?;
        Ok(())
    }
}
