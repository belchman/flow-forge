use chrono::Utc;
use rusqlite::params;

use flowforge_core::{types::ContextInjection, Result};

pub struct PatternEffectiveness {
    pub pattern_id: String,
    pub score: f64,
    pub samples: u32,
}

use super::{parse_datetime, MemoryDb, SqliteExt};

use rusqlite::OptionalExtension;

impl MemoryDb {
    // ── Context Injections (Impact Tracking) ──

    pub fn record_context_injection(
        &self,
        session_id: &str,
        trajectory_id: Option<&str>,
        injection_type: &str,
        reference_id: Option<&str>,
        similarity: Option<f64>,
        metadata: Option<&str>,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO context_injections (session_id, trajectory_id, injection_type, reference_id, similarity, timestamp, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![session_id, trajectory_id, injection_type, reference_id, similarity, now, metadata],
            )
            .sq()?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_injections_for_session(&self, session_id: &str) -> Result<Vec<ContextInjection>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, trajectory_id, injection_type, reference_id, similarity, timestamp, metadata
                 FROM context_injections WHERE session_id = ?1 ORDER BY id ASC",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(ContextInjection {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    trajectory_id: row.get(2)?,
                    injection_type: row.get(3)?,
                    reference_id: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    similarity: row.get(5)?,
                    timestamp: row.get(6)?,
                    metadata: row.get(7)?,
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Set effectiveness rating on a context injection.
    pub fn rate_context_injection(&self, id: i64, rating: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE context_injections SET effectiveness = ?1 WHERE id = ?2",
                params![rating, id],
            )
            .sq()?;
        Ok(())
    }

    /// Rate all injections in a session based on trajectory verdict.
    pub fn rate_session_injections(&self, session_id: &str, rating: &str) -> Result<usize> {
        let updated = self
            .conn
            .execute(
                "UPDATE context_injections SET effectiveness = ?1 WHERE session_id = ?2 AND effectiveness IS NULL",
                params![rating, session_id],
            )
            .sq()?;
        Ok(updated)
    }

