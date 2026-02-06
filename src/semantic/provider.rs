//! Embedding provider abstraction for pluggable embedding backends.
//!
//! Supports:
//! - Local inference with candle (BAAI/bge-small-en-v1.5)
//! - Third-party API providers (OpenAI, Cohere, etc.)

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::core::Result;

/// Embedding dimension for BAAI/bge-small-en-v1.5 (and all-MiniLM-L6-v2).
pub const BGE_SMALL_EMBEDDING_DIM: usize = 384;

/// Embedding dimension for OpenAI text-embedding-3-small.
pub const OPENAI_SMALL_EMBEDDING_DIM: usize = 1536;

/// Trait for embedding providers.
pub trait EmbeddingProvider: Send + Sync {
    /// Generate an embedding for a single text.
    fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Generate embeddings for a batch of texts.
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;

    /// Get the embedding dimension for this provider.
    fn embedding_dim(&self) -> usize;

    /// Get the provider name for logging/debugging.
    fn name(&self) -> &str;
}

/// Configuration for which embedding provider to use.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum EmbeddingProviderConfig {
    /// Local candle inference with BAAI/bge-small-en-v1.5 (default).
    #[serde(rename = "candle")]
    #[default]
    Candle,
    /// Ollama local embeddings API.
    #[serde(rename = "ollama")]
    Ollama {
        /// Ollama server URL (default: http://localhost:11434).
        #[serde(default = "default_ollama_url")]
        url: String,
        /// Model to use (default: bge-m3).
        #[serde(default = "default_ollama_model")]
        model: String,
    },
    /// OpenAI embeddings API.
    #[serde(rename = "openai")]
    OpenAI {
        /// API key (can also use OPENAI_API_KEY env var).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
        /// Model to use (default: text-embedding-3-small).
        #[serde(default = "default_openai_model")]
        model: String,
    },
    /// Cohere embeddings API.
    #[serde(rename = "cohere")]
    Cohere {
        /// API key (can also use COHERE_API_KEY env var).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
        /// Model to use (default: embed-english-v3.0).
        #[serde(default = "default_cohere_model")]
        model: String,
    },
    /// Voyage AI embeddings API.
    #[serde(rename = "voyage")]
    Voyage {
        /// API key (can also use VOYAGE_API_KEY env var).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
        /// Model to use (default: voyage-code-2).
        #[serde(default = "default_voyage_model")]
        model: String,
    },
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_ollama_model() -> String {
    "bge-m3".to_string()
}

fn default_openai_model() -> String {
    "text-embedding-3-small".to_string()
}

fn default_cohere_model() -> String {
    "embed-english-v3.0".to_string()
}

fn default_voyage_model() -> String {
    "voyage-code-2".to_string()
}

impl fmt::Display for EmbeddingProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Candle => write!(f, "candle (bge-small-en-v1.5)"),
            Self::Ollama { model, .. } => write!(f, "ollama ({})", model),
            Self::OpenAI { model, .. } => write!(f, "openai ({})", model),
            Self::Cohere { model, .. } => write!(f, "cohere ({})", model),
            Self::Voyage { model, .. } => write!(f, "voyage ({})", model),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_config_default() {
        let config = EmbeddingProviderConfig::default();
        assert_eq!(config, EmbeddingProviderConfig::Candle);
    }

    #[test]
    fn test_provider_config_serialization() {
        let config = EmbeddingProviderConfig::Candle;
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("candle"));

        let deserialized: EmbeddingProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, EmbeddingProviderConfig::Candle);
    }

    #[test]
    fn test_openai_config_serialization() {
        let config = EmbeddingProviderConfig::OpenAI {
            api_key: None,
            model: "text-embedding-3-small".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("openai"));
        assert!(json.contains("text-embedding-3-small"));
    }

    #[test]
    fn test_provider_display() {
        assert_eq!(
            EmbeddingProviderConfig::Candle.to_string(),
            "candle (bge-small-en-v1.5)"
        );
        assert_eq!(
            EmbeddingProviderConfig::Ollama {
                url: "http://localhost:11434".to_string(),
                model: "bge-m3".to_string(),
            }
            .to_string(),
            "ollama (bge-m3)"
        );
        assert_eq!(
            EmbeddingProviderConfig::OpenAI {
                api_key: None,
                model: "text-embedding-3-small".to_string()
            }
            .to_string(),
            "openai (text-embedding-3-small)"
        );
    }

    #[test]
    fn test_ollama_config_serialization() {
        let config = EmbeddingProviderConfig::Ollama {
            url: "http://localhost:11434".to_string(),
            model: "bge-m3".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("ollama"));
        assert!(json.contains("bge-m3"));

        let deserialized: EmbeddingProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, config);
    }
}
