use chrono::Utc;

use flowforge_core::{LongTermPattern, Result};

use super::PatternStore;

impl<'a> PatternStore<'a> {
    /// Promote eligible short-term patterns to long-term.
    /// Criteria: usage >= min_usage AND confidence >= min_confidence
    /// AND effectiveness score not below failure-correlation threshold.
    pub fn promote_eligible(&self) -> Result<u32> {
        let patterns = self.db.get_all_patterns_short()?;
        let mut promoted = 0;

        // Batch-fetch effectiveness scores to avoid N+1 queries
        let candidate_ids: Vec<String> = patterns
            .iter()
            .filter(|p| {
                p.usage_count >= self.config.promotion_min_usage
                    && p.confidence >= self.config.promotion_min_confidence
            })
            .map(|p| p.id.clone())
            .collect();
        let eff_scores = self.db.get_effectiveness_scores_batch(&candidate_ids)?;

        for p in &patterns {
            if p.usage_count >= self.config.promotion_min_usage
                && p.confidence >= self.config.promotion_min_confidence
            {
                // Gate: block promotion if pattern has high failure correlation
                let eff = eff_scores.get(&p.id);
                let (eff_samples, eff_score) =
                    eff.map(|e| (e.samples, e.score)).unwrap_or((0, 0.0));
                if eff_samples >= self.config.demotion_min_feedback
                    && eff_score < self.config.promotion_failure_correlation_max
                {
                    continue; // Too many failures correlated with this pattern
                }
                let now = Utc::now();
                let long_pattern = LongTermPattern {
                    id: p.id.clone(),
                    content: p.content.clone(),
                    category: p.category.clone(),
                    confidence: p.confidence,
                    usage_count: p.usage_count,
                    success_count: 0,
                    failure_count: 0,
                    created_at: p.created_at,
                    promoted_at: now,
                    last_used: p.last_used,
                    embedding_id: p.embedding_id,
                };

                self.db.with_transaction(|| {
                    self.db.store_pattern_long(&long_pattern)?;
                    // Update vector source_type from pattern_short → pattern_long
                    if let Some(eid) = p.embedding_id {
                        self.db.update_vector_source_type(eid, "pattern_long")?;
                    }
                    self.db.delete_pattern_short(&p.id)?;
                    Ok(())
                })?;
                promoted += 1;
            }
        }

        // Invalidate HNSW cache since tier mappings changed
        if promoted > 0 {
            *self.hnsw_cache.borrow_mut() = None;
        }

        Ok(promoted)
    }

    /// Demote long-term patterns with high failure ratios back to... nowhere.
    /// Patterns that have enough feedback and a failure ratio above the threshold are deleted.
    pub fn demote_failing(&self) -> Result<u32> {
        let patterns = self.db.get_all_patterns_long()?;
        let mut demoted = 0;

        for p in &patterns {
            let total_feedback = p.success_count + p.failure_count;
            if total_feedback < self.config.demotion_min_feedback {
                continue;
            }
            let failure_ratio = p.failure_count as f64 / total_feedback as f64;
            if failure_ratio >= self.config.demotion_failure_ratio {
                self.db.with_transaction(|| {
                    self.db.delete_pattern_long(&p.id)?;
                    self.db.delete_vectors_for_source("pattern_long", &p.id)?;
                    Ok(())
                })?;
                demoted += 1;
            }
        }

        if demoted > 0 {
            *self.hnsw_cache.borrow_mut() = None;
        }

        Ok(demoted)
    }

    /// Run consolidation: promotion, decay, demotion, expiration, deduplication, and migration.
    pub fn consolidate(&self) -> Result<()> {
        // 1. Promote eligible patterns
        self.promote_eligible()?;

        // 1b. Demote failing long-term patterns
        self.demote_failing()?;

        // 2. Apply confidence decay
        self.apply_decay()?;

        // 2b. Enforce long_term_max
        self.enforce_long_term_max()?;

        // 3. Expire old short-term patterns (TTL-based)
        self.expire_short_term()?;

        // 4. Deduplicate similar patterns (using stored vectors)
        self.deduplicate()?;

        // 5. Re-embed all vectors if embedding version changed
        self.migrate_embeddings()?;

        // 6. Re-cluster if needed
        self.maybe_recluster()?;

        Ok(())
    }

