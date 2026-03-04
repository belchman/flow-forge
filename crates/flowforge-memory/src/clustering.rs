use chrono::Utc;
use linfa::prelude::*;
use linfa_clustering::Dbscan;
use ndarray::Array2;

use flowforge_core::config::PatternsConfig;
use flowforge_core::{PatternCluster, Result};

use crate::db::MemoryDb;
use crate::embedding::cosine_similarity;

/// Result of a clustering run.
pub struct ClusterResult {
    pub cluster_count: usize,
    pub outlier_count: usize,
}

/// Result of auto-tuning DBSCAN parameters via k-distance elbow detection.
pub struct TuneResult {
    pub suggested_epsilon: f64,
    pub suggested_min_points: usize,
    pub vector_count: usize,
    pub k_distances: Vec<f64>,
    pub elbow_index: usize,
}

/// Result of finding a cluster for a query vector.
pub struct ClusterMatch {
    pub cluster_id: i64,
    pub distance: f32,
    pub within_p95: bool,
}

pub struct ClusterManager<'a> {
    db: &'a MemoryDb,
    config: &'a PatternsConfig,
}

impl<'a> ClusterManager<'a> {
    pub fn new(db: &'a MemoryDb, config: &'a PatternsConfig) -> Self {
        Self { db, config }
    }

    /// Run DBSCAN over all pattern vectors and update cluster assignments.
    /// Uses L2 distance on already-normalized vectors (equivalent to cosine distance).
    pub fn recluster(&self) -> Result<ClusterResult> {
        let vectors = self.db.get_all_pattern_vectors()?;
        if vectors.len() < self.config.clustering_min_points {
            return Ok(ClusterResult {
                cluster_count: 0,
                outlier_count: vectors.len(),
            });
        }

        let n = vectors.len();
        let dim = vectors.first().map(|(_, v)| v.len()).unwrap_or(0);
        if dim == 0 {
            return Ok(ClusterResult {
                cluster_count: 0,
                outlier_count: 0,
            });
        }

        // Build ndarray matrix from vectors
        let mut data = Array2::<f64>::zeros((n, dim));
        for (i, (_, vec)) in vectors.iter().enumerate() {
            for (j, &val) in vec.iter().enumerate() {
                if j < dim {
                    data[[i, j]] = val as f64;
                }
            }
        }

        // Run DBSCAN with L2 distance (on normalized vectors, L2 distance ~ sqrt(2 * cosine_distance))
        // Adjust epsilon: for cosine_distance=0.3, L2 ~ sqrt(2*0.3) ~ 0.77
        let l2_epsilon = (2.0 * self.config.clustering_epsilon).sqrt();

        let params = Dbscan::params(self.config.clustering_min_points).tolerance(l2_epsilon);
        let labels: ndarray::Array1<Option<usize>> = params.check_unwrap().transform(&data);

        // Clear old clusters
        self.db.delete_all_clusters()?;

        // Group vectors by cluster label
        let mut cluster_groups: std::collections::HashMap<usize, Vec<usize>> =
            std::collections::HashMap::new();
        let mut outlier_count = 0;

        for (i, label) in labels.iter().enumerate() {
            if let Some(cluster_idx) = label {
                cluster_groups.entry(*cluster_idx).or_default().push(i);
            } else {
                self.db.set_vector_cluster_id(vectors[i].0, None)?;
                outlier_count += 1;
            }
        }

        let now = Utc::now();
        let mut cluster_count = 0;

        for member_indices in cluster_groups.values() {
            if member_indices.is_empty() {
                continue;
            }

            // Compute centroid (mean of members)
            let mut centroid = vec![0.0f32; dim];
            for &i in member_indices {
                for (j, &val) in vectors[i].1.iter().enumerate() {
                    if j < dim {
                        centroid[j] += val;
                    }
                }
            }
            let count = member_indices.len() as f32;
            for v in &mut centroid {
                *v /= count;
            }

            // Compute cosine distances from centroid
            let mut distances: Vec<f32> = member_indices
                .iter()
                .map(|&i| 1.0 - cosine_similarity(&centroid, &vectors[i].1))
                .collect();
            distances.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            // p95 distance
            let p95_idx = (distances.len() as f64 * 0.95).ceil() as usize;
            let p95_distance = distances
                .get(p95_idx.saturating_sub(1).min(distances.len() - 1))
                .copied()
                .unwrap_or(0.0) as f64;

            let cluster = PatternCluster {
                id: 0,
                centroid: centroid.clone(),
                member_count: member_indices.len() as i64,
                p95_distance,
                avg_confidence: 0.0,
                created_at: now,
                last_recomputed: now,
            };

            let cluster_id = self.db.store_cluster(&cluster)?;

            for &i in member_indices {
                self.db
                    .set_vector_cluster_id(vectors[i].0, Some(cluster_id))?;
            }

            cluster_count += 1;
        }

        self.db.set_meta("outlier_count_since_cluster", "0")?;
        self.db.set_meta("last_cluster_run", &now.to_rfc3339())?;

        Ok(ClusterResult {
            cluster_count,
            outlier_count,
        })
    }

