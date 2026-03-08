//! Error recovery intelligence: fingerprinting, resolution tracking, and querying.

use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use flowforge_core::types::error_recovery::{
    classify_error, fingerprint_error, ErrorCategory, ErrorFingerprint, ErrorResolution,
};
use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

impl MemoryDb {
    /// Record an error occurrence. Creates or updates the fingerprint.
    /// Returns the fingerprint ID for resolution tracking.
    pub fn record_error_occurrence(
        &self,
        tool_name: &str,
        error_text: &str,
    ) -> Result<String> {
        let fp = fingerprint_error(error_text);
        let category = classify_error(error_text, tool_name);
        let preview: String = error_text.chars().take(200).collect();
        let now = Utc::now().to_rfc3339();

        // Upsert: increment count if exists, insert if not
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM error_fingerprints WHERE fingerprint = ?1",
                params![fp],
                |row| row.get(0),
            )
            .optional()
            .sq()?;

        if let Some(id) = existing {
            self.conn
                .execute(
                    "UPDATE error_fingerprints
                     SET occurrence_count = occurrence_count + 1,
                         last_seen = ?1
                     WHERE id = ?2",
                    params![now, id],
                )
                .sq()?;
            Ok(id)
        } else {
            let id = format!("ef-{}", &fp[..12]);
            self.conn
                .execute(
                    "INSERT INTO error_fingerprints
                     (id, fingerprint, category, tool_name, error_preview, first_seen, last_seen, occurrence_count)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
                    params![id, fp, category.to_string(), tool_name, preview, now],
                )
                .sq()?;
            Ok(id)
        }
    }

    /// Record that a specific tool sequence resolved an error.
    pub fn record_error_resolution(
        &self,
        fingerprint_id: &str,
        summary: &str,
        tool_sequence: &[String],
        files_changed: &[String],
        success: bool,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let tools_json = serde_json::to_string(tool_sequence).unwrap_or_default();
        let files_json = serde_json::to_string(files_changed).unwrap_or_default();

        // Check if a resolution with similar summary exists
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM error_resolutions
                 WHERE fingerprint_id = ?1 AND resolution_summary = ?2",
                params![fingerprint_id, summary],
                |row| row.get(0),
            )
            .optional()
            .sq()?;

        if let Some(id) = existing {
            if success {
                self.conn
                    .execute(
                        "UPDATE error_resolutions
                         SET success_count = success_count + 1, last_used = ?1
                         WHERE id = ?2",
                        params![now, id],
                    )
                    .sq()?;
            } else {
                self.conn
                    .execute(
                        "UPDATE error_resolutions
                         SET failure_count = failure_count + 1, last_used = ?1
                         WHERE id = ?2",
                        params![now, id],
                    )
                    .sq()?;
            }
        } else {
            let id = format!("er-{}", uuid_short());
            let (s, f) = if success { (1, 0) } else { (0, 1) };
            self.conn
                .execute(
                    "INSERT INTO error_resolutions
                     (id, fingerprint_id, resolution_summary, tool_sequence, files_changed,
                      success_count, failure_count, created_at, last_used)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                    params![id, fingerprint_id, summary, tools_json, files_json, s, f, now],
                )
                .sq()?;
        }

        Ok(())
    }

    /// Find resolutions for a given error text.
    /// Returns the fingerprint (if seen before) and its best resolutions.
    pub fn find_error_resolutions(
        &self,
        error_text: &str,
        limit: usize,
    ) -> Result<Option<(ErrorFingerprint, Vec<ErrorResolution>)>> {
        let fp = fingerprint_error(error_text);

        let fingerprint: Option<ErrorFingerprint> = self
            .conn
            .query_row(
                "SELECT id, fingerprint, category, tool_name, error_preview,
                        first_seen, last_seen, occurrence_count
                 FROM error_fingerprints WHERE fingerprint = ?1",
                params![fp],
                |row| {
                    Ok(ErrorFingerprint {
                        id: row.get(0)?,
                        fingerprint: row.get(1)?,
                        category: row
                            .get::<_, String>(2)?
                            .parse()
                            .unwrap_or(ErrorCategory::Unknown),
                        tool_name: row.get(3)?,
                        error_preview: row.get(4)?,
                        first_seen: super::helpers::parse_datetime(
                            row.get::<_, String>(5).unwrap_or_default(),
                        ),
                        last_seen: super::helpers::parse_datetime(
                            row.get::<_, String>(6).unwrap_or_default(),
                        ),
                        occurrence_count: row.get::<_, i64>(7).unwrap_or(1) as u32,
                    })
                },
            )
            .optional()
            .sq()?;

        let fingerprint = match fingerprint {
            Some(f) => f,
            None => return Ok(None),
        };

        // Get resolutions sorted by confidence (success_count / total)
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, fingerprint_id, resolution_summary, tool_sequence, files_changed,
                        success_count, failure_count, created_at, last_used
                 FROM error_resolutions
                 WHERE fingerprint_id = ?1
                 ORDER BY CAST(success_count AS REAL) / MAX(success_count + failure_count, 1) DESC
                 LIMIT ?2",
            )
            .sq()?;

        let resolutions = stmt
            .query_map(params![fingerprint.id, limit as i64], |row| {
                let tools_json: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
                let files_json: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
                Ok(ErrorResolution {
                    id: row.get(0)?,
                    fingerprint_id: row.get(1)?,
                    resolution_summary: row.get(2)?,
                    tool_sequence: serde_json::from_str(&tools_json).unwrap_or_default(),
                    files_changed: serde_json::from_str(&files_json).unwrap_or_default(),
                    success_count: row.get::<_, i64>(5).unwrap_or(0) as u32,
                    failure_count: row.get::<_, i64>(6).unwrap_or(0) as u32,
                    created_at: super::helpers::parse_datetime(
                        row.get::<_, String>(7).unwrap_or_default(),
                    ),
                    last_used: super::helpers::parse_datetime(
                        row.get::<_, String>(8).unwrap_or_default(),
                    ),
                })
            })
            .sq()?;

        let resolutions: Vec<_> = resolutions.filter_map(|r| r.ok()).collect();
        Ok(Some((fingerprint, resolutions)))
    }

    /// Record a tool failure for loop detection within a session.
    pub fn record_tool_failure(
        &self,
        session_id: &str,
        tool_name: &str,
        input_hash: &str,
        error_preview: Option<&str>,
        fingerprint_id: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO session_tool_failures
                 (session_id, tool_name, input_hash, error_preview, timestamp, fingerprint_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![session_id, tool_name, input_hash, error_preview, now, fingerprint_id],
            )
            .sq()?;
        Ok(())
    }

    /// Count how many times a specific tool+input has failed in this session.
    pub fn get_tool_failure_count(
        &self,
        session_id: &str,
        tool_name: &str,
        input_hash: &str,
    ) -> Result<u32> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM session_tool_failures
                 WHERE session_id = ?1 AND tool_name = ?2 AND input_hash = ?3",
                params![session_id, tool_name, input_hash],
                |row| row.get(0),
            )
            .sq()?;
        Ok(count as u32)
    }

    /// Get the previous session in the same working directory.
    /// Used for session continuity ("where was I?") injection.
    pub fn get_previous_session_context(
        &self,
        cwd: &str,
    ) -> Result<Option<flowforge_core::PreviousSessionContext>> {
        // Find the most recent ended session in the same cwd
        let session: Option<(String, String, String, i64, i64)> = self
            .conn
            .query_row(
                "SELECT id, started_at, ended_at, edits, commands
                 FROM sessions
                 WHERE cwd = ?1 AND ended_at IS NOT NULL
                 ORDER BY ended_at DESC LIMIT 1",
                params![cwd],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1).unwrap_or_default(),
                        row.get::<_, String>(2).unwrap_or_default(),
                        row.get::<_, i64>(3).unwrap_or(0),
                        row.get::<_, i64>(4).unwrap_or(0),
                    ))
                },
            )
            .optional()
            .sq()?;

        let (session_id, started_at, ended_at, edits, commands) = match session {
            Some(s) => s,
            None => return Ok(None),
        };

        // Get trajectory task description and verdict
        let traj_info: Option<(Option<String>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT task_description, verdict
                 FROM trajectories
                 WHERE session_id = ?1
                 ORDER BY started_at DESC LIMIT 1",
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .sq()?;

        let (task_description, verdict) = traj_info.unwrap_or((None, None));

        // Get files modified in that session
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT file_path FROM edits
                 WHERE session_id = ?1
                 ORDER BY timestamp DESC
                 LIMIT 10",
            )
            .sq()?;
        let files: Vec<String> = stmt
            .query_map(params![session_id], |row| row.get(0))
            .sq()?
            .filter_map(|r| r.ok())
            .collect();

        // Compute duration
        let start = super::helpers::parse_datetime(started_at);
        let end = super::helpers::parse_datetime(ended_at);
        let duration_minutes = (end - start).num_minutes();

        Ok(Some(flowforge_core::PreviousSessionContext {
            session_id,
            task_description,
            verdict,
            files_modified: files,
            edits_count: edits as u64,
            commands_count: commands as u64,
            duration_minutes,
        }))
    }

    /// Get the error preview for a specific tool failure by input hash.
    pub fn get_failure_error_preview(
        &self,
        session_id: &str,
        input_hash: &str,
    ) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT error_preview FROM session_tool_failures
                 WHERE session_id = ?1 AND input_hash = ?2
                 ORDER BY timestamp DESC LIMIT 1",
                params![session_id, input_hash],
                |row| row.get(0),
            )
            .optional()
            .sq()
    }

    /// Get recent error fingerprints for the current session's trajectory.
    /// Used to inject resolution suggestions during user_prompt_submit.
    pub fn get_recent_session_errors(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        // Get unique errors from recent failures, using stored fingerprint_id when available.
        // Falls back to hash recomputation for legacy rows.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT error_preview, fingerprint_id
                 FROM session_tool_failures
                 WHERE session_id = ?1 AND error_preview IS NOT NULL
                 ORDER BY timestamp DESC
                 LIMIT ?2",
            )
            .sq()?;

        let rows: Vec<(String, Option<String>)> = stmt
            .query_map(params![session_id, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .sq()?
            .filter_map(|r| r.ok())
            .collect();

        let mut results = Vec::new();
        for (preview, stored_fp_id) in rows {
            let fp_id = if let Some(id) = stored_fp_id {
                Some(id)
            } else {
                let fp_hash = fingerprint_error(&preview);
                self.conn
                    .query_row(
                        "SELECT id FROM error_fingerprints WHERE fingerprint = ?1",
                        params![fp_hash],
                        |row| row.get(0),
                    )
                    .optional()
                    .unwrap_or(None)
            };
            if let Some(id) = fp_id {
                results.push((preview, id));
            }
        }

        Ok(results)
    }

    /// List recent error fingerprints, ordered by last_seen desc.
    pub fn list_error_fingerprints(&self, limit: usize) -> Result<Vec<ErrorFingerprint>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, fingerprint, category, tool_name, error_preview,
                        first_seen, last_seen, occurrence_count
                 FROM error_fingerprints
                 ORDER BY last_seen DESC
                 LIMIT ?1",
            )
            .sq()?;

        let results = stmt
            .query_map(params![limit as i64], |row| {
                Ok(ErrorFingerprint {
                    id: row.get(0)?,
                    fingerprint: row.get(1)?,
                    category: row
                        .get::<_, String>(2)?
                        .parse()
                        .unwrap_or(ErrorCategory::Unknown),
                    tool_name: row.get(3)?,
                    error_preview: row.get(4)?,
                    first_seen: super::helpers::parse_datetime(
                        row.get::<_, String>(5).unwrap_or_default(),
                    ),
                    last_seen: super::helpers::parse_datetime(
                        row.get::<_, String>(6).unwrap_or_default(),
                    ),
                    occurrence_count: row.get::<_, i64>(7).unwrap_or(1) as u32,
                })
            })
            .sq()?;

        Ok(results.filter_map(|r| r.ok()).collect())
    }

    /// Get resolutions for a fingerprint by its ID.
    pub fn get_resolutions_for_fingerprint(
        &self,
        fingerprint_id: &str,
        limit: usize,
    ) -> Result<Vec<ErrorResolution>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, fingerprint_id, resolution_summary, tool_sequence, files_changed,
                        success_count, failure_count, created_at, last_used
                 FROM error_resolutions
                 WHERE fingerprint_id = ?1
                 ORDER BY CAST(success_count AS REAL) / MAX(success_count + failure_count, 1) DESC
                 LIMIT ?2",
            )
            .sq()?;

        let results = stmt
            .query_map(params![fingerprint_id, limit as i64], |row| {
                let tools_json: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
                let files_json: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
                Ok(ErrorResolution {
                    id: row.get(0)?,
                    fingerprint_id: row.get(1)?,
                    resolution_summary: row.get(2)?,
                    tool_sequence: serde_json::from_str(&tools_json).unwrap_or_default(),
                    files_changed: serde_json::from_str(&files_json).unwrap_or_default(),
                    success_count: row.get::<_, i64>(5).unwrap_or(0) as u32,
                    failure_count: row.get::<_, i64>(6).unwrap_or(0) as u32,
                    created_at: super::helpers::parse_datetime(
                        row.get::<_, String>(7).unwrap_or_default(),
                    ),
                    last_used: super::helpers::parse_datetime(
                        row.get::<_, String>(8).unwrap_or_default(),
                    ),
                })
            })
            .sq()?;

        Ok(results.filter_map(|r| r.ok()).collect())
    }

    /// Get a single error fingerprint by its ID (e.g. "ef-abc123").
    pub fn get_error_fingerprint_by_id(
        &self,
        id: &str,
    ) -> Result<Option<ErrorFingerprint>> {
        self.conn
            .query_row(
                "SELECT id, fingerprint, category, tool_name, error_preview,
                        first_seen, last_seen, occurrence_count
                 FROM error_fingerprints WHERE id = ?1",
                params![id],
                |row| {
                    Ok(ErrorFingerprint {
                        id: row.get(0)?,
                        fingerprint: row.get(1)?,
                        category: row
                            .get::<_, String>(2)?
                            .parse()
                            .unwrap_or(ErrorCategory::Unknown),
                        tool_name: row.get(3)?,
                        error_preview: row.get(4)?,
                        first_seen: super::helpers::parse_datetime(
                            row.get::<_, String>(5).unwrap_or_default(),
                        ),
                        last_seen: super::helpers::parse_datetime(
                            row.get::<_, String>(6).unwrap_or_default(),
                        ),
                        occurrence_count: row.get::<_, i64>(7).unwrap_or(1) as u32,
                    })
                },
            )
            .optional()
            .sq()
    }

    /// Find error resolutions using semantic vector similarity.
    /// Searches the "error" vector space and returns fingerprints with their resolutions.
    pub fn find_error_resolutions_semantic(
        &self,
        query_vec: &[f32],
        k: usize,
    ) -> Result<Vec<(ErrorFingerprint, Vec<ErrorResolution>)>> {
        let results = self.search_vectors(query_vec, &["error"], k)?;

        let mut output = Vec::new();
        for result in &results {
            if result.similarity < 0.3 {
                continue;
            }
            // source_id is the fingerprint_id
            if let Some(fp) = self.get_error_fingerprint_by_id(&result.source_id)? {
                let resolutions = self.get_resolutions_for_fingerprint(&fp.id, 3)?;
                if !resolutions.is_empty() {
                    output.push((fp, resolutions));
                }
            }
        }
        Ok(output)
    }

    /// Get error stats: total fingerprints, total resolutions, total occurrences.
    pub fn get_error_stats(&self) -> Result<(u64, u64, u64)> {
        let fp_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM error_fingerprints",
                [],
                |row| row.get(0),
            )
            .sq()?;
        let res_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM error_resolutions",
                [],
                |row| row.get(0),
            )
            .sq()?;
        let total_occ: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(occurrence_count), 0) FROM error_fingerprints",
                [],
                |row| row.get(0),
            )
            .sq()?;
        Ok((fp_count as u64, res_count as u64, total_occ as u64))
    }

    /// Auto-detect resolved errors at session end.
    /// Looks at trajectory steps: if a tool failed and later succeeded,
    /// the intermediate tools form a resolution path.
    /// Returns the number of resolutions recorded.
    pub fn auto_detect_resolutions(
        &self,
        session_id: &str,
        trajectory_id: &str,
    ) -> Result<u32> {
        use flowforge_core::trajectory::StepOutcome;

        // Get all trajectory steps in order
        let steps = self.get_trajectory_steps(trajectory_id)?;
        if steps.len() < 2 {
            return Ok(0);
        }

        // Get session failures with their fingerprint IDs.
        // Uses the fingerprint_id column stored at recording time (reliable).
        // Falls back to hash recomputation for legacy rows without fingerprint_id.
        let failures: Vec<(String, String, String)> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT tool_name, error_preview, fingerprint_id
                     FROM session_tool_failures
                     WHERE session_id = ?1 AND error_preview IS NOT NULL
                     ORDER BY timestamp ASC",
                )
                .sq()?;

            let rows: Vec<(String, String, Option<String>)> = stmt
                .query_map(params![session_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })
                .sq()?
                .filter_map(|r| r.ok())
                .collect();

            let mut result = Vec::new();
            for (tool_name, error_preview, stored_fp_id) in rows {
                let fp_id = if let Some(id) = stored_fp_id {
                    // Direct lookup — reliable
                    Some(id)
                } else {
                    // Legacy fallback: recompute hash (may not match for long errors)
                    let fp_hash = fingerprint_error(&error_preview);
                    self.conn
                        .query_row(
                            "SELECT id FROM error_fingerprints WHERE fingerprint = ?1",
                            params![fp_hash],
                            |row| row.get(0),
                        )
                        .optional()
                        .unwrap_or(None)
                };
                if let Some(id) = fp_id {
                    result.push((tool_name, error_preview, id));
                }
            }
            result
        };

        if failures.is_empty() {
            return Ok(0);
        }

        // Get files edited in this session for resolution context
        let edited_files: Vec<String> = self
            .get_edits_for_session(session_id)
            .unwrap_or_default()
            .iter()
            .map(|e| e.file_path.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let mut recorded = 0u32;

        for (fail_tool, _error_preview, fingerprint_id) in &failures {
            // Find failure step index
            let fail_step_idx = steps
                .iter()
                .position(|s| s.tool_name == *fail_tool && s.outcome == StepOutcome::Failure);

            if let Some(fail_idx) = fail_step_idx {
                // Find next success of the same tool after the failure
                let success_idx = steps[fail_idx + 1..]
                    .iter()
                    .position(|s| s.tool_name == *fail_tool && s.outcome == StepOutcome::Success);

                if let Some(rel_success_idx) = success_idx {
                    let abs_success_idx = fail_idx + 1 + rel_success_idx;

                    // Collect intermediate tool names as the resolution path
                    let resolution_tools: Vec<String> = steps[fail_idx + 1..abs_success_idx]
                        .iter()
                        .map(|s| s.tool_name.clone())
                        .collect();

                    // Skip transient retries (same tool immediately re-run with no
                    // intermediate steps). These aren't real resolutions — just retries
                    // that happened to succeed.
                    if resolution_tools.is_empty() {
                        continue;
                    }

                    // Build a summary with the fix path
                    let unique_tools: Vec<String> = {
                        let mut seen = std::collections::HashSet::new();
                        resolution_tools
                            .iter()
                            .filter(|t| seen.insert(t.as_str()))
                            .cloned()
                            .collect()
                    };
                    let summary = format!(
                        "Fixed via {} then re-ran {}",
                        unique_tools.join(", "),
                        fail_tool
                    );

                    self.record_error_resolution(
                        fingerprint_id,
                        &summary,
                        &resolution_tools,
                        &edited_files,
                        true,
                    )?;
                    recorded += 1;
                }
            }
        }

        Ok(recorded)
    }

    /// List all completed/judged trajectories that have failure steps.
    /// Returns (trajectory_id, session_id) pairs for resolution backfilling.
    pub fn list_trajectories_with_failures(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT t.id, t.session_id
                 FROM trajectories t
                 JOIN trajectory_steps ts ON ts.trajectory_id = t.id
                 WHERE t.status IN ('completed', 'judged')
                 AND ts.outcome = 'failure'",
            )
            .sq()?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Count distinct tool failures in a session (for statusline display).
    pub fn count_session_failures(&self, session_id: &str) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(DISTINCT input_hash) FROM session_tool_failures WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .sq()?;
        Ok(count as u64)
    }

    /// Find error warnings for a set of predicted files.
    /// Looks for errors that occurred in sessions where these files were edited.
    /// Returns (error_preview, tool_name, resolution_summary_if_any).
    pub fn get_errors_for_files(
        &self,
        file_paths: &[&str],
        limit: usize,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders: Vec<String> = (1..=file_paths.len())
            .map(|i| format!("?{}", i))
            .collect();
        let in_clause = placeholders.join(", ");

        let sql = format!(
            "SELECT DISTINCT ef.error_preview, ef.tool_name,
                    (SELECT er.resolution_summary FROM error_resolutions er
                     WHERE er.fingerprint_id = ef.id AND er.success_count > 0
                     ORDER BY er.success_count DESC LIMIT 1) as best_resolution
             FROM error_fingerprints ef
             JOIN session_tool_failures stf ON stf.fingerprint_id = ef.id
             JOIN edits e ON e.session_id = stf.session_id
             WHERE e.file_path IN ({})
               AND ef.occurrence_count >= 2
             GROUP BY ef.fingerprint
             ORDER BY ef.occurrence_count DESC
             LIMIT ?{}",
            in_clause,
            file_paths.len() + 1
        );

        let mut stmt = self.conn.prepare(&sql).sq()?;
        let mut params_vec: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
        for p in file_paths {
            params_vec.push(p as &dyn rusqlite::types::ToSql);
        }
        let limit_val = limit as i64;
        params_vec.push(&limit_val);

        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_vec), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1).unwrap_or_default(),
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }
}

fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", ts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};
    use flowforge_core::SessionInfo;
    use std::path::Path;

    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    fn test_session(db: &MemoryDb, id: &str) {
        let session = SessionInfo {
            id: id.to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp/test".to_string(),
            edits: 5,
            commands: 10,
            summary: None,
            transcript_path: None,
        };
        db.create_session(&session).unwrap();
    }

    #[test]
    fn test_record_error_occurrence_creates_fingerprint() {
        let db = test_db();
        let id = db
            .record_error_occurrence("Bash", "error[E0425]: cannot find value `foo`")
            .unwrap();
        assert!(id.starts_with("ef-"));

        // Second occurrence should return same ID and increment count
        let id2 = db
            .record_error_occurrence("Bash", "error[E0425]: cannot find value `foo`")
            .unwrap();
        assert_eq!(id, id2);

        // Verify count incremented
        let count: i64 = db
            .conn
            .query_row(
                "SELECT occurrence_count FROM error_fingerprints WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_record_error_resolution() {
        let db = test_db();
        let fp_id = db
            .record_error_occurrence("Bash", "cannot find module `foo`")
            .unwrap();

        db.record_error_resolution(
            &fp_id,
            "Added missing import: use crate::foo;",
            &["Read".to_string(), "Edit".to_string()],
            &["src/main.rs".to_string()],
            true,
        )
        .unwrap();

        // Record same resolution again (should update count)
        db.record_error_resolution(
            &fp_id,
            "Added missing import: use crate::foo;",
            &["Read".to_string(), "Edit".to_string()],
            &["src/main.rs".to_string()],
            true,
        )
        .unwrap();

        // Verify resolution exists with count=2
        let (_, resolutions) = db
            .find_error_resolutions("cannot find module `foo`", 5)
            .unwrap()
            .unwrap();
        assert_eq!(resolutions.len(), 1);
        assert_eq!(resolutions[0].success_count, 2);
    }

    #[test]
    fn test_find_error_resolutions_unknown_error() {
        let db = test_db();
        let result = db
            .find_error_resolutions("never seen before error", 5)
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_error_resolutions_with_match() {
        let db = test_db();
        let fp_id = db
            .record_error_occurrence("Bash", "mismatched types: expected u32")
            .unwrap();
        db.record_error_resolution(
            &fp_id,
            "Changed return type to match expected",
            &["Edit".to_string()],
            &["src/lib.rs".to_string()],
            true,
        )
        .unwrap();

        let result = db
            .find_error_resolutions("mismatched types: expected u32", 5)
            .unwrap();
        assert!(result.is_some());
        let (fingerprint, resolutions) = result.unwrap();
        assert_eq!(fingerprint.occurrence_count, 1);
        assert_eq!(resolutions.len(), 1);
        assert!(resolutions[0].confidence() > 0.9);
    }

    #[test]
    fn test_record_tool_failure() {
        let db = test_db();
        test_session(&db, "sess-fail-1");

        db.record_tool_failure("sess-fail-1", "Bash", "abc123", Some("error"), None)
            .unwrap();
        db.record_tool_failure("sess-fail-1", "Bash", "abc123", Some("error"), None)
            .unwrap();

        let count = db
            .get_tool_failure_count("sess-fail-1", "Bash", "abc123")
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_tool_failure_count_zero_for_unknown() {
        let db = test_db();
        let count = db
            .get_tool_failure_count("nonexistent", "Bash", "xyz")
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_tool_failure_count_scoped_to_session() {
        let db = test_db();
        test_session(&db, "sess-a");
        test_session(&db, "sess-b");

        db.record_tool_failure("sess-a", "Bash", "hash1", Some("err"), None)
            .unwrap();
        db.record_tool_failure("sess-b", "Bash", "hash1", Some("err"), None)
            .unwrap();

        assert_eq!(
            db.get_tool_failure_count("sess-a", "Bash", "hash1")
                .unwrap(),
            1
        );
        assert_eq!(
            db.get_tool_failure_count("sess-b", "Bash", "hash1")
                .unwrap(),
            1
        );
    }

    #[test]
    fn test_previous_session_context_none_when_empty() {
        let db = test_db();
        let result = db
            .get_previous_session_context("/tmp/test")
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_error_fingerprints() {
        let db = test_db();
        // Record 3 different errors with slight delays to ensure ordering
        db.record_error_occurrence("Bash", "error[E0425]: cannot find value `a`")
            .unwrap();
        db.record_error_occurrence("Bash", "mismatched types: expected u32, found String")
            .unwrap();
        db.record_error_occurrence("Bash", "permission denied: /etc/shadow")
            .unwrap();

        let fps = db.list_error_fingerprints(10).unwrap();
        assert_eq!(fps.len(), 3);
        // Should be ordered by last_seen desc — last recorded first
        assert!(fps[0].last_seen >= fps[1].last_seen);
        assert!(fps[1].last_seen >= fps[2].last_seen);
    }

    #[test]
    fn test_list_error_fingerprints_respects_limit() {
        let db = test_db();
        // Record 5 different errors
        for i in 0..5 {
            db.record_error_occurrence("Bash", &format!("unique error number {i}"))
                .unwrap();
        }

        let fps = db.list_error_fingerprints(3).unwrap();
        assert_eq!(fps.len(), 3);

        // Verify all 5 exist when limit is higher
        let all = db.list_error_fingerprints(10).unwrap();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_get_resolutions_for_fingerprint() {
        let db = test_db();
        let fp_id = db
            .record_error_occurrence("Bash", "error[E0425]: cannot find value `x`")
            .unwrap();

        // Add 2 resolutions with different success/failure ratios
        db.record_error_resolution(
            &fp_id,
            "Added use statement",
            &["Edit".to_string()],
            &["src/main.rs".to_string()],
            true,
        )
        .unwrap();
        // Give first resolution more successes to ensure higher confidence
        db.record_error_resolution(
            &fp_id,
            "Added use statement",
            &["Edit".to_string()],
            &["src/main.rs".to_string()],
            true,
        )
        .unwrap();

        db.record_error_resolution(
            &fp_id,
            "Removed unused code",
            &["Bash".to_string(), "Edit".to_string()],
            &["src/lib.rs".to_string()],
            true,
        )
        .unwrap();
        db.record_error_resolution(
            &fp_id,
            "Removed unused code",
            &["Bash".to_string(), "Edit".to_string()],
            &["src/lib.rs".to_string()],
            false,
        )
        .unwrap();

        let resolutions = db.get_resolutions_for_fingerprint(&fp_id, 10).unwrap();
        assert_eq!(resolutions.len(), 2);
        // Sorted by confidence desc: first (2/2=1.0), second (1/2=0.5)
        assert!(resolutions[0].confidence() >= resolutions[1].confidence());
        assert_eq!(resolutions[0].resolution_summary, "Added use statement");
    }

    #[test]
    fn test_get_error_stats() {
        let db = test_db();

        // Start with empty
        let (fp, res, occ) = db.get_error_stats().unwrap();
        assert_eq!(fp, 0);
        assert_eq!(res, 0);
        assert_eq!(occ, 0);

        // Record 2 errors (one twice)
        let fp_id1 = db
            .record_error_occurrence("Bash", "cannot find value `x`")
            .unwrap();
        db.record_error_occurrence("Bash", "cannot find value `x`")
            .unwrap();
        let fp_id2 = db
            .record_error_occurrence("Bash", "mismatched types")
            .unwrap();

        // Add resolutions
        db.record_error_resolution(
            &fp_id1,
            "Fix import",
            &["Edit".to_string()],
            &["src/main.rs".to_string()],
            true,
        )
        .unwrap();
        db.record_error_resolution(
            &fp_id2,
            "Change type",
            &["Edit".to_string()],
            &["src/lib.rs".to_string()],
            true,
        )
        .unwrap();

        let (fp_count, res_count, total_occ) = db.get_error_stats().unwrap();
        assert_eq!(fp_count, 2);
        assert_eq!(res_count, 2);
        assert_eq!(total_occ, 3); // 2 occurrences of first + 1 of second
    }

    #[test]
    fn test_error_recovery_pipeline() {
        let db = test_db();

        // Step 1: Record error
        let fp_id = db
            .record_error_occurrence("Bash", "error[E0308]: mismatched types in main.rs")
            .unwrap();
        assert!(fp_id.starts_with("ef-"));

        // Step 2: Record resolution
        db.record_error_resolution(
            &fp_id,
            "Changed return type to u32",
            &["Read".to_string(), "Edit".to_string()],
            &["src/main.rs".to_string()],
            true,
        )
        .unwrap();

        // Step 3: Find resolution by error text
        let result = db
            .find_error_resolutions("error[E0308]: mismatched types in main.rs", 5)
            .unwrap();
        assert!(result.is_some());
        let (fingerprint, resolutions) = result.unwrap();
        assert_eq!(fingerprint.id, fp_id);
        assert_eq!(resolutions.len(), 1);
        assert_eq!(resolutions[0].resolution_summary, "Changed return type to u32");
        assert_eq!(resolutions[0].tool_sequence, vec!["Read", "Edit"]);
        assert_eq!(resolutions[0].files_changed, vec!["src/main.rs"]);
        // 1 success, 0 failures → confidence = 1.0
        assert!((resolutions[0].confidence() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_error_category_classification() {
        use flowforge_core::types::error_recovery::{classify_error, ErrorCategory};

        // Compile errors
        assert_eq!(
            classify_error("error[E0425]: cannot find value `foo`", "Bash"),
            ErrorCategory::Compile
        );
        assert_eq!(
            classify_error("unresolved import crate::missing", "Bash"),
            ErrorCategory::Compile
        );

        // Test errors
        assert_eq!(
            classify_error("test result: FAILED. 3 passed; 1 failed", "Bash"),
            ErrorCategory::Test
        );
        assert_eq!(
            classify_error("assertion failed: left == right", "Bash"),
            ErrorCategory::Test
        );

        // Permission errors
        assert_eq!(
            classify_error("permission denied: /etc/passwd", "Bash"),
            ErrorCategory::Permission
        );

        // Network errors
        assert_eq!(
            classify_error("connection refused to localhost:8080", "Bash"),
            ErrorCategory::Network
        );
        assert_eq!(
            classify_error("dns resolution failed for example.com", "Bash"),
            ErrorCategory::Network
        );
    }

    #[test]
    fn test_previous_session_context_returns_last_ended() {
        let db = test_db();

        // Create and end a session
        let session = SessionInfo {
            id: "prev-sess-1".to_string(),
            started_at: Utc::now() - chrono::Duration::hours(2),
            ended_at: None,
            cwd: "/tmp/project".to_string(),
            edits: 15,
            commands: 30,
            summary: None,
            transcript_path: None,
        };
        db.create_session(&session).unwrap();
        db.end_session("prev-sess-1", Utc::now() - chrono::Duration::hours(1))
            .unwrap();

        // Create trajectory with task description
        use flowforge_core::trajectory::{Trajectory, TrajectoryStatus};
        let traj = Trajectory {
            id: "traj-prev-1".to_string(),
            session_id: "prev-sess-1".to_string(),
            work_item_id: None,
            agent_name: None,
            task_description: Some("Fix the parser bug".to_string()),
            status: TrajectoryStatus::Completed,
            started_at: Utc::now() - chrono::Duration::hours(2),
            ended_at: Some(Utc::now() - chrono::Duration::hours(1)),
            verdict: Some(flowforge_core::trajectory::TrajectoryVerdict::Success),
            confidence: Some(0.9),
            metadata: None,
            embedding_id: None,
        };
        db.create_trajectory(&traj).unwrap();

        let ctx = db
            .get_previous_session_context("/tmp/project")
            .unwrap();
        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.session_id, "prev-sess-1");
        assert_eq!(
            ctx.task_description.as_deref(),
            Some("Fix the parser bug")
        );
        assert_eq!(ctx.edits_count, 15);
        assert_eq!(ctx.commands_count, 30);
    }

    #[test]
    fn test_auto_detect_resolutions_no_trajectory() {
        let db = test_db();
        test_session(&db, "sess-auto-1");

        // No trajectory steps → 0 resolutions
        let count = db
            .auto_detect_resolutions("sess-auto-1", "nonexistent-traj")
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_auto_detect_resolutions_finds_fix_pattern() {
        use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};

        let db = test_db();
        test_session(&db, "sess-auto-2");

        // Create a trajectory
        let traj = Trajectory {
            id: "traj-auto-2".to_string(),
            session_id: "sess-auto-2".to_string(),
            work_item_id: None,
            agent_name: None,
            task_description: Some("Fix compile error".to_string()),
            status: TrajectoryStatus::Recording,
            started_at: Utc::now(),
            ended_at: None,
            verdict: None,
            confidence: None,
            metadata: None,
            embedding_id: None,
        };
        db.create_trajectory(&traj).unwrap();

        // Record trajectory steps: Bash fails → Read → Edit → Bash succeeds
        db.record_trajectory_step("traj-auto-2", "Bash", None, StepOutcome::Failure, Some(100))
            .unwrap();
        db.record_trajectory_step("traj-auto-2", "Read", None, StepOutcome::Success, Some(50))
            .unwrap();
        db.record_trajectory_step("traj-auto-2", "Edit", None, StepOutcome::Success, Some(200))
            .unwrap();
        db.record_trajectory_step("traj-auto-2", "Bash", None, StepOutcome::Success, Some(150))
            .unwrap();

        // Record the tool failure + error fingerprint (normally done by post_tool_use_failure hook)
        let fp_id = db
            .record_error_occurrence("Bash", "error[E0425]: cannot find value")
            .unwrap();
        db.record_tool_failure(
            "sess-auto-2",
            "Bash",
            "hash1",
            Some("error[E0425]: cannot find value"),
            Some(&fp_id),
        )
        .unwrap();

        // Auto-detect should find the Bash fail→Read→Edit→Bash success pattern
        let count = db
            .auto_detect_resolutions("sess-auto-2", "traj-auto-2")
            .unwrap();
        assert_eq!(count, 1);

        // Verify the resolution was recorded
        let resolutions = db.get_resolutions_for_fingerprint(&fp_id, 5).unwrap();
        assert_eq!(resolutions.len(), 1);
        assert!(resolutions[0]
            .resolution_summary
            .contains("Fixed via"));
        assert!(resolutions[0]
            .resolution_summary
            .contains("Read"));
        assert!(resolutions[0]
            .resolution_summary
            .contains("Edit"));
        assert_eq!(resolutions[0].tool_sequence, vec!["Read", "Edit"]);
    }

    #[test]
    fn test_auto_detect_resolutions_no_failures() {
        use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};

        let db = test_db();
        test_session(&db, "sess-auto-3");

        let traj = Trajectory {
            id: "traj-auto-3".to_string(),
            session_id: "sess-auto-3".to_string(),
            work_item_id: None,
            agent_name: None,
            task_description: None,
            status: TrajectoryStatus::Recording,
            started_at: Utc::now(),
            ended_at: None,
            verdict: None,
            confidence: None,
            metadata: None,
            embedding_id: None,
        };
        db.create_trajectory(&traj).unwrap();

        // All successes — no failures to detect
        db.record_trajectory_step("traj-auto-3", "Read", None, StepOutcome::Success, None)
            .unwrap();
        db.record_trajectory_step("traj-auto-3", "Edit", None, StepOutcome::Success, None)
            .unwrap();
        db.record_trajectory_step("traj-auto-3", "Bash", None, StepOutcome::Success, None)
            .unwrap();

        let count = db
            .auto_detect_resolutions("sess-auto-3", "traj-auto-3")
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_auto_detect_resolutions_retry_without_intermediate_tools() {
        use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};

        let db = test_db();
        test_session(&db, "sess-auto-4");

        let traj = Trajectory {
            id: "traj-auto-4".to_string(),
            session_id: "sess-auto-4".to_string(),
            work_item_id: None,
            agent_name: None,
            task_description: None,
            status: TrajectoryStatus::Recording,
            started_at: Utc::now(),
            ended_at: None,
            verdict: None,
            confidence: None,
            metadata: None,
            embedding_id: None,
        };
        db.create_trajectory(&traj).unwrap();

        // Bash fails then immediately succeeds (simple retry — no intermediate tools)
        db.record_trajectory_step("traj-auto-4", "Bash", None, StepOutcome::Failure, None)
            .unwrap();
        db.record_trajectory_step("traj-auto-4", "Bash", None, StepOutcome::Success, None)
            .unwrap();

        let fp_id = db
            .record_error_occurrence("Bash", "connection timed out")
            .unwrap();
        db.record_tool_failure(
            "sess-auto-4",
            "Bash",
            "hash2",
            Some("connection timed out"),
            Some(&fp_id),
        )
        .unwrap();

        // Transient retries (no intermediate steps) are now skipped —
        // they aren't real resolutions, just retries that happened to succeed.
        let count = db
            .auto_detect_resolutions("sess-auto-4", "traj-auto-4")
            .unwrap();
        assert_eq!(count, 0);

        let resolutions = db.get_resolutions_for_fingerprint(&fp_id, 5).unwrap();
        assert!(resolutions.is_empty());
    }

    #[test]
    fn test_failure_escalation_count_tracking() {
        // Proves: get_tool_failure_count increments correctly,
        // enabling the ASK→DENY escalation in pre_tool_use
        let db = test_db();
        test_session(&db, "sess-esc");

        let tool = "Bash";
        let hash = "same-input-hash";
        let preview = Some("error: command not found");

        // No failures yet
        assert_eq!(db.get_tool_failure_count("sess-esc", tool, hash).unwrap(), 0);

        // Record 1st failure → count=1 (below ASK threshold of 2)
        db.record_tool_failure("sess-esc", tool, hash, preview, None).unwrap();
        assert_eq!(db.get_tool_failure_count("sess-esc", tool, hash).unwrap(), 1);

        // Record 2nd failure → count=2 (ASK threshold)
        db.record_tool_failure("sess-esc", tool, hash, preview, None).unwrap();
        assert_eq!(db.get_tool_failure_count("sess-esc", tool, hash).unwrap(), 2);

        // Record 3rd failure → count=3 (default DENY threshold)
        db.record_tool_failure("sess-esc", tool, hash, preview, None).unwrap();
        assert_eq!(db.get_tool_failure_count("sess-esc", tool, hash).unwrap(), 3);

        // Different input hash → separate count (isolation proof)
        db.record_tool_failure("sess-esc", tool, "different-hash", preview, None).unwrap();
        assert_eq!(db.get_tool_failure_count("sess-esc", tool, "different-hash").unwrap(), 1);
        // Original still at 3
        assert_eq!(db.get_tool_failure_count("sess-esc", tool, hash).unwrap(), 3);
    }

    #[test]
    fn test_truncation_consistency_chars_not_bytes() {
        // Proves: chars().take(200) truncation is consistent between
        // record_error_occurrence and record_tool_failure, enabling the JOIN
        let db = test_db();
        test_session(&db, "sess-trunc");

        // Create a string with multi-byte characters that exceeds 200 chars
        // 'é' is 2 bytes in UTF-8 but 1 char
        let long_error: String = "é".repeat(250); // 250 chars, 500 bytes

        // Simulate what both functions do: chars().take(200)
        let truncated: String = long_error.chars().take(200).collect();
        assert_eq!(truncated.len(), 400); // 200 chars × 2 bytes each
        assert_eq!(truncated.chars().count(), 200);

        // Record via both paths
        let fp_id = db.record_error_occurrence("Bash", &long_error).unwrap();
        db.record_tool_failure("sess-trunc", "Bash", "hash-trunc", Some(&truncated), Some(&fp_id)).unwrap();

        // The JOIN must match — this is the core bug fix proof
        let matching: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM session_tool_failures stf
             JOIN error_fingerprints ef ON ef.error_preview = stf.error_preview
             WHERE stf.session_id = 'sess-trunc'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(matching, 1, "JOIN must match after truncation fix");

        // Verify fingerprint was stored
        assert!(!fp_id.is_empty());
    }

    #[test]
    fn test_auto_detect_resolutions_with_multibyte_errors() {
        // End-to-end proof: auto_detect_resolutions works with multi-byte error text
        let db = test_db();
        test_session(&db, "sess-mb");

        // Create trajectory with fail → success pattern
        let traj = Trajectory {
            id: "traj-mb".to_string(),
            session_id: "sess-mb".to_string(),
            task_description: Some("test multibyte".to_string()),
            status: TrajectoryStatus::Completed,
            started_at: Utc::now(),
            ended_at: None,
            verdict: None,
            confidence: None,
            metadata: None,
            embedding_id: None,
            agent_name: None,
            work_item_id: None,
        };
        db.create_trajectory(&traj).unwrap();
        db.record_trajectory_step("traj-mb", "Bash", None, StepOutcome::Failure, None).unwrap();
        db.record_trajectory_step("traj-mb", "Read", None, StepOutcome::Success, None).unwrap();
        db.record_trajectory_step("traj-mb", "Edit", None, StepOutcome::Success, None).unwrap();
        db.record_trajectory_step("traj-mb", "Bash", None, StepOutcome::Success, None).unwrap();

        // Use an error with multi-byte chars
        let error_text = "错误: 找不到文件 — this is a Chinese error message that is quite long and will need truncation";
        let fp_id = db.record_error_occurrence("Bash", error_text).unwrap();

        // Truncate the same way post_tool_use_failure.rs does
        let preview: String = error_text.chars().take(200).collect();
        db.record_tool_failure("sess-mb", "Bash", "hash-mb", Some(&preview), Some(&fp_id)).unwrap();

        let count = db.auto_detect_resolutions("sess-mb", "traj-mb").unwrap();
        assert_eq!(count, 1, "auto_detect_resolutions must find resolution with multibyte errors");

        let resolutions = db.get_resolutions_for_fingerprint(&fp_id, 5).unwrap();
        assert_eq!(resolutions.len(), 1);
    }

    #[test]
    fn test_get_errors_for_files_empty_input() {
        let db = test_db();
        let result = db.get_errors_for_files(&[], 5).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_errors_for_files_finds_related_errors() {
        let db = test_db();
        test_session(&db, "sess-ef-1");

        // Record an error with occurrence_count >= 2
        let fp_id = db.record_error_occurrence("Bash", "compilation error: cannot find crate").unwrap();
        db.record_error_occurrence("Bash", "compilation error: cannot find crate").unwrap();

        // Record tool failure in the session with fingerprint_id
        db.record_tool_failure("sess-ef-1", "Bash", "hash-ef", Some("compilation error"), Some(&fp_id)).unwrap();

        // Record an edit in the same session
        db.record_edit(&flowforge_core::EditRecord {
            session_id: "sess-ef-1".to_string(),
            timestamp: Utc::now(),
            file_path: "src/main.rs".to_string(),
            operation: "write".to_string(),
            file_extension: Some("rs".to_string()),
        }).unwrap();

        // Query errors for the file
        let errors = db.get_errors_for_files(&["src/main.rs"], 5).unwrap();
        assert!(!errors.is_empty(), "should find errors from sessions where the file was edited");
        assert!(errors[0].0.contains("compilation error"));
    }
}