    /// Get cluster-aware decay multiplier for a pattern.
    /// Uses pre-loaded cluster sizes to avoid per-pattern DB lookups.
    fn decay_multiplier_from_cache(
        &self,
        embedding_id: Option<i64>,
        cluster_sizes: &std::collections::HashMap<i64, i64>,
    ) -> f64 {
        if let Some(eid) = embedding_id {
            if let Some(&member_count) = cluster_sizes.get(&eid) {
                if member_count > 10 {
                    return self.config.cluster_decay_active_factor; // Active cluster, slow decay
                }
                return 1.0; // Small cluster, normal decay
            }
        }
        self.config.cluster_decay_isolated_factor // Outlier, fast decay
    }

    /// Apply confidence decay based on time since last use. (A6)
    /// Pre-loads all cluster sizes in one query to avoid N+1.
    pub(super) fn apply_decay(&self) -> Result<()> {
        let now = Utc::now();

        // Pre-load cluster sizes for all pattern vectors in one JOIN query
        let cluster_sizes = self.db.get_vector_cluster_sizes()?;

        // Short-term patterns: decay at configured rate * cluster multiplier
        let short_patterns = self.db.get_all_patterns_short()?;
        for p in &short_patterns {
            let hours = (now - p.last_used).num_hours().max(0) as f64;
            if hours < 1.0 {
                continue;
            }
            let multiplier = self.decay_multiplier_from_cache(p.embedding_id, &cluster_sizes);
            let decayed =
                (p.confidence - (self.config.decay_rate_per_hour * hours * multiplier)).max(0.0);
            if decayed < 0.1 {
                self.db.delete_pattern_short(&p.id)?;
                self.db.delete_vectors_for_source("pattern_short", &p.id)?;
            } else if (decayed - p.confidence).abs() > 0.001 {
                self.db.update_pattern_short_confidence(&p.id, decayed)?;
            }
        }

        // Long-term patterns: slower decay (0.1%/hr) * cluster multiplier
        let long_patterns = self.db.get_all_patterns_long()?;
        for p in &long_patterns {
            let hours = (now - p.last_used).num_hours().max(0) as f64;
            if hours < 1.0 {
                continue;
            }
            let multiplier = self.decay_multiplier_from_cache(p.embedding_id, &cluster_sizes);
            let decay_rate = 0.001; // 0.1% per hour for long-term
            let decayed = (p.confidence - (decay_rate * hours * multiplier)).max(0.05);
            if (decayed - p.confidence).abs() > 0.001 {
                self.db.update_pattern_long_confidence(&p.id, decayed)?;
            }
        }

        Ok(())
    }

    /// Expire short-term patterns past their TTL with low confidence.
    /// Uses a batch SQL DELETE to avoid per-row overhead.
    pub(super) fn expire_short_term(&self) -> Result<()> {
        let threshold = Utc::now() - chrono::Duration::hours(self.config.short_term_ttl_hours as i64);
        let threshold_str = threshold.to_rfc3339();
        let min_conf = self.config.promotion_min_confidence;

        // Clean up associated vectors first (needs the IDs)
        self.db.batch_delete_expired_short_vectors(&threshold_str, min_conf)?;

        // Then batch-delete the patterns themselves
        self.db.batch_delete_expired_short_patterns(&threshold_str, min_conf)?;

        Ok(())
    }

