use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use flowforge_core::{LongTermPattern, Result, ShortTermPattern};

use super::{parse_datetime, MemoryDb, SqliteExt};

impl MemoryDb {
    // ── Patterns (Short-term) ──

    pub fn store_pattern_short(&self, pattern: &ShortTermPattern) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO patterns_short
                 (id, content, category, confidence, usage_count, created_at, last_used, embedding_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    pattern.id,
                    pattern.content,
                    pattern.category,
                    pattern.confidence,
                    pattern.usage_count,
                    pattern.created_at.to_rfc3339(),
                    pattern.last_used.to_rfc3339(),
                    pattern.embedding_id,
                ],
            )
            .sq()?;
        Ok(())
    }

    pub fn search_patterns_short(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ShortTermPattern>> {
        let pattern = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
                 FROM patterns_short WHERE content LIKE ?1 OR category LIKE ?1
                 ORDER BY confidence DESC LIMIT ?2",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![pattern, limit], |row| {
                Ok(ShortTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                    last_used: parse_datetime(row.get::<_, String>(6)?),
                    embedding_id: row.get(7)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn get_all_patterns_short(&self) -> Result<Vec<ShortTermPattern>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
                 FROM patterns_short ORDER BY last_used DESC",
            )
            .sq()?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ShortTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                    last_used: parse_datetime(row.get::<_, String>(6)?),
                    embedding_id: row.get(7)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn update_pattern_short_usage(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE patterns_short SET usage_count = usage_count + 1, last_used = ?1,
                 confidence = MIN(1.0, confidence + 0.05) WHERE id = ?2",
                params![now, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn delete_pattern_short(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM patterns_short WHERE id = ?1", params![id])
            .sq()?;
        Ok(())
    }

    /// Batch-delete expired short-term patterns (created before threshold, low confidence).
    pub fn batch_delete_expired_short_patterns(
        &self,
        threshold: &str,
        min_confidence: f64,
    ) -> Result<u64> {
        let count = self
            .conn
            .execute(
                "DELETE FROM patterns_short WHERE created_at < ?1 AND confidence < ?2",
                params![threshold, min_confidence],
            )
            .sq()?;
        Ok(count as u64)
    }

    /// Batch-delete vectors for expired short-term patterns.
    pub fn batch_delete_expired_short_vectors(
        &self,
        threshold: &str,
        min_confidence: f64,
    ) -> Result<u64> {
        let count = self
            .conn
            .execute(
                "DELETE FROM hnsw_entries WHERE source_type = 'pattern_short'
                 AND source_id IN (
                     SELECT id FROM patterns_short WHERE created_at < ?1 AND confidence < ?2
                 )",
                params![threshold, min_confidence],
            )
            .sq()?;
        Ok(count as u64)
    }

    pub fn count_patterns_short(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM patterns_short", [], |row| row.get(0))
            .sq()
    }

    pub fn get_pattern_short(&self, id: &str) -> Result<Option<ShortTermPattern>> {
        self.conn
            .query_row(
                "SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
                 FROM patterns_short WHERE id = ?1",
                params![id],
                |row| {
                    Ok(ShortTermPattern {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        category: row.get(2)?,
                        confidence: row.get(3)?,
                        usage_count: row.get(4)?,
                        created_at: parse_datetime(row.get::<_, String>(5)?),
                        last_used: parse_datetime(row.get::<_, String>(6)?),
                        embedding_id: row.get(7)?,
                    })
                },
            )
            .optional()
            .sq()
    }

    pub fn update_pattern_short_confidence(&self, id: &str, confidence: f64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE patterns_short SET confidence = ?1 WHERE id = ?2",
                params![confidence, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn get_top_patterns(&self, limit: usize) -> Result<Vec<ShortTermPattern>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
                 FROM patterns_short ORDER BY confidence DESC, usage_count DESC LIMIT ?1",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(ShortTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                    last_used: parse_datetime(row.get::<_, String>(6)?),
                    embedding_id: row.get(7)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Search patterns by keyword overlap with the prompt.
    /// Returns patterns whose content shares words with the query, ranked by match quality.
    pub fn search_patterns_by_keywords(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ShortTermPattern>> {
        // Extract meaningful keywords (4+ chars, not stop words, lowercased)
        const STOP_WORDS: &[&str] = &[
            "the", "this", "that", "with", "from", "into", "about", "have", "been",
            "were", "will", "just", "should", "would", "could", "also", "need", "want",
            "make", "like", "some", "more", "very", "when", "then", "than", "only",
            "each", "does", "done", "here", "there", "what", "your", "they", "them",
        ];
        let keywords: Vec<String> = query
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .filter(|w| w.len() >= 4 && !STOP_WORDS.contains(&w.as_str()))
            .collect();
        if keywords.is_empty() {
            return self.get_top_patterns(limit);
        }

        // Build SQL LIKE conditions for keyword matching
        let conditions: Vec<String> = keywords
            .iter()
            .enumerate()
            .map(|(i, _)| format!("LOWER(content) LIKE ?{}", i + 1))
            .collect();
        let where_clause = conditions.join(" OR ");

        // Search both short-term and long-term patterns, prioritizing long-term (proven)
        let sql = format!(
            "SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
             FROM (
               SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
               FROM patterns_long WHERE ({0})
               UNION ALL
               SELECT id, content, category, confidence, usage_count, created_at, last_used, embedding_id
               FROM patterns_short WHERE ({0})
             )
             ORDER BY confidence DESC, usage_count DESC
             LIMIT ?{1}",
            where_clause,
            keywords.len() + 1
        );

        let mut stmt = self.conn.prepare(&sql).sq()?;
        let like_params: Vec<String> = keywords.iter().map(|k| format!("%{}%", k)).collect();
        let mut params_vec: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
        for p in &like_params {
            params_vec.push(p as &dyn rusqlite::types::ToSql);
        }
        let limit_val = limit as i64;
        params_vec.push(&limit_val);

        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_vec), |row| {
                Ok(ShortTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                    last_used: parse_datetime(row.get::<_, String>(6)?),
                    embedding_id: row.get(7)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    // ── Patterns (Long-term) ──

    pub fn store_pattern_long(&self, pattern: &LongTermPattern) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO patterns_long
                 (id, content, category, confidence, usage_count, success_count, failure_count,
                  created_at, promoted_at, last_used, embedding_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    pattern.id,
                    pattern.content,
                    pattern.category,
                    pattern.confidence,
                    pattern.usage_count,
                    pattern.success_count,
                    pattern.failure_count,
                    pattern.created_at.to_rfc3339(),
                    pattern.promoted_at.to_rfc3339(),
                    pattern.last_used.to_rfc3339(),
                    pattern.embedding_id,
                ],
            )
            .sq()?;
        Ok(())
    }

    pub fn search_patterns_long(&self, query: &str, limit: usize) -> Result<Vec<LongTermPattern>> {
        let pattern = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content, category, confidence, usage_count, success_count, failure_count,
                        created_at, promoted_at, last_used, embedding_id
                 FROM patterns_long WHERE content LIKE ?1 OR category LIKE ?1
                 ORDER BY confidence DESC LIMIT ?2",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![pattern, limit], |row| {
                Ok(LongTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    success_count: row.get(5)?,
                    failure_count: row.get(6)?,
                    created_at: parse_datetime(row.get::<_, String>(7)?),
                    promoted_at: parse_datetime(row.get::<_, String>(8)?),
                    last_used: parse_datetime(row.get::<_, String>(9)?),
                    embedding_id: row.get(10)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn get_pattern_long(&self, id: &str) -> Result<Option<LongTermPattern>> {
        self.conn
            .query_row(
                "SELECT id, content, category, confidence, usage_count, success_count, failure_count,
                        created_at, promoted_at, last_used, embedding_id
                 FROM patterns_long WHERE id = ?1",
                params![id],
                |row| {
                    Ok(LongTermPattern {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        category: row.get(2)?,
                        confidence: row.get(3)?,
                        usage_count: row.get(4)?,
                        success_count: row.get(5)?,
                        failure_count: row.get(6)?,
                        created_at: parse_datetime(row.get::<_, String>(7)?),
                        promoted_at: parse_datetime(row.get::<_, String>(8)?),
                        last_used: parse_datetime(row.get::<_, String>(9)?),
                        embedding_id: row.get(10)?,
                    })
                },
            )
            .optional()
            .sq()
    }

    pub fn update_pattern_long_feedback(&self, id: &str, success: bool) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        if success {
            self.conn
                .execute(
                    "UPDATE patterns_long SET success_count = success_count + 1,
                     confidence = MIN(1.0, confidence + 0.03), last_used = ?1 WHERE id = ?2",
                    params![now, id],
                )
                .sq()?;
        } else {
            self.conn
                .execute(
                    "UPDATE patterns_long SET failure_count = failure_count + 1,
                     confidence = MAX(0.0, confidence - 0.05), last_used = ?1 WHERE id = ?2",
                    params![now, id],
                )
                .sq()?;
        }
        Ok(())
    }

    pub fn update_pattern_long_confidence(&self, id: &str, confidence: f64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE patterns_long SET confidence = ?1 WHERE id = ?2",
                params![confidence, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn get_all_patterns_long(&self) -> Result<Vec<LongTermPattern>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, content, category, confidence, usage_count, success_count, failure_count,
                        created_at, promoted_at, last_used, embedding_id
                 FROM patterns_long ORDER BY last_used DESC",
            )
            .sq()?;
        let rows = stmt
            .query_map([], |row| {
                Ok(LongTermPattern {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    category: row.get(2)?,
                    confidence: row.get(3)?,
                    usage_count: row.get(4)?,
                    success_count: row.get(5)?,
                    failure_count: row.get(6)?,
                    created_at: parse_datetime(row.get::<_, String>(7)?),
                    promoted_at: parse_datetime(row.get::<_, String>(8)?),
                    last_used: parse_datetime(row.get::<_, String>(9)?),
                    embedding_id: row.get(10)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn delete_pattern_long(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM patterns_long WHERE id = ?1", params![id])
            .sq()?;
        Ok(())
    }

    pub fn count_patterns_long(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM patterns_long", [], |row| row.get(0))
            .sq()
    }

    pub fn count_patterns(&self) -> Result<u64> {
        let short = self.count_patterns_short()?;
        let long = self.count_patterns_long()?;
        Ok(short + long)
    }

    pub fn delete_dormant_long_patterns(&self, limit: usize) -> Result<u64> {
        let deleted = self
            .conn
            .execute(
                "DELETE FROM patterns_long WHERE id IN (
                    SELECT id FROM patterns_long
                    WHERE confidence <= 0.05
                    ORDER BY last_used ASC
                    LIMIT ?1
                )",
                params![limit],
            )
            .sq()?;
        Ok(deleted as u64)
    }

    pub fn delete_lowest_confidence_long(&self, limit: usize) -> Result<u64> {
        let deleted = self
            .conn
            .execute(
                "DELETE FROM patterns_long WHERE id IN (
                    SELECT id FROM patterns_long
                    ORDER BY confidence ASC, last_used ASC
                    LIMIT ?1
                )",
                params![limit],
            )
            .sq()?;
        Ok(deleted as u64)
    }
}
