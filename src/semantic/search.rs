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
    /// Chunk index within the symbol (0-based).
    pub chunk_index: u32,
    /// Total number of chunks for this symbol.
    pub total_chunks: u32,
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

        // Request extra results to account for deduplication of multi-chunk symbols
        let fetch_k = top_k * 3;
        let scored = self.cache.vector_search(&query_embedding, fetch_k)?;

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
                chunk_index: sym.chunk_index,
                total_chunks: sym.total_chunks,
            })
            .collect();

        Ok(deduplicate_chunks(results, top_k))
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

        let fetch_k = top_k * 3;
        let scored = self
            .cache
            .vector_search_in_files(&query_embedding, file_paths, fetch_k)?;

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
                chunk_index: sym.chunk_index,
                total_chunks: sym.total_chunks,
            })
            .collect();

        Ok(deduplicate_chunks(results, top_k))
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

/// Deduplicate search results from multi-chunk symbols.
/// Keeps only the highest-scoring chunk per (file_path, symbol_name) pair.
fn deduplicate_chunks(results: Vec<SearchResult>, limit: usize) -> Vec<SearchResult> {
    use std::collections::HashMap;

    let mut best: HashMap<(String, String), SearchResult> = HashMap::new();

    for result in results {
        let key = (result.file_path.clone(), result.symbol_name.clone());
        let entry = best.entry(key);
        entry
            .and_modify(|existing| {
                if result.score > existing.score {
                    *existing = result.clone();
                }
            })
            .or_insert(result);
    }

    let mut deduped: Vec<SearchResult> = best.into_values().collect();
    deduped.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    deduped.truncate(limit);
    deduped
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
            chunk_index: 0,
            total_chunks: 1,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("main"));
        assert!(json.contains("0.95"));
        assert!(json.contains("chunk_index"));
        assert!(json.contains("total_chunks"));

        let deserialized: SearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.symbol_name, "main");
        assert_eq!(deserialized.chunk_index, 0);
        assert_eq!(deserialized.total_chunks, 1);
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
                chunk_index: 0,
                total_chunks: 1,
            }],
        };

        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("test query"));
        assert!(json.contains("100"));
    }

    #[test]
    fn test_deduplicate_chunks_keeps_best_score() {
        let results = vec![
            SearchResult {
                file_path: "src/a.rs".to_string(),
                symbol_name: "func".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn func()".to_string(),
                start_line: 1,
                end_line: 10,
                score: 0.7,
                chunk_index: 0,
                total_chunks: 3,
            },
            SearchResult {
                file_path: "src/a.rs".to_string(),
                symbol_name: "func".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn func()".to_string(),
                start_line: 11,
                end_line: 20,
                score: 0.9,
                chunk_index: 1,
                total_chunks: 3,
            },
            SearchResult {
                file_path: "src/a.rs".to_string(),
                symbol_name: "func".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn func()".to_string(),
                start_line: 21,
                end_line: 30,
                score: 0.5,
                chunk_index: 2,
                total_chunks: 3,
            },
        ];

        let deduped = deduplicate_chunks(results, 10);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].score, 0.9);
        assert_eq!(deduped[0].chunk_index, 1);
    }

    #[test]
    fn test_deduplicate_preserves_different_symbols() {
        let results = vec![
            SearchResult {
                file_path: "src/a.rs".to_string(),
                symbol_name: "func_a".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn func_a()".to_string(),
                start_line: 1,
                end_line: 5,
                score: 0.8,
                chunk_index: 0,
                total_chunks: 1,
            },
            SearchResult {
                file_path: "src/b.rs".to_string(),
                symbol_name: "func_b".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn func_b()".to_string(),
                start_line: 1,
                end_line: 5,
                score: 0.7,
                chunk_index: 0,
                total_chunks: 1,
            },
        ];

        let deduped = deduplicate_chunks(results, 10);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn test_deduplicate_respects_limit() {
        let results: Vec<SearchResult> = (0..10)
            .map(|i| SearchResult {
                file_path: format!("src/{}.rs", i),
                symbol_name: format!("func_{}", i),
                symbol_type: "function".to_string(),
                signature: format!("fn func_{}()", i),
                start_line: 1,
                end_line: 5,
                score: 1.0 - (i as f32 * 0.1),
                chunk_index: 0,
                total_chunks: 1,
            })
            .collect();

        let deduped = deduplicate_chunks(results, 5);
        assert_eq!(deduped.len(), 5);
        // Should be sorted by score descending
        assert!(deduped[0].score >= deduped[1].score);
    }
}
