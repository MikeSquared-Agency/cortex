use crate::error::{CortexError, Result};
use crate::types::{Embedding, Node};
use fastembed::{InitOptions, TextEmbedding as FastEmbedModel, EmbeddingModel};

/// Service for generating text embeddings
pub trait EmbeddingService: Send + Sync {
    /// Generate embedding for a single text.
    fn embed(&self, text: &str) -> Result<Embedding>;

    /// Batch embedding for efficiency. FastEmbed batches internally.
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>>;

    /// Embedding dimension for the current model.
    fn dimension(&self) -> usize;

    /// Model identifier string.
    fn model_name(&self) -> &str;
}

/// FastEmbed-based embedding service
pub struct FastEmbedService {
    model: FastEmbedModel,
    model_name: String,
    dimension: usize,
}

impl FastEmbedService {
    /// Create a new FastEmbed service with the default model
    pub fn new() -> Result<Self> {
        Self::with_model(EmbeddingModel::BGESmallENV15)
    }

    /// Create a new FastEmbed service with a specific model
    pub fn with_model(model: EmbeddingModel) -> Result<Self> {
        let init_options = InitOptions::new(model.clone());

        let fastembed_model = FastEmbedModel::try_new(init_options)
            .map_err(|e| CortexError::Validation(format!("Failed to initialize FastEmbed: {}", e)))?;

        let model_name = format!("{:?}", model);
        // Determine dimension from model
        let dimension = match model {
            EmbeddingModel::BGESmallENV15 => 384,
            EmbeddingModel::BGEBaseENV15 => 768,
            EmbeddingModel::BGELargeENV15 => 1024,
            EmbeddingModel::AllMiniLML6V2 => 384,
            EmbeddingModel::AllMiniLML12V2 => 384,
            _ => 384, // Safe default for unknown models
        };

        Ok(Self {
            model: fastembed_model,
            model_name,
            dimension,
        })
    }
}

impl EmbeddingService for FastEmbedService {
    fn embed(&self, text: &str) -> Result<Embedding> {
        let embeddings = self.model
            .embed(vec![text.to_string()], None)
            .map_err(|e| CortexError::Validation(format!("Embedding failed: {}", e)))?;

        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| CortexError::Validation("No embedding generated".to_string()))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>> {
        let embeddings = self.model
            .embed(texts.to_vec(), None)
            .map_err(|e| CortexError::Validation(format!("Batch embedding failed: {}", e)))?;

        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }
}

impl Default for FastEmbedService {
    fn default() -> Self {
        Self::new().expect("Failed to initialize default FastEmbed model")
    }
}

impl<E: EmbeddingService> EmbeddingService for std::sync::Arc<E> {
    fn embed(&self, text: &str) -> Result<Embedding> {
        (**self).embed(text)
    }
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>> {
        (**self).embed_batch(texts)
    }
    fn dimension(&self) -> usize {
        (**self).dimension()
    }
    fn model_name(&self) -> &str {
        (**self).model_name()
    }
}

/// Generate the embedding input text for a node
pub fn embedding_input(node: &Node) -> String {
    let kind_str = match node.kind {
        crate::types::NodeKind::Agent => "Agent",
        crate::types::NodeKind::Decision => "Decision",
        crate::types::NodeKind::Fact => "Fact",
        crate::types::NodeKind::Event => "Event",
        crate::types::NodeKind::Goal => "Goal",
        crate::types::NodeKind::Preference => "Preference",
        crate::types::NodeKind::Pattern => "Pattern",
        crate::types::NodeKind::Observation => "Observation",
    };

    format!(
        "{}: {}\n{}\ntags: {}",
        kind_str,
        node.data.title,
        node.data.body,
        node.data.tags.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NodeKind, Source};

    #[test]
    fn test_embedding_input_format() {
        let node = Node::new(
            NodeKind::Fact,
            "Test title".to_string(),
            "Test body content".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        );

        let input = embedding_input(&node);
        assert!(input.contains("Fact: Test title"));
        assert!(input.contains("Test body content"));
    }

    #[test]
    #[ignore] // Requires downloading model
    fn test_fastembed_service() {
        let service = FastEmbedService::new().unwrap();

        assert_eq!(service.dimension(), 384);

        let text = "This is a test sentence for embedding generation.";
        let embedding = service.embed(text).unwrap();

        assert_eq!(embedding.len(), 384);
    }

    #[test]
    #[ignore] // Requires downloading model
    fn test_batch_embedding() {
        let service = FastEmbedService::new().unwrap();

        let texts = vec![
            "First sentence".to_string(),
            "Second sentence".to_string(),
            "Third sentence".to_string(),
        ];

        let embeddings = service.embed_batch(&texts).unwrap();
        assert_eq!(embeddings.len(), 3);
        assert_eq!(embeddings[0].len(), 384);
    }

    #[test]
    #[ignore] // Requires downloading model
    fn test_similar_texts_high_similarity() {
        let service = FastEmbedService::new().unwrap();

        let text1 = "The cat sat on the mat";
        let text2 = "A cat was sitting on a mat";

        let emb1 = service.embed(text1).unwrap();
        let emb2 = service.embed(text2).unwrap();

        let similarity = cosine_similarity(&emb1, &emb2);
        assert!(similarity > 0.7, "Similar texts should have high similarity: {}", similarity);
    }

    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm_a * norm_b)
    }
}
