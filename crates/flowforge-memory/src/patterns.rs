use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use chrono::Utc;
use uuid::Uuid;

use flowforge_core::config::PatternsConfig;
use flowforge_core::{LongTermPattern, PatternMatch, PatternTier, Result, ShortTermPattern};

use crate::db::MemoryDb;
use crate::embedding::{cosine_similarity, default_embedder, Embedder};
use crate::hnsw::HnswIndex;

/// Cached HNSW index with the vector count it was built from.
struct CachedIndex {
    index: HnswIndex,
    id_to_source: HashMap<i64, (String, PatternTier)>,
    built_from_count: usize,
}

/// Manages pattern learning lifecycle: store, promote, consolidate, search.
pub struct PatternStore<'a> {
    db: &'a MemoryDb,
    config: &'a PatternsConfig,
    embedding: Box<dyn Embedder>,
    /// Lazily-built HNSW index cached for the lifetime of this PatternStore.
    hnsw_cache: RefCell<Option<CachedIndex>>,
}

impl<'a> PatternStore<'a> {
    pub fn new(db: &'a MemoryDb, config: &'a PatternsConfig) -> Self {
        Self {
            db,
            config,
            embedding: default_embedder(config),
            hnsw_cache: RefCell::new(None),
        }
    }

    /// Store a new short-term pattern. Returns the pattern ID.
    pub fn store_short_term(&self, content: &str, category: &str) -> Result<String> {
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();

        // Generate and store embedding
        let vector = self.embedding.embed(content);
        let embedding_id = self.db.store_vector("pattern_short", &id, &vector)?;

        let pattern = ShortTermPattern {
            id: id.clone(),
            content: content.to_string(),
            category: category.to_string(),
            confidence: 0.5,
            usage_count: 0,
            created_at: now,
            last_used: now,
            embedding_id: Some(embedding_id),
        };

        self.db.store_pattern_short(&pattern)?;

        // Enforce max count: remove oldest if over limit
        let count = self.db.count_patterns_short()? as usize;
        if count > self.config.short_term_max {
            self.prune_oldest_short(count - self.config.short_term_max)?;
        }

        Ok(id)
    }

    /// Promote eligible short-term patterns to long-term.
    /// Criteria: usage >= min_usage AND confidence >= min_confidence
    pub fn promote_eligible(&self) -> Result<u32> {
        let patterns = self.db.get_all_patterns_short()?;
        let mut promoted = 0;

        for p in &patterns {
            if p.usage_count >= self.config.promotion_min_usage
                && p.confidence >= self.config.promotion_min_confidence
            {
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

                self.db.store_pattern_long(&long_pattern)?;
                // Update vector source_type from pattern_short → pattern_long
                if let Some(eid) = p.embedding_id {
                    self.db.update_vector_source_type(eid, "pattern_long")?;
                }
                self.db.delete_pattern_short(&p.id)?;
                promoted += 1;
            }
        }

        // Invalidate HNSW cache since tier mappings changed
        if promoted > 0 {
            *self.hnsw_cache.borrow_mut() = None;
        }

        Ok(promoted)
    }

