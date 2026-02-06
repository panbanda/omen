//! Query engine for semantic search.
//!
//! Uses LanceDB native vector search for approximate nearest neighbor queries.

use serde::{Deserialize, Serialize};

use crate::core::Result;

use super::cache::EmbeddingCache;
use super::embed::EmbeddingEngine;

/// A search result with similarity score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// File path relative to repository root.
    pub file_path: String,
    /// Symbol name.
    pub symbol_name: String,
    /// Symbol type (function, class, etc.).
    pub symbol_type: String,
    /// Symbol signature.
    pub signature: String,
    /// Start line in file (1-indexed).
    pub start_line: u32,
    /// End line in file (1-indexed).
    pub end_line: u32,
    /// Similarity score (0-1, higher is more similar).
    pub score: f32,
}

/// Search engine for semantic code search.
pub struct SearchEngine<'a> {
    cache: &'a EmbeddingCache,
    engine: &'a EmbeddingEngine,
}

impl<'a> SearchEngine<'a> {
    /// Create a new search engine.
    pub fn new(cache: &'a EmbeddingCache, engine: &'a EmbeddingEngine) -> Self {
        Self { cache, engine }
    }

    /// Search for symbols matching the query using LanceDB vector search.
    pub fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchResult>> {
        let query_embedding = self.engine.embed(query)?;

        let scored = self.cache.vector_search(&query_embedding, top_k)?;

        let results: Vec<SearchResult> = scored
            .into_iter()
            .map(|(sym, score)| SearchResult {
                file_path: sym.file_path,
                symbol_name: sym.symbol_name,
                symbol_type: sym.symbol_type,
                signature: sym.signature,
                start_line: sym.start_line,
                end_line: sym.end_line,
                score,
            })
            .collect();

        Ok(results)
    }

    /// Search for symbols with a minimum score threshold.
    pub fn search_with_threshold(
        &self,
        query: &str,
        top_k: usize,
        min_score: f32,
    ) -> Result<Vec<SearchResult>> {
        let results = self.search(query, top_k)?;
        Ok(results
            .into_iter()
            .filter(|r| r.score >= min_score)
            .collect())
    }

    /// Search within specific files.
    pub fn search_in_files(
        &self,
        query: &str,
        file_paths: &[&str],
        top_k: usize,
    ) -> Result<Vec<SearchResult>> {
        let query_embedding = self.engine.embed(query)?;

        let scored = self
            .cache
            .vector_search_in_files(&query_embedding, file_paths, top_k)?;

        let results: Vec<SearchResult> = scored
            .into_iter()
            .map(|(sym, score)| SearchResult {
                file_path: sym.file_path,
                symbol_name: sym.symbol_name,
                symbol_type: sym.symbol_type,
                signature: sym.signature,
                start_line: sym.start_line,
                end_line: sym.end_line,
                score,
            })
            .collect();

        Ok(results)
    }
}

/// Overall search result container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOutput {
    /// The original query.
    pub query: String,
    /// Number of indexed symbols searched.
    pub total_symbols: usize,
    /// Search results.
    pub results: Vec<SearchResult>,
}

impl SearchOutput {
    /// Create a new search output.
    pub fn new(query: String, total_symbols: usize, results: Vec<SearchResult>) -> Self {
        Self {
            query,
            total_symbols,
            results,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_result_serialization() {
        let result = SearchResult {
            file_path: "src/main.rs".to_string(),
            symbol_name: "main".to_string(),
            symbol_type: "function".to_string(),
            signature: "fn main()".to_string(),
            start_line: 1,
            end_line: 10,
            score: 0.95,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("main"));
        assert!(json.contains("0.95"));

        let deserialized: SearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.symbol_name, "main");
    }

    #[test]
    fn test_search_output_serialization() {
        let output = SearchOutput {
            query: "test query".to_string(),
            total_symbols: 100,
            results: vec![SearchResult {
                file_path: "src/main.rs".to_string(),
                symbol_name: "test".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn test()".to_string(),
                start_line: 1,
                end_line: 5,
                score: 0.8,
            }],
        };

        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("test query"));
        assert!(json.contains("100"));
    }
}