    /// Aggregate effectiveness stats per injection type.
    pub fn get_injection_effectiveness_stats(
        &self,
        injection_type: &str,
    ) -> Result<Vec<(String, u64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT COALESCE(effectiveness, 'unrated'), COUNT(*)
                 FROM context_injections
                 WHERE injection_type = ?1
                 GROUP BY effectiveness
                 ORDER BY COUNT(*) DESC",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![injection_type], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Count routing accuracy: how often the suggested agent matches the trajectory agent.
    /// Looks at routing_hit:* meta keys (set by session_end hook).
    pub fn routing_accuracy_stats(&self) -> Result<(u64, u64)> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM flowforge_meta WHERE key LIKE 'routing_hit:%'")
            .sq()?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0)).sq()?;
        let mut hits = 0u64;
        let mut total = 0u64;
        for val in rows.flatten() {
            total += 1;
            if val == "1" {
                hits += 1;
            }
        }
        Ok((hits, total))
    }

    /// Average trajectory confidence for sessions with vs without context injections.
    pub fn context_effectiveness_stats(&self) -> Result<(f64, f64, u64, u64)> {
        // Sessions WITH injections
        let (with_conf, with_count): (f64, u64) = self
            .conn
            .query_row(
                "SELECT COALESCE(AVG(t.confidence), 0.0), COUNT(*)
                 FROM trajectories t
                 WHERE t.status = 'judged' AND t.confidence IS NOT NULL
                   AND EXISTS (SELECT 1 FROM context_injections ci WHERE ci.session_id = t.session_id)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .sq()?;

        // Sessions WITHOUT injections
        let (without_conf, without_count): (f64, u64) = self
            .conn
            .query_row(
                "SELECT COALESCE(AVG(t.confidence), 0.0), COUNT(*)
                 FROM trajectories t
                 WHERE t.status = 'judged' AND t.confidence IS NOT NULL
                   AND NOT EXISTS (SELECT 1 FROM context_injections ci WHERE ci.session_id = t.session_id)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .sq()?;

        Ok((with_conf, without_conf, with_count, without_count))
    }

    /// Pattern hit rate: sessions with pattern injections where trajectory verdict = success.
    pub fn pattern_hit_rate(&self) -> Result<(u64, u64)> {
        let total: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(DISTINCT ci.session_id)
                 FROM context_injections ci
                 JOIN trajectories t ON t.session_id = ci.session_id
                 WHERE ci.injection_type = 'pattern' AND t.status = 'judged'",
                [],
                |row| row.get(0),
            )
            .sq()?;

        let successes: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(DISTINCT ci.session_id)
                 FROM context_injections ci
                 JOIN trajectories t ON t.session_id = ci.session_id
                 WHERE ci.injection_type = 'pattern' AND t.status = 'judged' AND t.verdict = 'success'",
                [],
                |row| row.get(0),
            )
            .sq()?;

        Ok((successes, total))
    }

    /// Success rate of the last N judged trajectories.
    pub fn recent_trajectory_success_rate(&self, limit: usize) -> Result<f64> {
        let (total, successes): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COUNT(*), SUM(CASE WHEN verdict = 'success' THEN 1 ELSE 0 END)
                 FROM (SELECT verdict FROM trajectories WHERE status = 'judged' ORDER BY ended_at DESC LIMIT ?1)",
                params![limit],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .sq()?;
        if total == 0 {
            return Ok(0.0);
        }
        Ok(successes as f64 / total as f64)
    }

    // ── Pattern Effectiveness ──

    /// Record an effectiveness observation for a pattern injected in a session.
    pub fn record_pattern_effectiveness(
        &self,
        pattern_id: &str,
        session_id: &str,
        outcome: &str,
        similarity: f64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO pattern_effectiveness (pattern_id, session_id, outcome, similarity, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![pattern_id, session_id, outcome, similarity, now],
            )
            .sq()?;
        Ok(())
    }

    /// Record effectiveness for ALL patterns injected in a session (both success and failure).
    pub fn record_session_effectiveness(&self, session_id: &str, outcome: &str) -> Result<u32> {
        // Get all pattern injections for this session
        let mut stmt = self
            .conn
            .prepare(
                "SELECT reference_id, COALESCE(similarity, 0.0)
                 FROM context_injections
                 WHERE session_id = ?1 AND injection_type = 'pattern'",
            )
            .sq()?;

        let injections: Vec<(String, f64)> = stmt
            .query_map(params![session_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .sq()?
            .filter_map(|r| r.ok())
            .collect();

        let mut count = 0u32;
        let now = Utc::now().to_rfc3339();
        for (pattern_id, similarity) in &injections {
            let _ = self.conn.execute(
                "INSERT INTO pattern_effectiveness (pattern_id, session_id, outcome, similarity, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![pattern_id, session_id, outcome, similarity, now],
            );
            count += 1;
        }

        // Update cached effectiveness_score/samples on pattern tables
        for (pattern_id, _) in &injections {
            let _ = self.recompute_pattern_effectiveness(pattern_id);
        }

        Ok(count)
    }

    /// Recompute the cached effectiveness_score and effectiveness_samples for a pattern.
    /// Uses time-decayed scoring with a configurable half-life.
    pub(crate) fn recompute_pattern_effectiveness(&self, pattern_id: &str) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT outcome, similarity, timestamp FROM pattern_effectiveness
                 WHERE pattern_id = ?1 ORDER BY timestamp DESC",
            )
            .sq()?;

        let now = Utc::now();
        let half_life_secs = 14.0 * 24.0 * 3600.0; // 14 days default
        let ln2 = std::f64::consts::LN_2;

        let mut weighted_sum = 0.0f64;
        let mut weight_total = 0.0f64;
        let mut sample_count = 0u32;

        let rows = stmt
            .query_map(params![pattern_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .sq()?;

        for row in rows.flatten() {
            let (outcome, similarity, timestamp_str) = row;
            let ts = parse_datetime(timestamp_str);
            let age_secs = (now - ts).num_seconds().max(0) as f64;
            let decay = (-ln2 * age_secs / half_life_secs).exp();
            let sim_weight = similarity.max(0.1); // minimum weight
            let weight = decay * sim_weight;

            let value = match outcome.as_str() {
                "success" => 1.0,
                "failure" => 0.0,
                "partial" => 0.5,
                _ => 0.5,
            };

            weighted_sum += value * weight;
            weight_total += weight;
            sample_count += 1;
        }

        let score = if weight_total > 0.0 {
            weighted_sum / weight_total
        } else {
            0.0
        };

        // Update both pattern tables (pattern may be in either)
        let _ = self.conn.execute(
            "UPDATE patterns_long SET effectiveness_score = ?1, effectiveness_samples = ?2 WHERE id = ?3",
            params![score, sample_count, pattern_id],
        );
        let _ = self.conn.execute(
            "UPDATE patterns_short SET effectiveness_score = ?1, effectiveness_samples = ?2 WHERE id = ?3",
            params![score, sample_count, pattern_id],
        );

        Ok(())
    }

    /// Get top/bottom patterns by effectiveness score.
    pub fn get_patterns_by_effectiveness(
        &self,
        limit: usize,
        ascending: bool,
    ) -> Result<Vec<(String, String, f64, u32)>> {
        let order = if ascending { "ASC" } else { "DESC" };
        let sql = format!(
            "SELECT id, content, effectiveness_score, effectiveness_samples
             FROM patterns_long
             WHERE effectiveness_samples >= 3
             ORDER BY effectiveness_score {order}
             LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql).sq()?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Get effectiveness scores for multiple patterns in a single query.
    /// Avoids N+1 queries during promotion.
    pub fn get_effectiveness_scores_batch(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, PatternEffectiveness>> {
        use std::collections::HashMap;
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        // Check both pattern tables; build a map from pattern_id -> (score, samples)
        let placeholders: Vec<String> = ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let in_clause = placeholders.join(",");

        // SQLite reuses the same positional params across UNION ALL
        let sql = format!(
            "SELECT id, effectiveness_score, effectiveness_samples FROM patterns_short WHERE id IN ({in_clause})
             UNION ALL
             SELECT id, effectiveness_score, effectiveness_samples FROM patterns_long WHERE id IN ({in_clause})"
        );
        let params: Vec<&dyn rusqlite::types::ToSql> = ids
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();

        let mut stmt = self.conn.prepare(&sql).sq()?;
        let rows = stmt
            .query_map(params.as_slice(), |row| {
                let pattern_id: String = row.get(0)?;
                let score: f64 = row.get(1)?;
                let samples: u32 = row.get(2)?;
                Ok((
                    pattern_id.clone(),
                    PatternEffectiveness {
                        pattern_id,
                        score,
                        samples,
                    },
                ))
            })
            .sq()?;
        let mut map = HashMap::new();
        for row in rows {
            let (id, eff) = row.sq()?;
            map.insert(id, eff);
        }
        Ok(map)
    }

    /// Get pattern effectiveness score for promotion gating.
    pub fn get_pattern_effectiveness_score(
        &self,
        pattern_id: &str,
    ) -> Result<PatternEffectiveness> {
        // Try long-term first, then short-term
        let result: Option<PatternEffectiveness> = self
            .conn
            .query_row(
                "SELECT effectiveness_score, effectiveness_samples FROM patterns_long WHERE id = ?1",
                params![pattern_id],
                |row| {
                    Ok(PatternEffectiveness {
                        pattern_id: pattern_id.to_string(),
                        score: row.get(0)?,
                        samples: row.get(1)?,
                    })
                },
            )
            .optional()
            .sq()?;

        if let Some(r) = result {
            return Ok(r);
        }

        self.conn
            .query_row(
                "SELECT effectiveness_score, effectiveness_samples FROM patterns_short WHERE id = ?1",
                params![pattern_id],
                |row| {
                    Ok(PatternEffectiveness {
                        pattern_id: pattern_id.to_string(),
                        score: row.get(0)?,
                        samples: row.get(1)?,
                    })
                },
            )
            .optional()
            .sq()
            .map(|r| {
                r.unwrap_or(PatternEffectiveness {
                    pattern_id: pattern_id.to_string(),
                    score: 0.0,
                    samples: 0,
                })
            })
    }
}
