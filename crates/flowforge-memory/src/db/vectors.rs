use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use flowforge_core::Result;

use super::{blob_to_vector, parse_datetime, vector_to_blob, MemoryDb, SqliteExt, VectorEntry};

impl MemoryDb {
    pub fn store_vector(&self, source_type: &str, source_id: &str, vector: &[f32]) -> Result<i64> {
        let blob = vector_to_blob(vector);
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO hnsw_entries (source_type, source_id, vector, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![source_type, source_id, blob, now],
            )
            .sq()?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_all_vectors(&self) -> Result<Vec<VectorEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source_type, source_id, vector FROM hnsw_entries")
            .sq()?;
        let rows = stmt
            .query_map([], |row| {
                let blob: Vec<u8> = row.get(3)?;
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, blob_to_vector(&blob)))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn get_vectors_for_source(
        &self,
        source_type: &str,
    ) -> Result<Vec<(i64, String, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source_id, vector FROM hnsw_entries WHERE source_type = ?1")
            .sq()?;
        let rows = stmt
            .query_map(params![source_type], |row| {
                let blob: Vec<u8> = row.get(2)?;
                Ok((row.get(0)?, row.get(1)?, blob_to_vector(&blob)))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Count vectors of a given source type without loading them.
    pub fn count_vectors_for_source(&self, source_type: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM hnsw_entries WHERE source_type = ?1",
                params![source_type],
                |row| row.get(0),
            )
            .sq()
    }

    pub fn delete_vectors_for_source(&self, source_type: &str, source_id: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM hnsw_entries WHERE source_type = ?1 AND source_id = ?2",
                params![source_type, source_id],
            )
            .sq()?;
        Ok(())
    }

    pub fn update_vector_source_type(&self, id: i64, new_source_type: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE hnsw_entries SET source_type = ?1 WHERE id = ?2",
                params![new_source_type, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn cleanup_orphaned_long_vectors(&self) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM hnsw_entries WHERE source_type = 'pattern_long'
                 AND source_id NOT IN (SELECT id FROM patterns_long)",
                [],
            )
            .sq()?;
        Ok(())
    }

    // ── Clustering ──

    pub fn store_cluster(&self, cluster: &flowforge_core::PatternCluster) -> Result<i64> {
        let centroid_bytes = vector_to_blob(&cluster.centroid);
        let now = cluster.created_at.to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO pattern_clusters (centroid, member_count, p95_distance, avg_confidence, created_at, last_recomputed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![centroid_bytes, cluster.member_count, cluster.p95_distance, cluster.avg_confidence, now],
            )
            .sq()?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_cluster(&self, id: i64) -> Result<Option<flowforge_core::PatternCluster>> {
        self.conn
            .query_row(
                "SELECT id, centroid, member_count, p95_distance, avg_confidence, created_at, last_recomputed
                 FROM pattern_clusters WHERE id = ?1",
                params![id],
                |row| {
                    let centroid_bytes: Vec<u8> = row.get(1)?;
                    Ok(flowforge_core::PatternCluster {
                        id: row.get(0)?,
                        centroid: blob_to_vector(&centroid_bytes),
                        member_count: row.get(2)?,
                        p95_distance: row.get(3)?,
                        avg_confidence: row.get(4)?,
                        created_at: parse_datetime(row.get::<_, String>(5)?),
                        last_recomputed: parse_datetime(row.get::<_, String>(6)?),
                    })
                },
            )
            .optional()
            .sq()
    }

    pub fn get_all_clusters(&self) -> Result<Vec<flowforge_core::PatternCluster>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, centroid, member_count, p95_distance, avg_confidence, created_at, last_recomputed
                 FROM pattern_clusters ORDER BY member_count DESC",
            )
            .sq()?;
        let rows = stmt
            .query_map([], |row| {
                let centroid_bytes: Vec<u8> = row.get(1)?;
                Ok(flowforge_core::PatternCluster {
                    id: row.get(0)?,
                    centroid: blob_to_vector(&centroid_bytes),
                    member_count: row.get(2)?,
                    p95_distance: row.get(3)?,
                    avg_confidence: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                    last_recomputed: parse_datetime(row.get::<_, String>(6)?),
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn delete_all_clusters(&self) -> Result<()> {
        self.with_transaction(|| {
            self.conn
                .execute("UPDATE hnsw_entries SET cluster_id = NULL", [])
                .sq()?;
            self.conn.execute("DELETE FROM pattern_clusters", []).sq()?;
            Ok(())
        })
    }

    pub fn set_vector_cluster_id(&self, vector_id: i64, cluster_id: Option<i64>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE hnsw_entries SET cluster_id = ?1 WHERE id = ?2",
                params![cluster_id, vector_id],
            )
            .sq()?;
        Ok(())
    }

    pub fn get_vector_cluster_id(&self, vector_id: i64) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT cluster_id FROM hnsw_entries WHERE id = ?1",
                params![vector_id],
                |row| row.get(0),
            )
            .optional()
            .sq()
            .map(|o| o.flatten())
    }

    /// Pre-load cluster sizes for all pattern vectors in one JOIN query.
    /// Returns a map from vector_id → cluster member_count.
    /// Eliminates the N+1 pattern of get_vector_cluster_id + get_cluster per pattern.
    pub fn get_vector_cluster_sizes(&self) -> Result<std::collections::HashMap<i64, i64>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT h.id, COALESCE(pc.member_count, 0)
                 FROM hnsw_entries h
                 LEFT JOIN pattern_clusters pc ON pc.id = h.cluster_id
                 WHERE h.source_type IN ('pattern_short', 'pattern_long')
                   AND h.cluster_id IS NOT NULL",
            )
            .sq()?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .sq()?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (id, count) = row.sq()?;
            map.insert(id, count);
        }
        Ok(map)
    }

    pub fn count_vectors(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM hnsw_entries", [], |row| row.get(0))
            .sq()
    }

    pub fn count_outlier_vectors(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM hnsw_entries WHERE cluster_id IS NULL AND source_type IN ('pattern_short', 'pattern_long')",
                [],
                |row| row.get(0),
            )
            .sq()
    }

    pub fn get_all_pattern_vectors(&self) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, vector FROM hnsw_entries WHERE source_type IN ('pattern_short', 'pattern_long')",
            )
            .sq()?;
        let rows = stmt
            .query_map([], |row| {
                let bytes: Vec<u8> = row.get(1)?;
                Ok((row.get(0)?, blob_to_vector(&bytes)))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn update_cluster_stats(
        &self,
        cluster_id: i64,
        member_count: i64,
        p95_distance: f64,
        avg_confidence: f64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE pattern_clusters SET member_count = ?1, p95_distance = ?2, avg_confidence = ?3, last_recomputed = ?4 WHERE id = ?5",
                params![member_count, p95_distance, avg_confidence, now, cluster_id],
            )
            .sq()?;
        Ok(())
    }
}
