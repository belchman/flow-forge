use flowforge_core::Result;
use rusqlite::params;

use super::{MemoryDb, SqliteExt};

impl MemoryDb {
    /// Get the cached injection hash for this session.
    /// Returns `(content_hash, skip_count)` if cached, None otherwise.
    pub fn get_injection_cache(&self, session_id: &str) -> Result<Option<(String, u32)>> {
        let result = self
            .conn
            .query_row(
                "SELECT content_hash, skip_count FROM injection_cache WHERE session_id = ?1",
                params![session_id],
                |row| {
                    let hash: String = row.get(0)?;
                    let skip_count: u32 = row.get(1)?;
                    Ok((hash, skip_count))
                },
            )
            .optional()
            .sq()?;
        Ok(result)
    }

    /// Update the injection cache for this session.
    /// Sets skip_count to 0 (content was injected).
    pub fn set_injection_cache(&self, session_id: &str, content_hash: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO injection_cache (session_id, content_hash, skip_count)
                 VALUES (?1, ?2, 0)",
                params![session_id, content_hash],
            )
            .sq()?;
        Ok(())
    }

    /// Increment skip_count for this session (content was unchanged, injection skipped).
    pub fn increment_injection_skip(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE injection_cache SET skip_count = skip_count + 1 WHERE session_id = ?1",
                params![session_id],
            )
            .sq()?;
        Ok(())
    }

    /// Clear the injection cache for this session (forces re-injection on next prompt).
    pub fn clear_injection_cache(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM injection_cache WHERE session_id = ?1",
                params![session_id],
            )
            .sq()?;
        Ok(())
    }
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryDb;

    fn test_db() -> MemoryDb {
        MemoryDb::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_injection_cache_round_trip() {
        let db = test_db();
        let sid = "test-session";

        // Initially empty
        assert!(db.get_injection_cache(sid).unwrap().is_none());

        // Set cache
        db.set_injection_cache(sid, "abc123").unwrap();
        let (hash, skip) = db.get_injection_cache(sid).unwrap().unwrap();
        assert_eq!(hash, "abc123");
        assert_eq!(skip, 0);

        // Increment skip
        db.increment_injection_skip(sid).unwrap();
        let (_, skip) = db.get_injection_cache(sid).unwrap().unwrap();
        assert_eq!(skip, 1);

        // Update hash resets skip
        db.set_injection_cache(sid, "def456").unwrap();
        let (hash, skip) = db.get_injection_cache(sid).unwrap().unwrap();
        assert_eq!(hash, "def456");
        assert_eq!(skip, 0);

        // Clear
        db.clear_injection_cache(sid).unwrap();
        assert!(db.get_injection_cache(sid).unwrap().is_none());
    }

    #[test]
    fn test_injection_cache_multiple_sessions() {
        let db = test_db();

        db.set_injection_cache("s1", "hash1").unwrap();
        db.set_injection_cache("s2", "hash2").unwrap();

        let (h1, _) = db.get_injection_cache("s1").unwrap().unwrap();
        let (h2, _) = db.get_injection_cache("s2").unwrap().unwrap();
        assert_eq!(h1, "hash1");
        assert_eq!(h2, "hash2");

        // Clear one doesn't affect other
        db.clear_injection_cache("s1").unwrap();
        assert!(db.get_injection_cache("s1").unwrap().is_none());
        assert!(db.get_injection_cache("s2").unwrap().is_some());
    }

    #[test]
    fn test_injection_cache_skip_count_increments() {
        let db = test_db();
        let sid = "sess";

        db.set_injection_cache(sid, "h").unwrap();
        for _ in 0..5 {
            db.increment_injection_skip(sid).unwrap();
        }
        let (_, skip) = db.get_injection_cache(sid).unwrap().unwrap();
        assert_eq!(skip, 5);
    }
}
