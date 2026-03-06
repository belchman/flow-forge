use chrono::Utc;
use rusqlite::params;

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

/// A file co-edit dependency edge: two files frequently edited in the same session.
#[derive(Debug, Clone)]
pub struct FileDependency {
    pub file_a: String,
    pub file_b: String,
    pub co_edit_count: u64,
    pub last_seen: String,
}

impl MemoryDb {
    /// From the `edits` table, find all pairs of files edited in the same session
    /// and upsert co-edit counts. Returns the number of pairs recorded.
    pub fn record_file_co_edits(&self, session_id: &str) -> Result<u32> {
        let edits = self.get_edits_for_session(session_id)?;
        if edits.len() < 2 {
            return Ok(0);
        }

        // Collect unique file paths
        let files: Vec<String> = {
            let mut set = std::collections::HashSet::new();
            for e in &edits {
                set.insert(e.file_path.clone());
            }
            let mut v: Vec<String> = set.into_iter().collect();
            v.sort();
            v
        };

        if files.len() < 2 {
            return Ok(0);
        }

        let now = Utc::now().to_rfc3339();
        let mut count = 0u32;

        // Generate all unique pairs (file_a < file_b to avoid duplicates)
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                self.conn
                    .execute(
                        "INSERT INTO file_co_edits (file_a, file_b, co_edit_count, last_seen)
                         VALUES (?1, ?2, 1, ?3)
                         ON CONFLICT(file_a, file_b) DO UPDATE SET
                             co_edit_count = co_edit_count + 1,
                             last_seen = ?3",
                        params![files[i], files[j], now],
                    )
                    .sq()?;
                count += 1;
            }
        }

        Ok(count)
    }

    /// Returns files most often co-edited with the given file, sorted by co_edit_count DESC.
    pub fn get_related_files(&self, file_path: &str, limit: usize) -> Result<Vec<FileDependency>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT file_a, file_b, co_edit_count, last_seen
                 FROM file_co_edits
                 WHERE file_a = ?1 OR file_b = ?1
                 ORDER BY co_edit_count DESC
                 LIMIT ?2",
            )
            .sq()?;

        let rows = stmt
            .query_map(params![file_path, limit as i64], |row| {
                Ok(FileDependency {
                    file_a: row.get(0)?,
                    file_b: row.get(1)?,
                    co_edit_count: row.get(2)?,
                    last_seen: row.get(3)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Returns the full dependency graph (top edges by co-edit count).
    pub fn get_dependency_graph(
        &self,
        min_count: u32,
        limit: usize,
    ) -> Result<Vec<FileDependency>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT file_a, file_b, co_edit_count, last_seen
                 FROM file_co_edits
                 WHERE co_edit_count >= ?1
                 ORDER BY co_edit_count DESC
                 LIMIT ?2",
            )
            .sq()?;

        let rows = stmt
            .query_map(params![min_count, limit as i64], |row| {
                Ok(FileDependency {
                    file_a: row.get(0)?,
                    file_b: row.get(1)?,
                    co_edit_count: row.get(2)?,
                    last_seen: row.get(3)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowforge_core::{EditRecord, SessionInfo};
    use std::path::Path;

    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    fn create_session(db: &MemoryDb, id: &str) {
        let session = SessionInfo {
            id: id.to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp".to_string(),
            edits: 0,
            commands: 0,
            summary: None,
            transcript_path: None,
        };
        db.create_session(&session).unwrap();
    }

    fn record_edit(db: &MemoryDb, session_id: &str, file_path: &str) {
        let edit = EditRecord {
            session_id: session_id.to_string(),
            timestamp: Utc::now(),
            file_path: file_path.to_string(),
            operation: "write".to_string(),
            file_extension: Some("rs".to_string()),
        };
        db.record_edit(&edit).unwrap();
    }

    #[test]
    fn test_record_file_co_edits() {
        let db = test_db();
        create_session(&db, "sess-1");

        record_edit(&db, "sess-1", "src/main.rs");
        record_edit(&db, "sess-1", "src/lib.rs");
        record_edit(&db, "sess-1", "src/utils.rs");

        let count = db.record_file_co_edits("sess-1").unwrap();
        // 3 files -> 3 pairs: (lib.rs, main.rs), (lib.rs, utils.rs), (main.rs, utils.rs)
        assert_eq!(count, 3);

        // Verify pairs were stored
        let graph = db.get_dependency_graph(1, 10).unwrap();
        assert_eq!(graph.len(), 3);
        for dep in &graph {
            assert_eq!(dep.co_edit_count, 1);
        }
    }

    #[test]
    fn test_get_related_files() {
        let db = test_db();
        create_session(&db, "sess-2");

        record_edit(&db, "sess-2", "src/a.rs");
        record_edit(&db, "sess-2", "src/b.rs");
        record_edit(&db, "sess-2", "src/c.rs");

        db.record_file_co_edits("sess-2").unwrap();

        // Query related files for src/a.rs
        let related = db.get_related_files("src/a.rs", 10).unwrap();
        assert_eq!(related.len(), 2);

        // Both b.rs and c.rs should appear as related
        let related_files: Vec<String> = related
            .iter()
            .map(|d| {
                if d.file_a == "src/a.rs" {
                    d.file_b.clone()
                } else {
                    d.file_a.clone()
                }
            })
            .collect();
        assert!(related_files.contains(&"src/b.rs".to_string()));
        assert!(related_files.contains(&"src/c.rs".to_string()));
    }

    #[test]
    fn test_co_edits_incremented_across_sessions() {
        let db = test_db();

        // Session 1: edit a.rs and b.rs together
        create_session(&db, "sess-3a");
        record_edit(&db, "sess-3a", "src/a.rs");
        record_edit(&db, "sess-3a", "src/b.rs");
        db.record_file_co_edits("sess-3a").unwrap();

        // Session 2: edit a.rs and b.rs together again
        create_session(&db, "sess-3b");
        record_edit(&db, "sess-3b", "src/a.rs");
        record_edit(&db, "sess-3b", "src/b.rs");
        db.record_file_co_edits("sess-3b").unwrap();

        // Session 3: edit a.rs and b.rs together a third time
        create_session(&db, "sess-3c");
        record_edit(&db, "sess-3c", "src/a.rs");
        record_edit(&db, "sess-3c", "src/b.rs");
        db.record_file_co_edits("sess-3c").unwrap();

        // Count should be 3
        let related = db.get_related_files("src/a.rs", 10).unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].co_edit_count, 3);
    }

    #[test]
    fn test_get_dependency_graph() {
        let db = test_db();

        // Create multiple sessions with different file pairs
        create_session(&db, "sess-4a");
        record_edit(&db, "sess-4a", "src/x.rs");
        record_edit(&db, "sess-4a", "src/y.rs");
        db.record_file_co_edits("sess-4a").unwrap();

        create_session(&db, "sess-4b");
        record_edit(&db, "sess-4b", "src/x.rs");
        record_edit(&db, "sess-4b", "src/y.rs");
        db.record_file_co_edits("sess-4b").unwrap();

        create_session(&db, "sess-4c");
        record_edit(&db, "sess-4c", "src/p.rs");
        record_edit(&db, "sess-4c", "src/q.rs");
        db.record_file_co_edits("sess-4c").unwrap();

        // Get graph with min_count=2 (should only include x.rs <-> y.rs)
        let graph = db.get_dependency_graph(2, 10).unwrap();
        assert_eq!(graph.len(), 1);
        assert_eq!(graph[0].co_edit_count, 2);

        // Get full graph with min_count=1
        let graph = db.get_dependency_graph(1, 10).unwrap();
        assert_eq!(graph.len(), 2);
        // First entry should have highest count
        assert!(graph[0].co_edit_count >= graph[1].co_edit_count);
    }

    #[test]
    fn test_single_file_session_records_nothing() {
        let db = test_db();
        create_session(&db, "sess-5");
        record_edit(&db, "sess-5", "src/only.rs");

        let count = db.record_file_co_edits("sess-5").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_empty_session_records_nothing() {
        let db = test_db();
        create_session(&db, "sess-6");

        let count = db.record_file_co_edits("sess-6").unwrap();
        assert_eq!(count, 0);
    }
}
