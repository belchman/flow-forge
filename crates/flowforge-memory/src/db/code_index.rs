use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;

use flowforge_core::Result;

use super::{parse_datetime, MemoryDb, SqliteExt};

/// A code file entry in the index.
#[derive(Debug, Clone)]
pub struct CodeIndexEntry {
    pub file_path: String,
    pub language: String,
    pub size_bytes: i64,
    pub symbols: Vec<String>,
    pub description: String,
    pub summary: String,
    pub content_hash: String,
    pub indexed_at: chrono::DateTime<Utc>,
    pub embedding_id: Option<i64>,
}

impl MemoryDb {
    /// Insert or update a code index entry.
    pub fn upsert_code_entry(&self, entry: &CodeIndexEntry) -> Result<()> {
        let symbols_json =
            serde_json::to_string(&entry.symbols).unwrap_or_else(|_| "[]".to_string());
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO code_index (file_path, language, size_bytes, symbols, description, summary, content_hash, indexed_at, embedding_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(file_path) DO UPDATE SET
                    language = excluded.language,
                    size_bytes = excluded.size_bytes,
                    symbols = excluded.symbols,
                    description = excluded.description,
                    summary = excluded.summary,
                    content_hash = excluded.content_hash,
                    indexed_at = excluded.indexed_at,
                    embedding_id = COALESCE(excluded.embedding_id, code_index.embedding_id)",
                params![
                    entry.file_path,
                    entry.language,
                    entry.size_bytes,
                    symbols_json,
                    entry.description,
                    entry.summary,
                    entry.content_hash,
                    now,
                    entry.embedding_id,
                ],
            )
            .sq()?;
        Ok(())
    }

    /// Get a code index entry by file path.
    pub fn get_code_entry(&self, file_path: &str) -> Result<Option<CodeIndexEntry>> {
        self.conn
            .query_row(
                "SELECT file_path, language, size_bytes, symbols, description, summary, content_hash, indexed_at, embedding_id
                 FROM code_index WHERE file_path = ?1",
                params![file_path],
                |row| {
                    let symbols_json: String = row.get(3)?;
                    let symbols: Vec<String> =
                        serde_json::from_str(&symbols_json).unwrap_or_default();
                    let indexed_at_str: String = row.get(7)?;
                    Ok(CodeIndexEntry {
                        file_path: row.get(0)?,
                        language: row.get(1)?,
                        size_bytes: row.get(2)?,
                        symbols,
                        description: row.get(4)?,
                        summary: row.get(5)?,
                        content_hash: row.get(6)?,
                        indexed_at: parse_datetime(indexed_at_str),
                        embedding_id: row.get(8)?,
                    })
                },
            )
            .optional()
            .sq()
    }

    /// Get just the content hash for incremental check.
    pub fn get_code_entry_hash(&self, file_path: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT content_hash FROM code_index WHERE file_path = ?1",
                params![file_path],
                |row| row.get(0),
            )
            .optional()
            .sq()
    }

    /// List all code index entries.
    pub fn list_code_entries(&self, limit: usize) -> Result<Vec<CodeIndexEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT file_path, language, size_bytes, symbols, description, summary, content_hash, indexed_at, embedding_id
                 FROM code_index ORDER BY file_path LIMIT ?1",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let symbols_json: String = row.get(3)?;
                let symbols: Vec<String> =
                    serde_json::from_str(&symbols_json).unwrap_or_default();
                let indexed_at_str: String = row.get(7)?;
                Ok(CodeIndexEntry {
                    file_path: row.get(0)?,
                    language: row.get(1)?,
                    size_bytes: row.get(2)?,
                    symbols,
                    description: row.get(4)?,
                    summary: row.get(5)?,
                    content_hash: row.get(6)?,
                    indexed_at: parse_datetime(indexed_at_str),
                    embedding_id: row.get(8)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Count total entries in the code index.
    pub fn count_code_entries(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM code_index", [], |row| row.get(0))
            .sq()
    }

    /// Count entries without embeddings.
    pub fn count_unvectorized_code_entries(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM code_index WHERE embedding_id IS NULL",
                [],
                |row| row.get(0),
            )
            .sq()
    }

    /// Search code entries by symbol name, file path, or description using LIKE.
    pub fn search_code_symbols(&self, query: &str, limit: usize) -> Result<Vec<CodeIndexEntry>> {
        let pattern = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT file_path, language, size_bytes, symbols, description, summary, content_hash, indexed_at, embedding_id
                 FROM code_index WHERE symbols LIKE ?1 OR file_path LIKE ?1 OR description LIKE ?1
                 ORDER BY file_path LIMIT ?2",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![pattern, limit as i64], |row| {
                let symbols_json: String = row.get(3)?;
                let symbols: Vec<String> =
                    serde_json::from_str(&symbols_json).unwrap_or_default();
                let indexed_at_str: String = row.get(7)?;
                Ok(CodeIndexEntry {
                    file_path: row.get(0)?,
                    language: row.get(1)?,
                    size_bytes: row.get(2)?,
                    symbols,
                    description: row.get(4)?,
                    summary: row.get(5)?,
                    content_hash: row.get(6)?,
                    indexed_at: parse_datetime(indexed_at_str),
                    embedding_id: row.get(8)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Delete entries for files that no longer exist on disk.
    pub fn delete_stale_code_entries(&self, valid_paths: &[String]) -> Result<u64> {
        if valid_paths.is_empty() {
            let count = self.conn.execute("DELETE FROM code_index", []).sq()?;
            return Ok(count as u64);
        }

        let mut stmt = self
            .conn
            .prepare("SELECT file_path FROM code_index")
            .sq()?;
        let all_paths: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .sq()?
            .filter_map(|r| r.ok())
            .collect();

        let valid_set: HashSet<&str> = valid_paths.iter().map(|s| s.as_str()).collect();
        let mut deleted = 0u64;
        for path in &all_paths {
            if !valid_set.contains(path.as_str()) {
                self.conn
                    .execute(
                        "DELETE FROM code_index WHERE file_path = ?1",
                        params![path],
                    )
                    .sq()?;
                let _ = self.conn.execute(
                    "DELETE FROM hnsw_entries WHERE source_type = 'code_file' AND source_id = ?1",
                    params![path],
                );
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// Update the embedding_id for a code entry after vectorization.
    pub fn update_code_entry_embedding(&self, file_path: &str, embedding_id: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE code_index SET embedding_id = ?1 WHERE file_path = ?2",
                params![embedding_id, file_path],
            )
            .sq()?;
        Ok(())
    }
}
