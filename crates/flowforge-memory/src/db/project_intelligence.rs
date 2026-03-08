use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};

use flowforge_core::Result;

use super::{parse_datetime, MemoryDb, SqliteExt};

/// A section of auto-generated (and optionally Claude-refined) project intelligence.
#[derive(Debug, Clone)]
pub struct IntelligenceSection {
    pub section_key: String,
    pub section_title: String,
    pub content: String,
    pub auto_generated: bool,
    pub confidence: f64,
    pub embedding_id: Option<i64>,
    pub project_type: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// Fixed display order for intelligence sections.
const SECTION_ORDER: &[&str] = &[
    "overview",
    "folder_structure",
    "conventions",
    "do_not_change",
    "api_formats",
    "business_logic",
    "dependency_graph",
    "error_hotspots",
    "test_coverage",
    "entry_points",
    "build_deploy",
    "env_catalog",
];

impl MemoryDb {
    /// Insert or update an intelligence section.
    pub fn upsert_intelligence_section(&self, section: &IntelligenceSection) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO project_intelligence (section_key, section_title, content, auto_generated, confidence, embedding_id, project_type, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(section_key) DO UPDATE SET
                    section_title = excluded.section_title,
                    content = excluded.content,
                    auto_generated = excluded.auto_generated,
                    confidence = excluded.confidence,
                    embedding_id = COALESCE(excluded.embedding_id, project_intelligence.embedding_id),
                    project_type = excluded.project_type,
                    updated_at = excluded.updated_at",
                params![
                    section.section_key,
                    section.section_title,
                    section.content,
                    section.auto_generated as i32,
                    section.confidence,
                    section.embedding_id,
                    section.project_type,
                    now,
                ],
            )
            .sq()?;
        Ok(())
    }

    /// Get a single intelligence section by key.
    pub fn get_intelligence_section(&self, key: &str) -> Result<Option<IntelligenceSection>> {
        self.conn
            .query_row(
                "SELECT section_key, section_title, content, auto_generated, confidence, embedding_id, project_type, updated_at
                 FROM project_intelligence WHERE section_key = ?1",
                params![key],
                |row| {
                    let updated_at_str: String = row.get(7)?;
                    let auto_gen: i32 = row.get(3)?;
                    Ok(IntelligenceSection {
                        section_key: row.get(0)?,
                        section_title: row.get(1)?,
                        content: row.get(2)?,
                        auto_generated: auto_gen != 0,
                        confidence: row.get(4)?,
                        embedding_id: row.get(5)?,
                        project_type: row.get(6)?,
                        updated_at: parse_datetime(updated_at_str),
                    })
                },
            )
            .optional()
            .sq()
    }

    /// List all intelligence sections.
    pub fn list_intelligence_sections(&self) -> Result<Vec<IntelligenceSection>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT section_key, section_title, content, auto_generated, confidence, embedding_id, project_type, updated_at
                 FROM project_intelligence ORDER BY section_key",
            )
            .sq()?;
        let rows = stmt
            .query_map([], |row| {
                let updated_at_str: String = row.get(7)?;
                let auto_gen: i32 = row.get(3)?;
                Ok(IntelligenceSection {
                    section_key: row.get(0)?,
                    section_title: row.get(1)?,
                    content: row.get(2)?,
                    auto_generated: auto_gen != 0,
                    confidence: row.get(4)?,
                    embedding_id: row.get(5)?,
                    project_type: row.get(6)?,
                    updated_at: parse_datetime(updated_at_str),
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Delete an intelligence section by key.
    pub fn delete_intelligence_section(&self, key: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM project_intelligence WHERE section_key = ?1",
                params![key],
            )
            .sq()?;
        Ok(())
    }

    /// Check if any intelligence sections exist.
    pub fn has_intelligence(&self) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM project_intelligence",
                [],
                |row| row.get(0),
            )
            .sq()?;
        Ok(count > 0)
    }

    /// Combine all sections into a single markdown document in fixed order.
    pub fn get_intelligence_markdown(&self) -> Result<String> {
        let sections = self.list_intelligence_sections()?;
        let mut by_key: std::collections::HashMap<&str, &IntelligenceSection> =
            sections.iter().map(|s| (s.section_key.as_str(), s)).collect();

        let mut md = String::from("# Project Intelligence\n\n");

        // Emit sections in fixed order first
        for &key in SECTION_ORDER {
            if let Some(section) = by_key.remove(key) {
                md.push_str(&format!("## {}\n\n", section.section_title));
                md.push_str(&section.content);
                md.push_str("\n\n");
            }
        }

        // Then any extra sections not in the standard order
        for section in by_key.values() {
            md.push_str(&format!("## {}\n\n", section.section_title));
            md.push_str(&section.content);
            md.push_str("\n\n");
        }

        Ok(md.trim_end().to_string())
    }

    /// Update the embedding_id for an intelligence section.
    pub fn update_intelligence_embedding(&self, key: &str, embedding_id: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE project_intelligence SET embedding_id = ?1 WHERE section_key = ?2",
                params![embedding_id, key],
            )
            .sq()?;
        Ok(())
    }

    /// Count intelligence sections without embeddings.
    pub fn count_unvectorized_intelligence(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM project_intelligence WHERE embedding_id IS NULL",
                [],
                |row| row.get(0),
            )
            .sq()
    }

    /// Get file co-edit pairs for intelligence generation.
    pub fn list_co_edit_pairs(&self, limit: usize) -> Result<Vec<(String, String, i64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT file_a, file_b, co_edit_count FROM file_co_edits ORDER BY co_edit_count DESC LIMIT ?1",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Get error hotspots for intelligence generation.
    pub fn list_error_hotspots(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, String, i64, bool)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT COALESCE(ef.tool_name, 'unknown'), ef.category, ef.error_preview, ef.occurrence_count,
                        CASE WHEN EXISTS(SELECT 1 FROM error_resolutions er WHERE er.fingerprint_id = ef.id) THEN 1 ELSE 0 END
                 FROM error_fingerprints ef
                 ORDER BY ef.occurrence_count DESC LIMIT ?1",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let has_res: i32 = row.get(4)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    has_res != 0,
                ))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Get test co-occurrences for intelligence generation.
    pub fn list_test_co_occurrences(&self, limit: usize) -> Result<Vec<(String, String, i64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT edited_file, test_file, occurrence_count FROM test_co_occurrences ORDER BY occurrence_count DESC LIMIT ?1",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Get the most recent code_index indexed_at timestamp.
    pub fn get_latest_code_index_time(&self) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT MAX(indexed_at) FROM code_index",
                [],
                |row| row.get(0),
            )
            .sq()
    }
}