    /// Check if a vector matches any existing cluster (within p95).
    pub fn find_cluster(&self, vector: &[f32]) -> Result<Option<ClusterMatch>> {
        let clusters = self.db.get_all_clusters()?;
        if clusters.is_empty() {
            return Ok(None);
        }

        let mut best: Option<ClusterMatch> = None;

        for c in &clusters {
            let distance = 1.0 - cosine_similarity(vector, &c.centroid);
            let within_p95 = (distance as f64) <= c.p95_distance;

            match &best {
                Some(current) if distance < current.distance => {
                    best = Some(ClusterMatch {
                        cluster_id: c.id,
                        distance,
                        within_p95,
                    });
                }
                None => {
                    best = Some(ClusterMatch {
                        cluster_id: c.id,
                        distance,
                        within_p95,
                    });
                }
                _ => {}
            }
        }

        Ok(best)
    }

    /// Auto-tune DBSCAN parameters via k-distance elbow detection.
    /// Computes k-th nearest neighbor distance for each vector (k = min_points),
    /// sorts descending, and finds the elbow via max second derivative.
    pub fn tune(&self) -> Result<TuneResult> {
        let vectors = self.db.get_all_pattern_vectors()?;
        let n = vectors.len();
        let k = self.config.clustering_min_points;

        if n < k + 1 {
            return Ok(TuneResult {
                suggested_epsilon: self.config.clustering_epsilon,
                suggested_min_points: k,
                vector_count: n,
                k_distances: Vec::new(),
                elbow_index: 0,
            });
        }

        // Compute pairwise cosine distances and find k-th nearest neighbor distance
        let mut k_distances: Vec<f64> = Vec::with_capacity(n);
        for i in 0..n {
            let mut dists: Vec<f64> = Vec::with_capacity(n - 1);
            for j in 0..n {
                if i != j {
                    let sim = cosine_similarity(&vectors[i].1, &vectors[j].1);
                    dists.push((1.0 - sim) as f64);
                }
            }
            dists.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            // k-th nearest (0-indexed, so index k-1)
            if let Some(&d) = dists.get(k - 1) {
                k_distances.push(d);
            }
        }

        // Sort descending for elbow detection
        k_distances.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

        let elbow_index = find_elbow(&k_distances);
        let suggested_epsilon = k_distances
            .get(elbow_index)
            .copied()
            .unwrap_or(self.config.clustering_epsilon);

        Ok(TuneResult {
            suggested_epsilon,
            suggested_min_points: k,
            vector_count: n,
            k_distances,
            elbow_index,
        })
    }

    /// Record an outlier. Returns true if outlier count exceeds recluster threshold.
    pub fn record_outlier(&self) -> Result<bool> {
        let current = self
            .db
            .get_meta("outlier_count_since_cluster")?
            .unwrap_or_else(|| "0".to_string())
            .parse::<usize>()
            .unwrap_or(0);
        let new_count = current + 1;
        self.db
            .set_meta("outlier_count_since_cluster", &new_count.to_string())?;
        Ok(new_count >= self.config.outlier_recluster_threshold)
    }
}

