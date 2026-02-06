//! Embedding engine for generating vector embeddings from code.
//!
//! Uses pluggable providers: candle (local) or third-party APIs (OpenAI, Cohere, Voyage).

use std::sync::Arc;

use crate::core::Result;
use crate::parser::FunctionNode;

use super::api_provider::{CohereProvider, OllamaProvider, OpenAIProvider, VoyageProvider};
use super::candle_provider::CandleProvider;
use super::provider::{EmbeddingProvider, EmbeddingProviderConfig, BGE_SMALL_EMBEDDING_DIM};

/// Embedding dimension for BAAI/bge-small-en-v1.5 (default candle provider).
pub const EMBEDDING_DIM: usize = BGE_SMALL_EMBEDDING_DIM;

/// Embedding engine using pluggable providers.
pub struct EmbeddingEngine {
    provider: Arc<dyn EmbeddingProvider>,
}

impl EmbeddingEngine {
    /// Create a new embedding engine with the default candle provider.
    pub fn new() -> Result<Self> {
        Self::with_config(&EmbeddingProviderConfig::default())
    }

    /// Create an embedding engine with a specific provider configuration.
    pub fn with_config(config: &EmbeddingProviderConfig) -> Result<Self> {
        let provider: Arc<dyn EmbeddingProvider> = match config {
            EmbeddingProviderConfig::Candle => Arc::new(CandleProvider::new()?),
            EmbeddingProviderConfig::Ollama { url, model } => {
                Arc::new(OllamaProvider::new(url.clone(), model.clone()))
            }
            EmbeddingProviderConfig::OpenAI { api_key, model } => {
                Arc::new(OpenAIProvider::new(api_key.clone(), model.clone())?)
            }
            EmbeddingProviderConfig::Cohere { api_key, model } => {
                Arc::new(CohereProvider::new(api_key.clone(), model.clone())?)
            }
            EmbeddingProviderConfig::Voyage { api_key, model } => {
                Arc::new(VoyageProvider::new(api_key.clone(), model.clone())?)
            }
        };

        Ok(Self { provider })
    }

    /// Create an embedding engine with a custom provider.
    pub fn with_provider(provider: Arc<dyn EmbeddingProvider>) -> Self {
        Self { provider }
    }

    /// Generate an embedding for a single text.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.provider.embed(text)
    }

    /// Generate embeddings for a batch of texts.
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.provider.embed_batch(texts)
    }

    /// Generate an embedding for a function/symbol.
    pub fn embed_symbol(&self, func: &FunctionNode, source: &str) -> Result<Vec<f32>> {
        let text = format_symbol_text(func, source);
        self.embed(&text)
    }

    /// Get the embedding dimension for the current provider.
    pub fn embedding_dim(&self) -> usize {
        self.provider.embedding_dim()
    }

    /// Get the provider name.
    pub fn provider_name(&self) -> &str {
        self.provider.name()
    }
}

/// Format a symbol for embedding generation.
pub fn format_symbol_text(func: &FunctionNode, source: &str) -> String {
    // Get function body from source by line numbers
    let lines: Vec<&str> = source.lines().collect();
    let start = (func.start_line as usize).saturating_sub(1);
    let end = (func.end_line as usize).min(lines.len());

    let body = if start < end && start < lines.len() {
        lines[start..end].join("\n")
    } else {
        func.signature.clone()
    };

    // Limit body length to avoid exceeding token limit
    let max_chars = 1500;
    if body.len() > max_chars {
        let end = body.floor_char_boundary(max_chars);
        format!("{}\n{}", func.signature, &body[..end])
    } else {
        body
    }
}

/// Compute cosine similarity between two embeddings.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_format_symbol_text() {
        let func = FunctionNode {
            name: "test_func".to_string(),
            start_line: 1,
            end_line: 3,
            body_byte_range: None,
            is_exported: true,
            signature: "fn test_func()".to_string(),
        };
        let source = "fn test_func() {\n    println!(\"hello\");\n}";
        let text = format_symbol_text(&func, source);
        assert!(text.contains("fn test_func()"));
        assert!(text.contains("println!"));
    }

    #[test]
    fn test_format_symbol_text_truncation() {
        let func = FunctionNode {
            name: "test_func".to_string(),
            start_line: 1,
            end_line: 1,
            body_byte_range: None,
            is_exported: true,
            signature: "fn test_func()".to_string(),
        };
        // Create a very long source
        let source = "x".repeat(3000);
        let text = format_symbol_text(&func, &source);
        assert!(text.len() <= 1600); // signature + truncated body
    }

    #[test]
    fn test_format_symbol_text_truncation_multibyte() {
        let func = FunctionNode {
            name: "test_func".to_string(),
            start_line: 1,
            end_line: 1,
            body_byte_range: None,
            is_exported: true,
            signature: "fn test_func()".to_string(),
        };
        // CJK characters are 3 bytes each; 600 chars = 1800 bytes > 1500
        let source = "\u{4e16}".repeat(600);
        let text = format_symbol_text(&func, &source);
        // Must not panic from slicing mid-character
        assert!(text.is_char_boundary(text.len()));
    }

    #[test]
    fn test_embedding_dim_constant() {
        assert_eq!(EMBEDDING_DIM, 384);
    }
}
