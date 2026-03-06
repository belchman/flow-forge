use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};

use flowforge_core::{
    work_tracking::{WorkDb, WorkStealing},
    Result, WorkEvent, WorkFilter, WorkItem, WorkStatus,
};

use super::row_parsers::parse_work_item_row;
use super::{parse_datetime, MemoryDb, SqliteExt};

impl MemoryDb {
    // ── Work Items ──

    pub fn create_work_item(&self, item: &WorkItem) -> Result<()> {
        let labels_json = serde_json::to_string(&item.labels).unwrap_or_else(|_| "[]".to_string());
        let priority = item.priority.clamp(0, 4);
        let status_str = item.status.to_string();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO work_items
                 (id, external_id, backend, item_type, title, description, status, assignee,
                  parent_id, priority, labels, created_at, updated_at, completed_at, session_id, metadata,
                  claimed_by, claimed_at, last_heartbeat, progress, stealable)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
                params![
                    item.id,
                    item.external_id,
                    item.backend,
                    item.item_type,
                    item.title,
                    item.description,
                    status_str,
                    item.assignee,
                    item.parent_id,
                    priority,
                    labels_json,
                    item.created_at.to_rfc3339(),
                    item.updated_at.to_rfc3339(),
                    item.completed_at.map(|t| t.to_rfc3339()),
                    item.session_id,
                    item.metadata,
                    item.claimed_by,
                    item.claimed_at.map(|t| t.to_rfc3339()),
                    item.last_heartbeat.map(|t| t.to_rfc3339()),
                    item.progress,
                    item.stealable as i32,
                ],
            )
            .sq()?;
        Ok(())
    }

    pub fn get_work_item(&self, id: &str) -> Result<Option<WorkItem>> {
        self.conn
            .query_row(
                "SELECT id, external_id, backend, item_type, title, description, status, assignee,
                        parent_id, priority, labels, created_at, updated_at, completed_at, session_id, metadata,
                        claimed_by, claimed_at, last_heartbeat, progress, stealable
                 FROM work_items WHERE id = ?1",
                params![id],
                |row| Ok(parse_work_item_row(row)),
            )
            .optional()
            .sq()
    }

    pub fn get_work_item_by_external_id(&self, external_id: &str) -> Result<Option<WorkItem>> {
        self.conn
            .query_row(
                "SELECT id, external_id, backend, item_type, title, description, status, assignee,
                        parent_id, priority, labels, created_at, updated_at, completed_at, session_id, metadata,
                        claimed_by, claimed_at, last_heartbeat, progress, stealable
                 FROM work_items WHERE external_id = ?1",
                params![external_id],
                |row| Ok(parse_work_item_row(row)),
            )
            .optional()
            .sq()
    }

    pub fn update_work_item_status(&self, id: &str, status: WorkStatus) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let completed_at = if status == WorkStatus::Completed {
            Some(now.clone())
        } else {
            None
        };
        let status_str = status.to_string();
        self.conn
            .execute(
                "UPDATE work_items SET status = ?1, updated_at = ?2, completed_at = COALESCE(?3, completed_at)
                 WHERE id = ?4",
                params![status_str, now, completed_at, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn update_work_item_assignee(&self, id: &str, assignee: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE work_items SET assignee = ?1, updated_at = ?2 WHERE id = ?3",
                params![assignee, now, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn list_work_items(&self, filter: &WorkFilter) -> Result<Vec<WorkItem>> {
        let mut sql = String::from(
            "SELECT id, external_id, backend, item_type, title, description, status, assignee,
                    parent_id, priority, labels, created_at, updated_at, completed_at, session_id, metadata,
                    claimed_by, claimed_at, last_heartbeat, progress, stealable
             FROM work_items WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(status) = filter.status {
            param_values.push(Box::new(status.to_string()));
            sql.push_str(&format!(" AND status = ?{}", param_values.len()));
        }
        if let Some(ref item_type) = filter.item_type {
            param_values.push(Box::new(item_type.clone()));
            sql.push_str(&format!(" AND item_type = ?{}", param_values.len()));
        }
        if let Some(ref backend) = filter.backend {
            param_values.push(Box::new(backend.clone()));
            sql.push_str(&format!(" AND backend = ?{}", param_values.len()));
        }
        if let Some(ref assignee) = filter.assignee {
            param_values.push(Box::new(assignee.clone()));
            sql.push_str(&format!(" AND assignee = ?{}", param_values.len()));
        }
        if let Some(ref parent_id) = filter.parent_id {
            param_values.push(Box::new(parent_id.clone()));
            sql.push_str(&format!(" AND parent_id = ?{}", param_values.len()));
        }
        if let Some(stealable) = filter.stealable {
            param_values.push(Box::new(stealable as i32));
            sql.push_str(&format!(" AND stealable = ?{}", param_values.len()));
        }
        if let Some(ref claimed_by) = filter.claimed_by {
            param_values.push(Box::new(claimed_by.clone()));
            sql.push_str(&format!(" AND claimed_by = ?{}", param_values.len()));
        }

        sql.push_str(" ORDER BY updated_at DESC");

        let limit = filter.limit.unwrap_or(100);
        param_values.push(Box::new(limit as i64));
        sql.push_str(&format!(" LIMIT ?{}", param_values.len()));

        let mut stmt = self.conn.prepare(&sql).sq()?;

        let params_slice: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params_slice.as_slice(), |row| Ok(parse_work_item_row(row)))
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn update_work_item_backend(&self, id: &str, backend: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE work_items SET backend = ?1, updated_at = ?2 WHERE id = ?3",
                params![backend, now, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn update_work_item_external_id(&self, id: &str, external_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE work_items SET external_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![external_id, now, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn delete_work_item(&self, id: &str) -> Result<()> {
        self.with_transaction(|| {
            self.conn
                .execute(
                    "DELETE FROM work_events WHERE work_item_id = ?1",
                    params![id],
                )
                .sq()?;
            self.conn
                .execute("DELETE FROM work_items WHERE id = ?1", params![id])
                .sq()?;
            Ok(())
        })
    }

    /// Find a work item by title, preferring in-progress items.
    pub fn get_work_item_by_title(&self, title: &str) -> Result<Option<WorkItem>> {
        // Try in-progress first, then any non-completed
        self.conn
            .query_row(
                "SELECT id, external_id, backend, item_type, title, description, status, assignee,
                        parent_id, priority, labels, created_at, updated_at, completed_at, session_id, metadata,
                        claimed_by, claimed_at, last_heartbeat, progress, stealable
                 FROM work_items WHERE title = ?1 AND status != 'completed'
                 ORDER BY CASE status WHEN 'in_progress' THEN 0 ELSE 1 END, updated_at DESC
                 LIMIT 1",
                params![title],
                |row| Ok(parse_work_item_row(row)),
            )
            .optional()
            .sq()
    }

    pub fn count_work_items_by_status(&self, status: WorkStatus) -> Result<u64> {
        let status_str = status.to_string();
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM work_items WHERE status = ?1",
                params![status_str],
                |row| row.get(0),
            )
            .sq()
    }

    // ── Work Tracking Config ──

    pub fn get_work_config(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM work_tracking_config WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .sq()
    }

    pub fn set_work_config(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO work_tracking_config (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .sq()?;
        Ok(())
    }

    // ── Work-Stealing ──

    pub fn claim_work_item(&self, id: &str, session_id: &str) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let updated = self
            .conn
            .execute(
                "UPDATE work_items SET claimed_by = ?1, claimed_at = ?2, last_heartbeat = ?2, stealable = 0
                 WHERE id = ?3 AND (claimed_by IS NULL OR stealable = 1)",
                params![session_id, now, id],
            )
            .sq()?;
        Ok(updated > 0)
    }

    pub fn release_work_item(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE work_items SET claimed_by = NULL, claimed_at = NULL, last_heartbeat = NULL,
                 stealable = 0, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn get_last_heartbeat_time(&self, session_id: &str) -> Result<Option<DateTime<Utc>>> {
        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT MAX(last_heartbeat) FROM work_items WHERE claimed_by = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .sq()?
            .flatten();
        Ok(result.map(parse_datetime))
    }

    pub fn update_heartbeat(&self, session_id: &str) -> Result<u64> {
        let now = Utc::now().to_rfc3339();
        let count = self
            .conn
            .execute(
                "UPDATE work_items SET last_heartbeat = ?1 WHERE claimed_by = ?2",
                params![now, session_id],
            )
            .sq()?;
        Ok(count as u64)
    }

    pub fn update_progress(&self, id: &str, progress: i32) -> Result<()> {
        let clamped = progress.clamp(0, 100);
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE work_items SET progress = ?1, updated_at = ?2 WHERE id = ?3",
                params![clamped, now, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn mark_stale_items_stealable(&self, stale_mins: u64, min_progress: i32) -> Result<u64> {
        let threshold = Utc::now() - chrono::Duration::minutes(stale_mins as i64);
        let count = self
            .conn
            .execute(
                "UPDATE work_items SET stealable = 1
                 WHERE claimed_by IS NOT NULL AND stealable = 0
                   AND last_heartbeat < ?1 AND progress < ?2
                   AND status = 'in_progress'",
                params![threshold.to_rfc3339(), min_progress],
            )
            .sq()?;
        Ok(count as u64)
    }

    pub fn auto_release_abandoned(&self, abandon_mins: u64) -> Result<u64> {
        let threshold = Utc::now() - chrono::Duration::minutes(abandon_mins as i64);
        let now = Utc::now().to_rfc3339();
        let count = self
            .conn
            .execute(
                "UPDATE work_items SET claimed_by = NULL, claimed_at = NULL,
                 last_heartbeat = NULL, stealable = 0, status = 'pending', updated_at = ?1
                 WHERE claimed_by IS NOT NULL AND last_heartbeat < ?2
                   AND status = 'in_progress'",
                params![now, threshold.to_rfc3339()],
            )
            .sq()?;
        Ok(count as u64)
    }

    pub fn get_stealable_items(&self, limit: usize) -> Result<Vec<WorkItem>> {
        let filter = WorkFilter {
            stealable: Some(true),
            limit: Some(limit),
            ..Default::default()
        };
        self.list_work_items(&filter)
    }

    pub fn steal_work_item(&self, id: &str, new_session_id: &str) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let updated = self
            .conn
            .execute(
                "UPDATE work_items SET claimed_by = ?1, claimed_at = ?2, last_heartbeat = ?2,
                 stealable = 0,
                 progress = CASE WHEN progress >= 50 THEN progress ELSE 0 END
                 WHERE id = ?3 AND stealable = 1",
                params![new_session_id, now, id],
            )
            .sq()?;
        Ok(updated > 0)
    }

    // ── Active Work-Stealing helpers ──

    /// Get count of in-progress items claimed by a session.
    pub fn get_session_load(&self, session_id: &str) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM work_items WHERE claimed_by = ?1 AND status = 'in_progress'",
                params![session_id],
                |row| row.get(0),
            )
            .sq()
    }

    /// Tiered progress-aware stale detection.
    /// Higher progress items get longer grace periods before being marked stealable.
    pub fn detect_stale_tiered(
        &self,
        base_stale_mins: u64,
        max_steal_count: u32,
        steal_cooldown_mins: u64,
    ) -> Result<u64> {
        let now = Utc::now();
        let cooldown_threshold =
            (now - chrono::Duration::minutes(steal_cooldown_mins as i64)).to_rfc3339();

        // Tiered thresholds based on progress
        let tiers: [(i32, i32, f64); 4] = [
            (0, 25, 1.0),   // 0-25%: standard threshold
            (25, 50, 1.5),  // 25-50%: 1.5x
            (50, 80, 2.0),  // 50-80%: 2x
            (80, 100, 0.0), // 80%+: never stolen
        ];

        let mut total_marked = 0u64;
        for (min_p, max_p, multiplier) in &tiers {
            if *multiplier == 0.0 {
                continue; // skip 80%+ tier
            }
            let stale_mins = (base_stale_mins as f64 * multiplier) as i64;
            let threshold = (now - chrono::Duration::minutes(stale_mins)).to_rfc3339();

            let count = self
                .conn
                .execute(
                    "UPDATE work_items SET stealable = 1
                     WHERE claimed_by IS NOT NULL AND stealable = 0
                       AND last_heartbeat < ?1
                       AND progress >= ?2 AND progress < ?3
                       AND status = 'in_progress'
                       AND steal_count < ?4
                       AND (last_stolen_at IS NULL OR last_stolen_at < ?5)",
                    params![threshold, min_p, max_p, max_steal_count, cooldown_threshold],
                )
                .sq()?;
            total_marked += count as u64;
        }

        Ok(total_marked)
    }

    /// Steal with anti-thrashing: increments steal_count, sets last_stolen_at.
    pub fn steal_work_item_safe(
        &self,
        id: &str,
        new_session_id: &str,
        max_steal_count: u32,
    ) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let updated = self
            .conn
            .execute(
                "UPDATE work_items SET claimed_by = ?1, claimed_at = ?2, last_heartbeat = ?2,
                 stealable = 0, progress = 0, steal_count = steal_count + 1, last_stolen_at = ?2
                 WHERE id = ?3 AND stealable = 1 AND steal_count < ?4",
                params![new_session_id, now, id, max_steal_count],
            )
            .sq()?;
        Ok(updated > 0)
    }

    /// Claim with load awareness: atomic check-and-claim in a single UPDATE
    /// to prevent race conditions where two sessions both pass the load check.
    pub fn claim_work_item_load_aware(
        &self,
        id: &str,
        session_id: &str,
        max_concurrent: u64,
    ) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let updated = self
            .conn
            .execute(
                "UPDATE work_items SET claimed_by = ?1, claimed_at = ?2, last_heartbeat = ?2, stealable = 0
                 WHERE id = ?3 AND (claimed_by IS NULL OR stealable = 1)
                   AND (SELECT COUNT(*) FROM work_items WHERE claimed_by = ?1 AND status = 'in_progress') < ?4",
                params![session_id, now, id, max_concurrent],
            )
            .sq()?;
        Ok(updated > 0)
    }

    /// Get the last stale scan timestamp for rate-limiting.
    pub fn get_last_stale_scan(&self) -> Result<Option<String>> {
        self.get_meta("last_stale_scan")
    }

    /// Update the last stale scan timestamp.
    pub fn set_last_stale_scan(&self) -> Result<()> {
        self.set_meta("last_stale_scan", &Utc::now().to_rfc3339())
    }
}

impl WorkDb for MemoryDb {
    fn create_work_item(&self, item: &WorkItem) -> Result<()> {
        self.create_work_item(item)
    }
    fn get_work_item(&self, id: &str) -> Result<Option<WorkItem>> {
        self.get_work_item(id)
    }
    fn get_work_item_by_external_id(&self, external_id: &str) -> Result<Option<WorkItem>> {
        self.get_work_item_by_external_id(external_id)
    }
    fn update_work_item_status(&self, id: &str, status: WorkStatus) -> Result<()> {
        self.update_work_item_status(id, status)
    }
    fn update_work_item_assignee(&self, id: &str, assignee: &str) -> Result<()> {
        self.update_work_item_assignee(id, assignee)
    }
    fn update_work_item_backend(&self, id: &str, backend: &str) -> Result<()> {
        self.update_work_item_backend(id, backend)
    }
    fn update_work_item_external_id(&self, id: &str, external_id: &str) -> Result<()> {
        self.update_work_item_external_id(id, external_id)
    }
    fn list_work_items(&self, filter: &WorkFilter) -> Result<Vec<WorkItem>> {
        self.list_work_items(filter)
    }
    fn delete_work_item(&self, id: &str) -> Result<()> {
        self.delete_work_item(id)
    }
    fn count_work_items_by_status(&self, status: WorkStatus) -> Result<u64> {
        self.count_work_items_by_status(status)
    }
    fn record_work_event(&self, event: &WorkEvent) -> Result<i64> {
        self.record_work_event(event)
    }
    fn get_work_events(&self, work_item_id: &str, limit: usize) -> Result<Vec<WorkEvent>> {
        self.get_work_events(work_item_id, limit)
    }
    fn get_recent_work_events(&self, limit: usize) -> Result<Vec<WorkEvent>> {
        self.get_recent_work_events(limit)
    }
}

impl WorkStealing for MemoryDb {
    fn claim_work_item(&self, id: &str, session_id: &str) -> Result<bool> {
        self.claim_work_item(id, session_id)
    }
    fn release_work_item(&self, id: &str) -> Result<()> {
        self.release_work_item(id)
    }
    fn update_heartbeat(&self, session_id: &str) -> Result<u64> {
        self.update_heartbeat(session_id)
    }
    fn update_progress(&self, id: &str, progress: i32) -> Result<()> {
        self.update_progress(id, progress)
    }
    fn mark_stale_items_stealable(&self, stale_mins: u64, min_progress: i32) -> Result<u64> {
        self.mark_stale_items_stealable(stale_mins, min_progress)
    }
    fn auto_release_abandoned(&self, abandon_mins: u64) -> Result<u64> {
        self.auto_release_abandoned(abandon_mins)
    }
    fn get_stealable_items(&self, limit: usize) -> Result<Vec<WorkItem>> {
        self.get_stealable_items(limit)
    }
    fn steal_work_item(&self, id: &str, new_session_id: &str) -> Result<bool> {
        self.steal_work_item(id, new_session_id)
    }
}