    /// Run consolidation: promotion, decay, expiration, deduplication, and migration.
    pub fn consolidate(&self) -> Result<()> {
        // 1. Promote eligible patterns
        self.promote_eligible()?;

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

    /// Search all patterns (both tiers) using HNSW when >50 total, else brute-force.
    /// Boosts results from the same cluster as the query.
    pub fn search_all_patterns(&self, query: &str, k: usize) -> Result<Vec<PatternMatch>> {
        let query_vec = self.embedding.embed(query);
        let total =
            self.db.count_patterns_short()? as usize + self.db.count_patterns_long()? as usize;

        let mut results = if total > 50 {
            self.search_with_hnsw(&query_vec, k)?
        } else {
            self.search_brute_force(&query_vec, k)?
        };

        // Boost results from the same cluster as the query
        let cluster_mgr = crate::clustering::ClusterManager::new(self.db, self.config);
        if let Ok(Some(query_cluster)) = cluster_mgr.find_cluster(&query_vec) {
            // Look up embedding IDs for each result to check cluster membership
            let all_short = self.db.get_vectors_for_source("pattern_short")?;
            let all_long = self.db.get_vectors_for_source("pattern_long")?;
            let mut source_to_eid: std::collections::HashMap<String, i64> =
                std::collections::HashMap::new();
            for (db_id, source_id, _) in all_short.iter().chain(all_long.iter()) {
                source_to_eid.insert(source_id.clone(), *db_id);
            }

            for result in &mut results {
                if let Some(&eid) = source_to_eid.get(&result.id) {
                    if let Ok(Some(cid)) = self.db.get_vector_cluster_id(eid) {
                        if cid == query_cluster.cluster_id {
                            result.similarity *= 1.1; // 10% boost for same-cluster
                        }
                    }
                }
            }
            results.sort_by(|a, b| {
                b.similarity
                    .partial_cmp(&a.similarity)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        Ok(results)
    }

    /// Fetch a pattern by ID and tier, returning a PatternMatch if found.
    fn fetch_pattern_match(
        &self,
        id: &str,
        tier: PatternTier,
        similarity: f32,
    ) -> Option<PatternMatch> {
        match tier {
            PatternTier::Short => {
                if let Ok(Some(p)) = self.db.get_pattern_short(id) {
                    Some(PatternMatch {
                        id: p.id,
                        content: p.content,
                        category: p.category,
                        confidence: p.confidence,
                        usage_count: p.usage_count,
                        tier: PatternTier::Short,
                        similarity,
                    })
                } else {
                    None
                }
            }
            PatternTier::Long => {
                if let Ok(Some(p)) = self.db.get_pattern_long(id) {
                    Some(PatternMatch {
                        id: p.id,
                        content: p.content,
                        category: p.category,
                        confidence: p.confidence,
                        usage_count: p.usage_count,
                        tier: PatternTier::Long,
                        similarity,
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Ensure the HNSW cache is built from BOTH tiers (or rebuilt if vector count changed).
    fn ensure_hnsw_cache(&self) -> Result<()> {
        let short_vecs = self.db.get_vectors_for_source("pattern_short")?;
        let long_vecs = self.db.get_vectors_for_source("pattern_long")?;
        let current_count = short_vecs.len() + long_vecs.len();

        let needs_rebuild = {
            let cache = self.hnsw_cache.borrow();
            match &*cache {
                Some(c) => c.built_from_count != current_count,
                None => true,
            }
        };

        if needs_rebuild {
            if current_count == 0 {
                *self.hnsw_cache.borrow_mut() = None;
                return Ok(());
            }

            let mut id_to_source: HashMap<i64, (String, PatternTier)> = HashMap::new();
            let mut points: Vec<(i64, Vec<f32>)> = Vec::new();

            for (db_id, source_id, vector) in &short_vecs {
                id_to_source.insert(*db_id, (source_id.clone(), PatternTier::Short));
                points.push((*db_id, vector.clone()));
            }
            for (db_id, source_id, vector) in &long_vecs {
                id_to_source.insert(*db_id, (source_id.clone(), PatternTier::Long));
                points.push((*db_id, vector.clone()));
            }

            let mut index = HnswIndex::new();
            index.build(&points);

            *self.hnsw_cache.borrow_mut() = Some(CachedIndex {
                index,
                id_to_source,
                built_from_count: current_count,
            });
        }

        Ok(())
    }

    /// Search using cached HNSW index built from stored vectors (both tiers).
    fn search_with_hnsw(&self, query_vec: &[f32], k: usize) -> Result<Vec<PatternMatch>> {
        self.ensure_hnsw_cache()?;

        let cache = self.hnsw_cache.borrow();
        let cached = match &*cache {
            Some(c) => c,
            None => return Ok(Vec::new()),
        };

        let results = cached.index.search(query_vec, k);

        let mut scored = Vec::new();
        for (db_id, distance) in results {
            if let Some((pattern_id, tier)) = cached.id_to_source.get(&db_id) {
                let similarity = 1.0 - distance;
                if let Some(m) = self.fetch_pattern_match(pattern_id, *tier, similarity) {
                    scored.push(m);
                }
            }
        }

        Ok(scored)
    }

    /// Brute-force search using stored vectors across both tiers.
    fn search_brute_force(&self, query_vec: &[f32], k: usize) -> Result<Vec<PatternMatch>> {
        let short_vecs = self.db.get_vectors_for_source("pattern_short")?;
        let long_vecs = self.db.get_vectors_for_source("pattern_long")?;

        let mut scored: Vec<(String, PatternTier, f32)> = Vec::new();

        for (_, source_id, vec) in &short_vecs {
            let sim = cosine_similarity(query_vec, vec);
            scored.push((source_id.clone(), PatternTier::Short, sim));
        }
        for (_, source_id, vec) in &long_vecs {
            let sim = cosine_similarity(query_vec, vec);
            scored.push((source_id.clone(), PatternTier::Long, sim));
        }

        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);

        let mut results = Vec::new();
        for (id, tier, similarity) in scored {
            if let Some(m) = self.fetch_pattern_match(&id, tier, similarity) {
                results.push(m);
            }
        }

        Ok(results)
    }

    /// Record usage of a pattern (increments count and confidence).
    pub fn record_usage(&self, pattern_id: &str) -> Result<()> {
        self.db.update_pattern_short_usage(pattern_id)
    }

    /// Record feedback on a pattern (success/failure). (A4)
    /// Looks up in both short-term and long-term stores.
    pub fn record_feedback(&self, pattern_id: &str, success: bool) -> Result<()> {
        // Try long-term first (feedback is most meaningful for promoted patterns)
        if self.db.get_pattern_long(pattern_id)?.is_some() {
            self.db.update_pattern_long_feedback(pattern_id, success)?;
            return Ok(());
        }

        // Fall back to short-term: adjust confidence
        if let Some(p) = self.db.get_pattern_short(pattern_id)? {
            let new_confidence = if success {
                (p.confidence + 0.05).min(1.0)
            } else {
                (p.confidence - 0.08).max(0.0)
            };
            self.db
                .update_pattern_short_confidence(pattern_id, new_confidence)?;
            return Ok(());
        }

        Ok(()) // Pattern not found, silently ignore
    }

    /// Get cluster-aware decay multiplier for a pattern.
    /// Patterns in large clusters decay slower; outliers decay faster.
    fn decay_multiplier(&self, embedding_id: Option<i64>) -> f64 {
        if let Some(eid) = embedding_id {
            if let Ok(Some(cluster_id)) = self.db.get_vector_cluster_id(eid) {
                if let Ok(Some(cluster)) = self.db.get_cluster(cluster_id) {
                    if cluster.member_count > 10 {
                        return self.config.cluster_decay_active_factor; // Active cluster, slow decay
                    }
                }
                return 1.0; // Small cluster, normal decay
            }
        }
        self.config.cluster_decay_isolated_factor // Outlier, fast decay
    }

    /// Apply confidence decay based on time since last use. (A6)
    /// Uses cluster-aware decay multipliers.
    fn apply_decay(&self) -> Result<()> {
        let now = Utc::now();

        // Short-term patterns: decay at configured rate * cluster multiplier
        let short_patterns = self.db.get_all_patterns_short()?;
        for p in &short_patterns {
            let hours = (now - p.last_used).num_hours().max(0) as f64;
            if hours < 1.0 {
                continue;
            }
            let multiplier = self.decay_multiplier(p.embedding_id);
            let decayed = p.confidence - (self.config.decay_rate_per_hour * hours * multiplier);
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
            let multiplier = self.decay_multiplier(p.embedding_id);
            let decay_rate = 0.001; // 0.1% per hour for long-term
            let decayed = (p.confidence - (decay_rate * hours * multiplier)).max(0.05);
            if (decayed - p.confidence).abs() > 0.001 {
                self.db.update_pattern_long_confidence(&p.id, decayed)?;
            }
        }

        Ok(())
    }

    fn expire_short_term(&self) -> Result<()> {
        let patterns = self.db.get_all_patterns_short()?;
        let now = Utc::now();
        let ttl = chrono::Duration::hours(self.config.short_term_ttl_hours as i64);

        for p in &patterns {
            if now - p.created_at > ttl && p.confidence < self.config.promotion_min_confidence {
                self.db.delete_pattern_short(&p.id)?;
                self.db.delete_vectors_for_source("pattern_short", &p.id)?;
            }
        }

        Ok(())
    }

    /// Get the dedup threshold, using per-cluster p95 when both vectors are in the same cluster.
    fn get_dedup_threshold(&self, embedding_id_a: Option<i64>, embedding_id_b: Option<i64>) -> f32 {
        if let (Some(eid_a), Some(eid_b)) = (embedding_id_a, embedding_id_b) {
            if let (Ok(cluster_a), Ok(cluster_b)) = (
                self.db.get_vector_cluster_id(eid_a),
                self.db.get_vector_cluster_id(eid_b),
            ) {
                if let (Some(ca), Some(cb)) = (cluster_a, cluster_b) {
                    if ca == cb {
                        // Same cluster — use cluster's p95 as threshold (convert distance to similarity)
                        if let Ok(Some(cluster)) = self.db.get_cluster(ca) {
                            return 1.0 - cluster.p95_distance as f32;
                        }
                    }
                }
            }
        }
        self.config.dedup_similarity_threshold as f32
    }

    /// Deduplicate using stored vectors instead of re-embedding. (A11)
    /// Uses per-cluster p95 thresholds when available.
    fn deduplicate(&self) -> Result<()> {
        let patterns = self.db.get_all_patterns_short()?;
        if patterns.len() < 2 {
            return Ok(());
        }

        // Load stored vectors indexed by source_id, also track embedding IDs
        let vectors = self.db.get_vectors_for_source("pattern_short")?;
        let vec_map: HashMap<String, (Vec<f32>, i64)> = vectors
            .into_iter()
            .map(|(db_id, source_id, vec)| (source_id, (vec, db_id)))
            .collect();

        let mut to_remove: HashSet<usize> = HashSet::new();
        let fallback_threshold = self.config.dedup_similarity_threshold as f32;

        for i in 0..patterns.len() {
            if to_remove.contains(&i) {
                continue;
            }
            let (vec_i, eid_i) = match vec_map.get(&patterns[i].id) {
                Some(v) => (&v.0, Some(v.1)),
                None => continue,
            };
            for j in (i + 1)..patterns.len() {
                if to_remove.contains(&j) {
                    continue;
                }
                let (vec_j, eid_j) = match vec_map.get(&patterns[j].id) {
                    Some(v) => (&v.0, Some(v.1)),
                    None => continue,
                };
                let sim = cosine_similarity(vec_i, vec_j);
                let threshold = self.get_dedup_threshold(eid_i, eid_j);
                // Use at least the fallback threshold to prevent over-aggressive dedup
                let threshold = threshold.max(fallback_threshold);
                if sim > threshold {
                    if patterns[j].confidence < patterns[i].confidence
                        || (patterns[j].confidence == patterns[i].confidence
                            && patterns[j].usage_count < patterns[i].usage_count)
                    {
                        to_remove.insert(j);
                    } else {
                        to_remove.insert(i);
                        break;
                    }
                }
            }
        }

        for idx in to_remove {
            let p = &patterns[idx];
            self.db.delete_pattern_short(&p.id)?;
            self.db.delete_vectors_for_source("pattern_short", &p.id)?;
        }

        Ok(())
    }

    /// Re-embed all stored vectors if the embedding algorithm version has changed.
    fn migrate_embeddings(&self) -> Result<()> {
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
    fn enforce_long_term_max(&self) -> Result<()> {
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

    /// Re-cluster if never run or if outlier count exceeds threshold.
    fn maybe_recluster(&self) -> Result<()> {
        let last_run = self.db.get_meta("last_cluster_run")?;
        let outlier_count = self
            .db
            .get_meta("outlier_count_since_cluster")?
            .unwrap_or_else(|| "0".to_string())
            .parse::<usize>()
            .unwrap_or(0);

        let should_recluster =
            last_run.is_none() || outlier_count >= self.config.outlier_recluster_threshold;

        if should_recluster {
            let mgr = crate::clustering::ClusterManager::new(self.db, self.config);
            mgr.recluster()?;
        }
        Ok(())
    }

    fn prune_oldest_short(&self, count: usize) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn setup_db() -> MemoryDb {
        let path = PathBuf::from(format!(
            "/tmp/flowforge-test-patterns-{}.db",
            Uuid::new_v4()
        ));
        MemoryDb::open(&path).unwrap()
    }

    #[test]
    fn test_store_and_search() {
        let db = setup_db();
        let config = PatternsConfig::default();
        let store = PatternStore::new(&db, &config);

        store
            .store_short_term("use cargo build for compilation", "rust")
            .unwrap();
        store
            .store_short_term("python uses pip for packages", "python")
            .unwrap();

        let results = store.search_all_patterns("cargo rust", 2).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].content.contains("cargo"));
        assert_eq!(results[0].tier, PatternTier::Short);
    }

    #[test]
    fn test_promote_eligible() {
        let db = setup_db();
        let config = PatternsConfig {
            promotion_min_usage: 2,
            promotion_min_confidence: 0.5,
            ..PatternsConfig::default()
        };
        let store = PatternStore::new(&db, &config);

        let id = store.store_short_term("test pattern", "test").unwrap();
        // Bump usage to meet promotion threshold
        store.record_usage(&id).unwrap();
        store.record_usage(&id).unwrap();

        let promoted = store.promote_eligible().unwrap();
        assert_eq!(promoted, 1);

        // Should be gone from short-term
        assert_eq!(db.count_patterns_short().unwrap(), 0);
        // Should be in long-term
        assert_eq!(db.count_patterns_long().unwrap(), 1);
    }

    #[test]
    fn test_record_feedback_long_term() {
        let db = setup_db();
        let config = PatternsConfig {
            promotion_min_usage: 1,
            promotion_min_confidence: 0.4,
            ..PatternsConfig::default()
        };
        let store = PatternStore::new(&db, &config);

        let id = store.store_short_term("feedback pattern", "test").unwrap();
        store.record_usage(&id).unwrap();
        store.promote_eligible().unwrap();

        // Now give positive feedback on the long-term pattern
        store.record_feedback(&id, true).unwrap();
        let p = db.get_pattern_long(&id).unwrap().unwrap();
        assert_eq!(p.success_count, 1);
        assert!(p.confidence > 0.5);

        // Negative feedback
        store.record_feedback(&id, false).unwrap();
        let p = db.get_pattern_long(&id).unwrap().unwrap();
        assert_eq!(p.failure_count, 1);
    }

    #[test]
    fn test_record_feedback_short_term() {
        let db = setup_db();
        let config = PatternsConfig::default();
        let store = PatternStore::new(&db, &config);

        let id = store
            .store_short_term("short feedback pattern", "test")
            .unwrap();
        let original = db.get_pattern_short(&id).unwrap().unwrap();
        let original_conf = original.confidence;

        store.record_feedback(&id, true).unwrap();
        let updated = db.get_pattern_short(&id).unwrap().unwrap();
        assert!(updated.confidence > original_conf);

        store.record_feedback(&id, false).unwrap();
        let updated2 = db.get_pattern_short(&id).unwrap().unwrap();
        assert!(updated2.confidence < updated.confidence);
    }

    #[test]
    fn test_consolidate_runs_without_error() {
        let db = setup_db();
        let config = PatternsConfig::default();
        let store = PatternStore::new(&db, &config);

        store.store_short_term("pattern one", "test").unwrap();
        store.store_short_term("pattern two", "test").unwrap();

        // Should run all phases without error
        store.consolidate().unwrap();
    }

    #[test]
    fn test_search_finds_promoted_patterns() {
        let db = setup_db();
        let config = PatternsConfig {
            promotion_min_usage: 1,
            promotion_min_confidence: 0.4,
            ..PatternsConfig::default()
        };
        let store = PatternStore::new(&db, &config);

        let id = store
            .store_short_term("deploy kubernetes service", "devops")
            .unwrap();
        store.record_usage(&id).unwrap();

        // Promote to long-term
        let promoted = store.promote_eligible().unwrap();
        assert_eq!(promoted, 1);
        assert_eq!(db.count_patterns_short().unwrap(), 0);
        assert_eq!(db.count_patterns_long().unwrap(), 1);

        // Search should still find it, now as Long tier
        let results = store.search_all_patterns("deploy kubernetes", 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].tier, PatternTier::Long);
        assert!(results[0].content.contains("kubernetes"));
    }

    #[test]
    fn test_search_all_combines_tiers() {
        let db = setup_db();
        let config = PatternsConfig {
            promotion_min_usage: 1,
            promotion_min_confidence: 0.4,
            ..PatternsConfig::default()
        };
        let store = PatternStore::new(&db, &config);

        // Store 3 short-term patterns
        store
            .store_short_term("fix rust compilation error", "rust")
            .unwrap();
        store
            .store_short_term("debug rust test failure", "rust")
            .unwrap();
        let id3 = store
            .store_short_term("optimize rust build time", "rust")
            .unwrap();

        // Promote one to long-term
        store.record_usage(&id3).unwrap();
        store.promote_eligible().unwrap();

        assert_eq!(db.count_patterns_short().unwrap(), 2);
        assert_eq!(db.count_patterns_long().unwrap(), 1);

        // Search should return results from both tiers
        let results = store.search_all_patterns("rust", 5).unwrap();
        assert_eq!(results.len(), 3);

        let has_short = results.iter().any(|m| m.tier == PatternTier::Short);
        let has_long = results.iter().any(|m| m.tier == PatternTier::Long);
        assert!(has_short, "Expected short-term results");
        assert!(has_long, "Expected long-term results");
    }

    #[test]
    fn test_enforce_long_term_max() {
        let db = setup_db();
        let config = PatternsConfig {
            promotion_min_usage: 1,
            promotion_min_confidence: 0.4,
            long_term_max: 3,
            ..PatternsConfig::default()
        };
        let store = PatternStore::new(&db, &config);

        // Store and promote 6 patterns (exceeds max of 3)
        for i in 0..6 {
            let id = store
                .store_short_term(&format!("pattern number {i}"), "test")
                .unwrap();
            store.record_usage(&id).unwrap();
        }
        store.promote_eligible().unwrap();
        assert_eq!(db.count_patterns_long().unwrap(), 6);

        // Enforce max — should prune down to 3
        store.enforce_long_term_max().unwrap();
        assert!(db.count_patterns_long().unwrap() <= 3);
    }

    #[test]
    fn test_migrate_embeddings() {
        let db = setup_db();
        let config = PatternsConfig::default();
        let store = PatternStore::new(&db, &config);

        // Store some patterns
        store
            .store_short_term("test migration pattern", "test")
            .unwrap();
        store
            .store_short_term("another test pattern", "test")
            .unwrap();

        // Set meta to current version — migration should be a no-op
        use crate::embedding::EMBEDDING_VERSION;
        db.set_meta("embedding_version", &EMBEDDING_VERSION.to_string())
            .unwrap();

        // Should succeed without re-embedding (version matches)
        store.migrate_embeddings().unwrap();

        // Force stale version
        db.set_meta("embedding_version", "0").unwrap();

        // Should re-embed all vectors
        store.migrate_embeddings().unwrap();

        // Version should now be updated
        let version = db.get_meta("embedding_version").unwrap();
        assert_eq!(version, Some(EMBEDDING_VERSION.to_string()));

        // Vectors should still exist and search should work
        let results = store.search_all_patterns("test migration", 5).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_hnsw_cache_invalidated_after_promotion() {
        let db = setup_db();
        let config = PatternsConfig {
            promotion_min_usage: 1,
            promotion_min_confidence: 0.4,
            ..PatternsConfig::default()
        };
        let store = PatternStore::new(&db, &config);

        // Store enough patterns to trigger HNSW (>50)
        for i in 0..55 {
            store
                .store_short_term(&format!("hnsw test pattern {i}"), "test")
                .unwrap();
        }

        // First search builds the HNSW cache
        let results1 = store.search_all_patterns("hnsw test pattern 0", 5).unwrap();
        assert!(!results1.is_empty());

        // Promote pattern 0 — should invalidate cache
        let patterns = db.get_all_patterns_short().unwrap();
        let p0 = patterns
            .iter()
            .find(|p| p.content == "hnsw test pattern 0")
            .unwrap();
        store.record_usage(&p0.id).unwrap();
        store.promote_eligible().unwrap();

        // Search again — cache should rebuild, promoted pattern should appear as Long
        let results2 = store.search_all_patterns("hnsw test pattern 0", 5).unwrap();
        assert!(!results2.is_empty());
        assert_eq!(
            results2[0].tier,
            PatternTier::Long,
            "Promoted pattern should be Long tier after cache rebuild"
        );
    }

    #[test]
    fn test_dedup_catches_similar_patterns() {
        let db = setup_db();
        let config = PatternsConfig {
            dedup_similarity_threshold: 0.88,
            ..PatternsConfig::default()
        };
        let store = PatternStore::new(&db, &config);

        // Store two very similar patterns (same words, slight reorder)
        store
            .store_short_term("use cargo build for rust compilation", "rust")
            .unwrap();
        store
            .store_short_term("use cargo build for rust compilation tasks", "rust")
            .unwrap();

        assert_eq!(db.count_patterns_short().unwrap(), 2);

        // Run dedup
        store.deduplicate().unwrap();

        // Should have removed one (they share almost all n-grams)
        let remaining = db.count_patterns_short().unwrap();
        assert!(
            remaining <= 1,
            "Expected dedup to remove near-duplicate, got {remaining} remaining"
        );
    }
}
