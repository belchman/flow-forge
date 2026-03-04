use instant_distance::{Builder, HnswMap, Search};

use crate::embedding::cosine_similarity;

/// Wrapper around instant-distance HNSW index for vector search.
pub struct HnswIndex {
    map: Option<HnswMap<Point, i64>>,
}

#[derive(Clone)]
struct Point(Vec<f32>);

impl instant_distance::Point for Point {
    fn distance(&self, other: &Self) -> f32 {
        // Cosine distance = 1.0 - cosine_similarity
        1.0 - cosine_similarity(&self.0, &other.0)
    }
}

impl HnswIndex {
    pub fn new() -> Self {
        Self { map: None }
    }

    /// Build the HNSW index from stored vectors.
    /// Each entry is (external_id, vector).
    pub fn build(&mut self, points: &[(i64, Vec<f32>)]) {
        if points.is_empty() {
            self.map = None;
            return;
        }

        let hnsw_points: Vec<Point> = points.iter().map(|(_, v)| Point(v.clone())).collect();
        let values: Vec<i64> = points.iter().map(|(id, _)| *id).collect();

        let map = Builder::default().build(hnsw_points, values);
        self.map = Some(map);
    }

    /// Search for the k nearest neighbors to a query vector.
    /// Returns (external_id, distance) pairs sorted by distance.
    pub fn search(&self, query: &[f32], k: usize) -> Vec<(i64, f32)> {
        let map = match &self.map {
            Some(m) => m,
            None => return Vec::new(),
        };

        let query_point = Point(query.to_vec());
        let mut search = Search::default();
        let results = map.search(&query_point, &mut search);

        results
            .take(k)
            .map(|item| (*item.value, item.distance))
            .collect()
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_none()
    }
}

impl Default for HnswIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::Embedder;
    use crate::embedding::HashEmbedder;

    #[test]
    fn test_empty_index() {
        let index = HnswIndex::new();
        assert!(index.is_empty());
        let results = index.search(&[0.0; 128], 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_build_and_search() {
        let emb = HashEmbedder::new(128);
        let mut index = HnswIndex::new();

        let points = vec![
            (1, emb.embed("rust programming")),
            (2, emb.embed("python programming")),
            (3, emb.embed("cooking recipes")),
            (4, emb.embed("rust cargo build")),
        ];

        index.build(&points);
        assert!(!index.is_empty());

        let query = emb.embed("rust language");
        let results = index.search(&query, 2);

        assert_eq!(results.len(), 2);
        // Closest should be one of the rust-related entries
        assert!(results[0].0 == 1 || results[0].0 == 4);
    }

    #[test]
    fn test_build_single_point() {
        let emb = HashEmbedder::new(128);
        let mut index = HnswIndex::new();

        index.build(&[(42, emb.embed("hello"))]);
        let results = index.search(&emb.embed("hello"), 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 42);
    }
}