/// Find the elbow point in a sorted (descending) k-distance curve.
/// Uses max second derivative (discrete approximation).
fn find_elbow(distances: &[f64]) -> usize {
    if distances.len() < 3 {
        return 0;
    }
    let mut max_second_deriv = 0.0f64;
    let mut elbow = 0;
    for i in 1..distances.len() - 1 {
        let second_deriv = (distances[i - 1] - 2.0 * distances[i] + distances[i + 1]).abs();
        if second_deriv > max_second_deriv {
            max_second_deriv = second_deriv;
            elbow = i;
        }
    }
    elbow
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MemoryDb;
    use crate::embedding::{Embedder, HashEmbedder};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn setup_db() -> MemoryDb {
        let path = PathBuf::from(format!(
            "/tmp/flowforge-test-clustering-{}.db",
            Uuid::new_v4()
        ));
        MemoryDb::open(&path).unwrap()
    }

    #[test]
    fn test_dbscan_forms_clusters() {
        let db = setup_db();
        let emb = HashEmbedder::new(128);
        let config = PatternsConfig {
            clustering_min_points: 2,
            clustering_epsilon: 0.5,
            ..Default::default()
        };

        // Group A: rust-related
        for text in &[
            "rust programming language",
            "cargo build rust",
            "rust compiler error",
            "rustc optimization",
            "rust trait impl",
        ] {
            let vec = emb.embed(text);
            db.store_vector("pattern_short", &Uuid::new_v4().to_string(), &vec)
                .unwrap();
        }

        // Group B: python-related
        for text in &[
            "python pip install",
            "python package manager",
            "python import module",
            "python virtualenv setup",
            "python flask web",
        ] {
            let vec = emb.embed(text);
            db.store_vector("pattern_short", &Uuid::new_v4().to_string(), &vec)
                .unwrap();
        }

        // Group C: cooking
        for text in &[
            "bake chocolate cake",
            "cooking recipe dinner",
            "kitchen oven temperature",
            "flour sugar butter",
            "roast chicken herbs",
        ] {
            let vec = emb.embed(text);
            db.store_vector("pattern_short", &Uuid::new_v4().to_string(), &vec)
                .unwrap();
        }

        let mgr = ClusterManager::new(&db, &config);
        let result = mgr.recluster().unwrap();

        assert!(
            result.cluster_count >= 2,
            "Expected at least 2 clusters, got {}",
            result.cluster_count
        );

        // Verify clusters stored in DB
        let clusters = db.get_all_clusters().unwrap();
        assert_eq!(clusters.len(), result.cluster_count);
    }

    #[test]
    fn test_outlier_threshold() {
        let db = setup_db();
        let config = PatternsConfig {
            outlier_recluster_threshold: 3,
            ..Default::default()
        };
        let mgr = ClusterManager::new(&db, &config);

        assert!(!mgr.record_outlier().unwrap());
        assert!(!mgr.record_outlier().unwrap());
        assert!(mgr.record_outlier().unwrap());
    }

    #[test]
    fn test_find_cluster_empty() {
        let db = setup_db();
        let config = PatternsConfig::default();
        let mgr = ClusterManager::new(&db, &config);

        let emb = HashEmbedder::new(128);
        let v = emb.embed("test");
        assert!(mgr.find_cluster(&v).unwrap().is_none());
    }

    #[test]
    fn test_recluster_too_few_points() {
        let db = setup_db();
        let emb = HashEmbedder::new(128);
        let config = PatternsConfig {
            clustering_min_points: 5,
            ..Default::default()
        };

        // Only add 2 vectors
        let vec = emb.embed("hello");
        db.store_vector("pattern_short", "p1", &vec).unwrap();
        let vec = emb.embed("world");
        db.store_vector("pattern_short", "p2", &vec).unwrap();

        let mgr = ClusterManager::new(&db, &config);
        let result = mgr.recluster().unwrap();
        assert_eq!(result.cluster_count, 0);
        assert_eq!(result.outlier_count, 2);
    }
}
