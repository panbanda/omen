//! Semantic search module for natural language code queries.
//!
//! This module provides semantic search capabilities over code symbols using
//! vector embeddings. Supports multiple embedding providers:
//!
//! - **Candle (default)**: Local inference with BAAI/bge-small-en-v1.5 model
//! - **Ollama**: Local Ollama server (bge-m3, nomic-embed-text, etc.)
//! - **OpenAI**: text-embedding-3-small, text-embedding-3-large, etc.
//! - **Cohere**: embed-english-v3.0, embed-multilingual-v3.0, etc.
//! - **Voyage**: voyage-code-2 (optimized for code), voyage-2, etc.
//!
//! # Architecture
//!
//! - **provider**: Abstraction for pluggable embedding backends
//! - **candle_provider**: Local candle inference (BAAI/bge-small-en-v1.5)
//! - **api_provider**: Third-party API providers (Ollama, OpenAI, Cohere, Voyage)
//! - **model**: Downloads and manages the candle model
//! - **embed**: High-level embedding generation interface
//! - **cache**: LanceDB storage for embeddings and vector search
//! - **sync**: Incremental indexing and staleness detection
//! - **search**: Query engine for nearest neighbor search
//!
//! # Usage
//!
//! ```no_run
//! use omen::semantic::{SemanticSearch, SearchConfig, EmbeddingProviderConfig};
//!
//! // Default: local candle inference
//! let config = SearchConfig::default();
//! let search = SemanticSearch::new(&config, "/path/to/repo").unwrap();
//!
//! // Or use OpenAI
//! let config = SearchConfig {
//!     provider: EmbeddingProviderConfig::OpenAI {
//!         api_key: None, // uses OPENAI_API_KEY env var
//!         model: "text-embedding-3-small".to_string(),
//!     },
//!     ..Default::default()
//! };
//! let search = SemanticSearch::new(&config, "/path/to/repo").unwrap();
//!
//! let results = search.search("function that handles HTTP requests", Some(10)).unwrap();
//! ```

pub mod api_provider;
pub mod cache;
pub mod candle_provider;
pub mod chunker;
pub mod embed;
pub mod metrics;
pub mod model;
pub mod multi;
pub mod provider;
pub mod search;
pub mod sync;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::core::{FileSet, Result};

pub use cache::EmbeddingCache;
pub use embed::EmbeddingEngine;
pub use model::ModelManager;
pub use multi::MultiRepoSearch;
pub use provider::{EmbeddingProvider, EmbeddingProviderConfig};
pub use search::{SearchEngine, SearchOutput, SearchResult};
pub use sync::{SyncManager, SyncStats};

/// Configuration for semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Embedding provider configuration (default: Candle local inference).
    #[serde(default)]
    pub provider: EmbeddingProviderConfig,
    /// Path to the LanceDB cache directory (default: .omen/search.lance)
    pub cache_path: Option<PathBuf>,
    /// Maximum number of results to return
    pub max_results: usize,
    /// Minimum similarity score (0-1)
    pub min_score: f32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            provider: EmbeddingProviderConfig::default(),
            cache_path: None,
            max_results: 20,
            min_score: 0.3,
        }
    }
}

/// High-level semantic search interface.
pub struct SemanticSearch {
    cache: EmbeddingCache,
    engine: EmbeddingEngine,
    root_path: PathBuf,
    config: SearchConfig,
}

impl SemanticSearch {
    /// Create a new semantic search instance.
    pub fn new(config: &SearchConfig, root_path: impl AsRef<Path>) -> Result<Self> {
        let root_path = root_path.as_ref().to_path_buf();

        // Determine cache path
        let cache_path = config
            .cache_path
            .clone()
            .unwrap_or_else(|| root_path.join(".omen").join("search.lance"));

        // Create cache directory if needed
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let cache = EmbeddingCache::open(&cache_path)?;
        let engine = EmbeddingEngine::with_config(&config.provider)?;

        Ok(Self {
            cache,
            engine,
            root_path,
            config: config.clone(),
        })
    }

    /// Create semantic search with a custom cache.
    pub fn with_cache(cache: EmbeddingCache, engine: EmbeddingEngine, root_path: PathBuf) -> Self {
        Self {
            cache,
            engine,
            root_path,
            config: SearchConfig::default(),
        }
    }

    /// Index the repository (or update the index).
    pub fn index(&self, file_config: &Config) -> Result<SyncStats> {
        let file_set = FileSet::from_path(&self.root_path, file_config)?;
        let sync_manager = SyncManager::new(&self.cache, &self.engine, String::new());
        sync_manager.sync(&file_set, &self.root_path)
    }

    /// Search for symbols matching the query.
    pub fn search(&self, query: &str, top_k: Option<usize>) -> Result<SearchOutput> {
        let top_k = top_k.unwrap_or(self.config.max_results);
        let search_engine = SearchEngine::new(&self.cache, &self.engine);

        let results = search_engine.search_with_threshold(query, top_k, self.config.min_score)?;
        let total_symbols = self.cache.symbol_count()?;

        Ok(SearchOutput::new(query.to_string(), total_symbols, results))
    }

    /// Search within specific files.
    pub fn search_in_files(
        &self,
        query: &str,
        file_paths: &[&str],
        top_k: Option<usize>,
    ) -> Result<SearchOutput> {
        let top_k = top_k.unwrap_or(self.config.max_results);
        let search_engine = SearchEngine::new(&self.cache, &self.engine);

        let results = search_engine.search_in_files(query, file_paths, top_k)?;
        let total_symbols = self.cache.symbol_count()?;

        Ok(SearchOutput::new(query.to_string(), total_symbols, results))
    }

    /// Get the number of indexed symbols.
    pub fn symbol_count(&self) -> Result<usize> {
        self.cache.symbol_count()
    }

    /// Get the root path.
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    /// Get the embedding provider name.
    pub fn provider_name(&self) -> &str {
        self.engine.provider_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_config_default() {
        let config = SearchConfig::default();
        assert_eq!(config.max_results, 20);
        assert_eq!(config.min_score, 0.3);
        assert!(config.cache_path.is_none());
        assert_eq!(config.provider, EmbeddingProviderConfig::Candle);
    }

    #[test]
    fn test_search_config_serialization() {
        let config = SearchConfig {
            provider: EmbeddingProviderConfig::Candle,
            cache_path: Some(PathBuf::from("/tmp/search.db")),
            max_results: 10,
            min_score: 0.5,
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("10"));
        assert!(json.contains("0.5"));

        let deserialized: SearchConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_results, 10);
    }

    #[test]
    fn test_search_config_with_openai() {
        let config = SearchConfig {
            provider: EmbeddingProviderConfig::OpenAI {
                api_key: Some("sk-test".to_string()),
                model: "text-embedding-3-small".to_string(),
            },
            ..Default::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("openai"));
        assert!(json.contains("text-embedding-3-small"));
    }
}
