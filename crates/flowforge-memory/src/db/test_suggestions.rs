use chrono::Utc;
use rusqlite::params;

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

/// A suggested test based on historical co-occurrence data.
#[derive(Debug, Clone)]
pub struct TestSuggestion {
    pub test_file: String,
    pub test_command: Option<String>,
    pub confidence: f64,
    pub co_occurrence_count: u64,
}

impl MemoryDb {
    /// Record that when `edited_file` was modified, `test_file` was subsequently run.
    /// Uses an upsert on the `test_co_occurrences` table.
    pub fn record_test_co_occurrence(
        &self,
        edited_file: &str,
        test_file: &str,
        test_command: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO test_co_occurrences (edited_file, test_file, test_command, occurrence_count, last_seen)
                 VALUES (?1, ?2, ?3, 1, ?4)
                 ON CONFLICT(edited_file, test_file) DO UPDATE SET
                     occurrence_count = occurrence_count + 1,
                     test_command = COALESCE(?3, test_command),
                     last_seen = ?4",
                params![edited_file, test_file, test_command, now],
            )
            .sq()?;
        Ok(())
    }

    /// Returns suggested tests for a file based on historical co-occurrence,
    /// ordered by count descending.
    pub fn get_test_suggestions(
        &self,
        edited_file: &str,
        limit: usize,
    ) -> Result<Vec<TestSuggestion>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT test_file, test_command, occurrence_count
                 FROM test_co_occurrences
                 WHERE edited_file = ?1
                 ORDER BY occurrence_count DESC
                 LIMIT ?2",
            )
            .sq()?;

        // Compute max count for confidence normalization
        let max_count: u64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(occurrence_count), 0) FROM test_co_occurrences WHERE edited_file = ?1",
                params![edited_file],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let rows = stmt
            .query_map(params![edited_file, limit as i64], |row| {
                let count: u64 = row.get(2)?;
                let confidence = if max_count > 0 {
                    count as f64 / max_count as f64
                } else {
                    0.0
                };
                Ok(TestSuggestion {
                    test_file: row.get(0)?,
                    test_command: row.get(1)?,
                    confidence,
                    co_occurrence_count: count,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Returns suggested tests for multiple files, deduplicating results.
    /// When the same test appears for multiple edited files, the highest
    /// co-occurrence count and confidence are used.
    pub fn get_test_suggestions_batch(
        &self,
        edited_files: &[&str],
        limit: usize,
    ) -> Result<Vec<TestSuggestion>> {
        if edited_files.is_empty() {
            return Ok(Vec::new());
        }

        // Collect all suggestions, then deduplicate
        let mut seen: std::collections::HashMap<String, TestSuggestion> =
            std::collections::HashMap::new();

        for file in edited_files {
            // Use a generous per-file limit to gather candidates, then trim at the end
            let suggestions = self.get_test_suggestions(file, limit)?;
            for s in suggestions {
                seen.entry(s.test_file.clone())
                    .and_modify(|existing| {
                        if s.co_occurrence_count > existing.co_occurrence_count {
                            existing.co_occurrence_count = s.co_occurrence_count;
                            existing.confidence = s.confidence;
                            if s.test_command.is_some() {
                                existing.test_command = s.test_command.clone();
                            }
                        }
                    })
                    .or_insert(s);
            }
        }

        let mut results: Vec<TestSuggestion> = seen.into_values().collect();
        results.sort_by(|a, b| b.co_occurrence_count.cmp(&a.co_occurrence_count));
        results.truncate(limit);
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_record_and_get_suggestions() {
        let db = test_db();
        db.record_test_co_occurrence("src/lib.rs", "tests/lib_test.rs", Some("cargo test"))
            .unwrap();

        let suggestions = db.get_test_suggestions("src/lib.rs", 5).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].test_file, "tests/lib_test.rs");
        assert_eq!(suggestions[0].test_command.as_deref(), Some("cargo test"));
        assert_eq!(suggestions[0].co_occurrence_count, 1);
        assert!((suggestions[0].confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_suggestions_ordered_by_count() {
        let db = test_db();

        // Record test_a once, test_b three times for the same edited file
        db.record_test_co_occurrence("src/main.rs", "tests/test_a.rs", None)
            .unwrap();
        for _ in 0..3 {
            db.record_test_co_occurrence("src/main.rs", "tests/test_b.rs", None)
                .unwrap();
        }

        let suggestions = db.get_test_suggestions("src/main.rs", 5).unwrap();
        assert_eq!(suggestions.len(), 2);
        // test_b should come first (count=3) over test_a (count=1)
        assert_eq!(suggestions[0].test_file, "tests/test_b.rs");
        assert_eq!(suggestions[0].co_occurrence_count, 3);
        assert_eq!(suggestions[1].test_file, "tests/test_a.rs");
        assert_eq!(suggestions[1].co_occurrence_count, 1);
    }

    #[test]
    fn test_batch_suggestions_deduplicated() {
        let db = test_db();

        // Both edited files map to the same test_file
        db.record_test_co_occurrence("src/a.rs", "tests/integration.rs", Some("cargo test"))
            .unwrap();
        db.record_test_co_occurrence("src/a.rs", "tests/integration.rs", Some("cargo test"))
            .unwrap();
        db.record_test_co_occurrence("src/b.rs", "tests/integration.rs", Some("cargo test"))
            .unwrap();
        // b.rs also maps to a unique test
        db.record_test_co_occurrence("src/b.rs", "tests/unit_b.rs", None)
            .unwrap();

        let suggestions = db
            .get_test_suggestions_batch(&["src/a.rs", "src/b.rs"], 10)
            .unwrap();

        // Should have 2 unique test files: integration.rs and unit_b.rs
        assert_eq!(suggestions.len(), 2);

        // integration.rs should appear once (deduplicated), with highest count (2 from a.rs)
        let integration = suggestions
            .iter()
            .find(|s| s.test_file == "tests/integration.rs")
            .unwrap();
        assert_eq!(integration.co_occurrence_count, 2);
    }

    #[test]
    fn test_no_suggestions_unknown_file() {
        let db = test_db();

        // Record some data for other files
        db.record_test_co_occurrence("src/known.rs", "tests/known_test.rs", None)
            .unwrap();

        let suggestions = db.get_test_suggestions("src/unknown.rs", 5).unwrap();
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_upsert_increments_count() {
        let db = test_db();

        // Record the same pair multiple times
        for _ in 0..5 {
            db.record_test_co_occurrence("src/lib.rs", "tests/lib_test.rs", None)
                .unwrap();
        }

        let suggestions = db.get_test_suggestions("src/lib.rs", 5).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].co_occurrence_count, 5);
    }
}