    /// Re-embed all stored vectors if the embedding algorithm version has changed.
    pub(super) fn migrate_embeddings(&self) -> Result<()> {
        use crate::embedding::EMBEDDING_VERSION;

        let current = EMBEDDING_VERSION.to_string();
        let stored = self.db.get_meta("embedding_version")?;

        if stored.as_deref() == Some(current.as_str()) {
            return Ok(());
        }

        // Re-embed short-term patterns
        for p in &self.db.get_all_patterns_short()? {
            let vec = self.embedding.embed(&p.content);
            self.db.delete_vectors_for_source("pattern_short", &p.id)?;
            self.db.store_vector("pattern_short", &p.id, &vec)?;
        }

        // Re-embed long-term patterns
        for p in &self.db.get_all_patterns_long()? {
            let vec = self.embedding.embed(&p.content);
            self.db.delete_vectors_for_source("pattern_long", &p.id)?;
            self.db.store_vector("pattern_long", &p.id, &vec)?;
        }

        // Re-embed routing vectors
        let routing_vecs = self.db.get_vectors_for_source("routing")?;
        for (_, source_id, _) in &routing_vecs {
            // source_id is "task_pattern::agent_name" — extract the task pattern
            if let Some((task_pattern, _)) = source_id.split_once("::") {
                let vec = self.embedding.embed(task_pattern);
                self.db.delete_vectors_for_source("routing", source_id)?;
                self.db.store_vector("routing", source_id, &vec)?;
            }
        }

        self.db.set_meta("embedding_version", &current)?;
        Ok(())
    }

    /// Enforce long_term_max by pruning dormant then lowest-confidence patterns.
    pub(super) fn enforce_long_term_max(&self) -> Result<()> {
        let count = self.db.count_patterns_long()? as usize;
        if count <= self.config.long_term_max {
            return Ok(());
        }
        let excess = count - self.config.long_term_max;

        // First pass: remove dormant patterns (confidence at 0.05 floor)
        let deleted = self.db.delete_dormant_long_patterns(excess)? as usize;

        // Second pass: if still over limit, remove lowest-confidence
        if deleted < excess {
            let remaining = excess - deleted;
            self.db.delete_lowest_confidence_long(remaining)?;
        }

        // Clean up orphaned vectors
        self.db.cleanup_orphaned_long_vectors()?;
        Ok(())
    }

    /// Re-cluster if never run, if outlier count exceeds threshold, or if epsilon changed.
    pub(super) fn maybe_recluster(&self) -> Result<()> {
        let last_run = self.db.get_meta("last_cluster_run")?;
        let outlier_count = self
            .db
            .get_meta("outlier_count_since_cluster")?
            .unwrap_or_else(|| "0".to_string())
            .parse::<usize>()
            .unwrap_or(0);

        // Detect epsilon change: recluster when stored epsilon doesn't match config
        let stored_epsilon = self
            .db
            .get_meta("clustering_epsilon")?
            .and_then(|s| s.parse::<f64>().ok());
        let epsilon_changed = stored_epsilon
            .map(|e| (e - self.config.clustering_epsilon).abs() > 0.001)
            .unwrap_or(last_run.is_some()); // only force on first config write if clusters exist

        let should_recluster = last_run.is_none()
            || outlier_count >= self.config.outlier_recluster_threshold
            || epsilon_changed;

        if should_recluster {
            let mgr = crate::clustering::ClusterManager::new(self.db, self.config);
            mgr.recluster()?;
            self.db.set_meta(
                "clustering_epsilon",
                &self.config.clustering_epsilon.to_string(),
            )?;
        }
        Ok(())
    }

    /// Prune the oldest short-term patterns.
    pub(super) fn prune_oldest_short(&self, count: usize) -> Result<()> {
        let patterns = self.db.get_all_patterns_short()?;
        // Patterns are returned sorted by last_used DESC, so take from the end
        let to_prune = patterns.iter().rev().take(count);
        for p in to_prune {
            self.db.delete_pattern_short(&p.id)?;
            self.db.delete_vectors_for_source("pattern_short", &p.id)?;
        }
        Ok(())
    }
}
