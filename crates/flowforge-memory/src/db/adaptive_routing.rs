use std::collections::HashMap;

use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

impl MemoryDb {
    // ── Adaptive Weight Storage (via flowforge_meta) ──

    /// Store an adaptive routing weight for a signal.
    pub fn set_adaptive_weight(&self, signal_name: &str, weight: f64) -> Result<()> {
        let key = format!("adaptive_weight:{signal_name}");
        self.conn
            .execute(
                "INSERT OR REPLACE INTO flowforge_meta (key, value) VALUES (?1, ?2)",
                params![key, weight.to_string()],
            )
            .sq()?;
        Ok(())
    }

    /// Get an adaptive routing weight for a signal. Returns None if not set.
    pub fn get_adaptive_weight(&self, signal_name: &str) -> Result<Option<f64>> {
        let key = format!("adaptive_weight:{signal_name}");
        let val = self
            .conn
            .query_row(
                "SELECT value FROM flowforge_meta WHERE key = ?1",
                [&key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .sq()?;
        Ok(val.and_then(|v| v.parse().ok()))
    }

    /// Get all adaptive routing weights as a signal_name -> weight map.
    pub fn get_all_adaptive_weights(&self) -> Result<HashMap<String, f64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM flowforge_meta WHERE key LIKE 'adaptive_weight:%'")
            .sq()?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .sq()?;
        let mut map = HashMap::new();
        for (key, val) in rows.flatten() {
            let signal = key
                .strip_prefix("adaptive_weight:")
                .unwrap_or(&key);
            if let Ok(w) = val.parse::<f64>() {
                map.insert(signal.to_string(), w);
            }
        }
        Ok(map)
    }

    // ── Routing Outcome Recording ──

    /// Record a routing outcome with per-signal score breakdown.
    #[allow(clippy::too_many_arguments)]
    pub fn record_routing_outcome(
        &self,
        session_id: &str,
        agent_name: &str,
        task_pattern: &str,
        pattern_score: f64,
        capability_score: f64,
        learned_score: f64,
        priority_score: f64,
        context_score: f64,
        semantic_score: f64,
        outcome: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO routing_outcomes (
                    session_id, agent_name, task_pattern,
                    pattern_score, capability_score, learned_score,
                    priority_score, context_score, semantic_score,
                    outcome, timestamp
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    session_id,
                    agent_name,
                    task_pattern,
                    pattern_score,
                    capability_score,
                    learned_score,
                    priority_score,
                    context_score,
                    semantic_score,
                    outcome,
                    now,
                ],
            )
            .sq()?;
        Ok(())
    }

    /// Count total routing outcomes recorded.
    pub fn count_routing_outcomes(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM routing_outcomes", [], |row| {
                row.get(0)
            })
            .sq()
    }

    /// Compute adaptive weights from recent routing outcomes.
    ///
    /// Algorithm:
    /// 1. Load outcomes from the last `window_days` days.
    /// 2. For each signal, compute correlation with success:
    ///    `signal_avg_success - signal_avg_failure`
    /// 3. Normalize to valid weights that sum to 1.0.
    /// 4. Apply exponential smoothing: `new = 0.7 * old + 0.3 * computed`
    pub fn compute_adaptive_weights(
        &self,
        window_days: u64,
    ) -> Result<HashMap<String, f64>> {
        let threshold = Utc::now() - chrono::Duration::days(window_days as i64);
        let threshold_str = threshold.to_rfc3339();

        // Compute average score per signal for successes and failures
        let mut stmt = self
            .conn
            .prepare(
                "SELECT outcome,
                        AVG(pattern_score), AVG(capability_score), AVG(learned_score),
                        AVG(priority_score), AVG(context_score), AVG(semantic_score),
                        COUNT(*)
                 FROM routing_outcomes
                 WHERE timestamp > ?1
                 GROUP BY outcome",
            )
            .sq()?;

        let mut success_avgs = [0.0f64; 6];
        let mut failure_avgs = [0.0f64; 6];
        let mut has_success = false;
        let mut has_failure = false;

        let rows = stmt
            .query_map(params![threshold_str], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    [
                        row.get::<_, f64>(1)?,
                        row.get::<_, f64>(2)?,
                        row.get::<_, f64>(3)?,
                        row.get::<_, f64>(4)?,
                        row.get::<_, f64>(5)?,
                        row.get::<_, f64>(6)?,
                    ],
                    row.get::<_, i64>(7)?,
                ))
            })
            .sq()?;

        for row in rows {
            let (outcome, avgs, _count) = row.sq()?;
            match outcome.as_str() {
                "success" => {
                    success_avgs = avgs;
                    has_success = true;
                }
                "failure" => {
                    failure_avgs = avgs;
                    has_failure = true;
                }
                // "partial" is ignored for weight computation
                _ => {}
            }
        }

        // If we don't have both success and failure data, return empty
        // (not enough signal to tune weights)
        if !has_success || !has_failure {
            return Ok(HashMap::new());
        }

        let signal_names = [
            "pattern",
            "capability",
            "learned",
            "priority",
            "context",
            "semantic",
        ];

        // Compute correlation: how much each signal contributes to success vs failure
        let mut correlations = [0.0f64; 6];
        for i in 0..6 {
            correlations[i] = success_avgs[i] - failure_avgs[i];
        }

        // Shift to positive range: add offset so minimum is at least 0.01
        let min_corr = correlations
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min);
        if min_corr < 0.0 {
            for c in &mut correlations {
                *c -= min_corr; // shift so min is 0
                *c += 0.01; // ensure non-zero
            }
        } else {
            // All non-negative, just ensure non-zero
            for c in &mut correlations {
                if *c < 0.01 {
                    *c = 0.01;
                }
            }
        }

        // Normalize to sum to 1.0
        let total: f64 = correlations.iter().sum();
        let mut computed = [0.0f64; 6];
        for i in 0..6 {
            computed[i] = correlations[i] / total;
        }

        // Apply exponential smoothing with existing weights
        let existing = self.get_all_adaptive_weights()?;
        let mut result = HashMap::new();

        for (i, name) in signal_names.iter().enumerate() {
            let new_weight = if let Some(&old) = existing.get(*name) {
                // Smooth: 70% old + 30% computed
                0.7 * old + 0.3 * computed[i]
            } else {
                computed[i]
            };

            // Guard against NaN/Inf
            let safe_weight = if new_weight.is_finite() {
                new_weight
            } else {
                computed[i]
            };

            result.insert(name.to_string(), safe_weight);
        }

        // Re-normalize the smoothed weights to sum to 1.0
        let total: f64 = result.values().sum();
        if total > 0.0 && total.is_finite() {
            for v in result.values_mut() {
                *v /= total;
            }
        }

        // Persist the computed weights
        for (name, weight) in &result {
            self.set_adaptive_weight(name, *weight)?;
        }

        Ok(result)
    }
}
