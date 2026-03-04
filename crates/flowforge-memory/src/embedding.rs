use xxhash_rust::xxh3::xxh3_64;

/// Embedding version — increment when the embedding algorithm changes.
/// Used to trigger re-embedding of stored vectors on next consolidation.
pub const EMBEDDING_VERSION: u32 = 3;

/// Trait for text embedding backends.
pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Vec<f32>;
    fn dimension(&self) -> usize;
}

/// Compute cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Hash-based deterministic embeddings using character and word n-gram feature hashing.
/// Combines character bigrams (captures subword patterns) with word unigrams and bigrams
/// (captures semantic-level token overlap).
pub struct HashEmbedder {
    dim: usize,
}

impl HashEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self::new(128)
    }
}

impl Embedder for HashEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0f32; self.dim];
        let text_lower = text.to_lowercase();
        let chars: Vec<char> = text_lower.chars().collect();

        if chars.is_empty() {
            return vector;
        }

        // Unigrams
        for &ch in &chars {
            let hash = xxh3_64(ch.to_string().as_bytes());
            let idx = (hash as usize) % self.dim;
            let sign = if (hash >> 32) & 1 == 0 { 1.0 } else { -1.0 };
            vector[idx] += sign;
        }

        // Character bigrams
        for pair in chars.windows(2) {
            let bigram = format!("{}{}", pair[0], pair[1]);
            let hash = xxh3_64(bigram.as_bytes());
            let idx = (hash as usize) % self.dim;
            let sign = if (hash >> 32) & 1 == 0 { 1.0 } else { -1.0 };
            vector[idx] += sign * 1.5;
        }

        // Word unigrams (weight 2.0) — captures token-level semantics
        let words: Vec<&str> = text_lower.split_whitespace().collect();
        for word in &words {
            let hash = xxh3_64(word.as_bytes());
            let idx = (hash as usize) % self.dim;
            let sign = if (hash >> 32) & 1 == 0 { 1.0 } else { -1.0 };
            vector[idx] += sign * 2.0;
        }

        // Word bigrams (weight 3.0) — captures phrase-level patterns
        for pair in words.windows(2) {
            let bigram = format!("{} {}", pair[0], pair[1]);
            let hash = xxh3_64(bigram.as_bytes());
            let idx = (hash as usize) % self.dim;
            let sign = if (hash >> 32) & 1 == 0 { 1.0 } else { -1.0 };
            vector[idx] += sign * 3.0;
        }

        // L2 normalize
        l2_normalize(&mut vector);

        vector
    }

    fn dimension(&self) -> usize {
        self.dim
    }
}

/// Create the default embedder based on config.
/// Returns SemanticEmbedder when the "semantic" feature is enabled and config allows it,
/// otherwise falls back to HashEmbedder.
pub fn default_embedder(
    #[allow(unused)] config: &flowforge_core::config::PatternsConfig,
) -> Box<dyn Embedder> {
    #[cfg(feature = "semantic")]
    if config.semantic_embeddings {
        return Box::new(SemanticEmbedder::new());
    }
    Box::new(HashEmbedder::default())
}

fn l2_normalize(vector: &mut [f32]) {
    let norm: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in vector.iter_mut() {
            *v /= norm;
        }
    }
}

#[cfg(feature = "semantic")]
pub struct SemanticEmbedder {
    model: std::sync::Mutex<Option<fastembed::TextEmbedding>>,
    show_progress: bool,
}

#[cfg(feature = "semantic")]
impl SemanticEmbedder {
    pub fn new() -> Self {
        Self {
            model: std::sync::Mutex::new(None),
            show_progress: false,
        }
    }

    fn ensure_model(&self) -> std::sync::MutexGuard<'_, Option<fastembed::TextEmbedding>> {
        let mut guard = self.model.lock().unwrap();
        if guard.is_none() {
            *guard = Some(
                fastembed::TextEmbedding::try_new(
                    fastembed::InitOptions::new(fastembed::EmbeddingModel::AllMiniLML6V2Q)
                        .with_show_download_progress(self.show_progress),
                )
                .expect("Failed to load embedding model"),
            );
        }
        guard
    }

    /// Create a new SemanticEmbedder with download progress enabled (for CLI).
    pub fn new_with_progress() -> Self {
        let embedder = Self {
            model: std::sync::Mutex::new(None),
            show_progress: true,
        };
        // Eagerly init with progress
        drop(embedder.ensure_model());
        embedder
    }
}

