pub mod clustering;
pub mod db;
pub mod embedding;
pub mod hnsw;
pub mod patterns;
#[cfg(test)]
pub mod test_helpers;
pub mod trajectory;

pub use db::code_index::CodeIndexEntry;
pub use db::project_intelligence::IntelligenceSection;
pub use db::failure_patterns::FailurePattern;
pub use db::vectors::{new_multi_hnsw_cache, CachedSourceIndex, MultiHnswCache, VectorSearchResult};
pub use db::MemoryDb;
#[cfg(feature = "semantic")]
pub use embedding::SemanticEmbedder;
pub use embedding::{cosine_similarity, default_embedder, Embedder, HashEmbedder};
pub use hnsw::HnswIndex;
pub use patterns::{new_hnsw_cache, HnswCache, PatternStore};