#[cfg(feature = "semantic")]
impl Default for SemanticEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "semantic")]
impl Embedder for SemanticEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut guard = self.ensure_model();
        guard
            .as_mut()
            .unwrap()
            .embed(vec![text], None)
            .unwrap_or_else(|_| vec![vec![0.0; 384]])[0]
            .clone()
    }

    fn dimension(&self) -> usize {
        384
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embed_produces_correct_dim() {
        let emb = HashEmbedder::new(128);
        let vec = emb.embed("hello world");
        assert_eq!(vec.len(), 128);
    }

    #[test]
    fn test_embed_is_normalized() {
        let emb = HashEmbedder::new(128);
        let vec = emb.embed("test input text");
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_embed_is_deterministic() {
        let emb = HashEmbedder::new(128);
        let v1 = emb.embed("same text");
        let v2 = emb.embed("same text");
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let emb = HashEmbedder::new(128);
        let v = emb.embed("hello");
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_cosine_similarity_different() {
        let emb = HashEmbedder::new(128);
        let v1 = emb.embed("rust programming language");
        let v2 = emb.embed("python programming language");
        let v3 = emb.embed("cooking recipes for dinner");
        let sim_related = cosine_similarity(&v1, &v2);
        let sim_unrelated = cosine_similarity(&v1, &v3);
        assert!(sim_related > sim_unrelated);
    }

    #[test]
    fn test_embed_empty_string() {
        let emb = HashEmbedder::new(128);
        let vec = emb.embed("");
        assert!(vec.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_word_ngrams_boost_similarity() {
        let emb = HashEmbedder::new(128);
        let v1 = emb.embed("fix auth bug");
        let v2 = emb.embed("fix authentication bug");
        let sim = cosine_similarity(&v1, &v2);
        assert!(
            sim > 0.4,
            "Expected similarity > 0.4 for shared-word phrases, got {sim}"
        );
    }

    #[test]
    fn test_word_ngrams_unrelated_low_similarity() {
        let emb = HashEmbedder::new(128);
        let v1 = emb.embed("fix authentication bug");
        let v2 = emb.embed("deploy kubernetes service");
        let sim = cosine_similarity(&v1, &v2);
        assert!(
            sim < 0.5,
            "Expected similarity < 0.5 for unrelated phrases, got {sim}"
        );
    }

    #[cfg(feature = "semantic")]
    #[test]
    fn test_semantic_similarity() {
        let emb = SemanticEmbedder::new();
        let v1 = emb.embed("deploy kubernetes");
        let v2 = emb.embed("deploy k8s");
        let sim = cosine_similarity(&v1, &v2);
        assert!(
            sim > 0.65,
            "Expected semantic similarity > 0.65 for k8s/kubernetes, got {sim}"
        );
    }

    #[cfg(feature = "semantic")]
    #[test]
    fn test_semantic_unrelated_low() {
        let emb = SemanticEmbedder::new();
        let v1 = emb.embed("deploy k8s");
        let v2 = emb.embed("bake cookies");
        let sim = cosine_similarity(&v1, &v2);
        assert!(
            sim < 0.3,
            "Expected semantic similarity < 0.3 for unrelated phrases, got {sim}"
        );
    }

    #[cfg(feature = "semantic")]
    #[test]
    fn test_semantic_dimension() {
        let emb = SemanticEmbedder::new();
        assert_eq!(emb.dimension(), 384);
        let v = emb.embed("test");
        assert_eq!(v.len(), 384);
    }

    #[test]
    fn test_fallback_to_hash() {
        let config = flowforge_core::config::PatternsConfig {
            semantic_embeddings: false,
            ..Default::default()
        };
        let emb = default_embedder(&config);
        assert_eq!(emb.dimension(), 128);
    }
}
