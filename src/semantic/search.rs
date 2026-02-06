//! Query engine for semantic search.
//!
//! Uses LanceDB native vector search for approximate nearest neighbor queries.

use serde::{Deserialize, Serialize};

use crate::core::Result;

use super::cache::EmbeddingCache;
use super::embed::EmbeddingEngine;
use super::metrics::quality_adjusted_score;

/// A search result with similarity score and quality metrics.
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
    /// Quality-adjusted score (semantic similarity * quality weight).
    pub score: f32,
    /// Raw semantic similarity score before quality adjustment.
    pub raw_score: f32,
    /// Chunk index within the symbol (0-based).
    pub chunk_index: u32,
    /// Total number of chunks for this symbol.
    pub total_chunks: u32,
    /// Cyclomatic complexity of the symbol.
    pub cyclomatic_complexity: u32,
    /// Cognitive complexity of the symbol.
    pub cognitive_complexity: u32,
    /// TDG score (0-100).
    pub tdg_score: f32,
    /// TDG grade (A+, A, B, C, D, F).
    pub tdg_grade: String,
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
            .map(|(sym, raw_score)| {
                let adjusted = quality_adjusted_score(raw_score, &sym.tdg_grade);
                SearchResult {
                    file_path: sym.file_path,
                    symbol_name: sym.symbol_name,
                    symbol_type: sym.symbol_type,
                    signature: sym.signature,
                    start_line: sym.start_line,
                    end_line: sym.end_line,
                    score: adjusted,
                    raw_score,
                    chunk_index: sym.chunk_index,
                    total_chunks: sym.total_chunks,
                    cyclomatic_complexity: sym.cyclomatic_complexity,
                    cognitive_complexity: sym.cognitive_complexity,
                    tdg_score: sym.tdg_score,
                    tdg_grade: sym.tdg_grade,
                }
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
            .map(|(sym, raw_score)| {
                let adjusted = quality_adjusted_score(raw_score, &sym.tdg_grade);
                SearchResult {
                    file_path: sym.file_path,
                    symbol_name: sym.symbol_name,
                    symbol_type: sym.symbol_type,
                    signature: sym.signature,
                    start_line: sym.start_line,
                    end_line: sym.end_line,
                    score: adjusted,
                    raw_score,
                    chunk_index: sym.chunk_index,
                    total_chunks: sym.total_chunks,
                    cyclomatic_complexity: sym.cyclomatic_complexity,
                    cognitive_complexity: sym.cognitive_complexity,
                    tdg_score: sym.tdg_score,
                    tdg_grade: sym.tdg_grade,
                }
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
            raw_score: 0.95,
            chunk_index: 0,
            total_chunks: 1,
            cyclomatic_complexity: 3,
            cognitive_complexity: 2,
            tdg_score: 25.0,
            tdg_grade: "A".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("main"));
        assert!(json.contains("0.95"));
        assert!(json.contains("chunk_index"));
        assert!(json.contains("total_chunks"));
        assert!(json.contains("cyclomatic_complexity"));
        assert!(json.contains("tdg_grade"));

        let deserialized: SearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.symbol_name, "main");
        assert_eq!(deserialized.chunk_index, 0);
        assert_eq!(deserialized.total_chunks, 1);
        assert_eq!(deserialized.cyclomatic_complexity, 3);
        assert_eq!(deserialized.tdg_grade, "A");
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
                raw_score: 0.8,
                chunk_index: 0,
                total_chunks: 1,
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 0.0,
                tdg_grade: String::new(),
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
                raw_score: 0.7,
                chunk_index: 0,
                total_chunks: 3,
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 0.0,
                tdg_grade: String::new(),
            },
            SearchResult {
                file_path: "src/a.rs".to_string(),
                symbol_name: "func".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn func()".to_string(),
                start_line: 11,
                end_line: 20,
                score: 0.9,
                raw_score: 0.9,
                chunk_index: 1,
                total_chunks: 3,
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 0.0,
                tdg_grade: String::new(),
            },
            SearchResult {
                file_path: "src/a.rs".to_string(),
                symbol_name: "func".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn func()".to_string(),
                start_line: 21,
                end_line: 30,
                score: 0.5,
                raw_score: 0.5,
                chunk_index: 2,
                total_chunks: 3,
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 0.0,
                tdg_grade: String::new(),
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
                raw_score: 0.8,
                chunk_index: 0,
                total_chunks: 1,
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 0.0,
                tdg_grade: String::new(),
            },
            SearchResult {
                file_path: "src/b.rs".to_string(),
                symbol_name: "func_b".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn func_b()".to_string(),
                start_line: 1,
                end_line: 5,
                score: 0.7,
                raw_score: 0.7,
                chunk_index: 0,
                total_chunks: 1,
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 0.0,
                tdg_grade: String::new(),
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
                raw_score: 1.0 - (i as f32 * 0.1),
                chunk_index: 0,
                total_chunks: 1,
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 0.0,
                tdg_grade: String::new(),
            })
            .collect();

        let deduped = deduplicate_chunks(results, 5);
        assert_eq!(deduped.len(), 5);
        // Should be sorted by score descending
        assert!(deduped[0].score >= deduped[1].score);
    }

    #[test]
    fn test_quality_weighted_dedup_reorders() {
        // Two symbols: one with high similarity but F grade, one with lower similarity but A grade.
        // After quality weighting, the A-grade symbol should rank higher.
        let results = vec![
            SearchResult {
                file_path: "src/bad.rs".to_string(),
                symbol_name: "bad_func".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn bad_func()".to_string(),
                start_line: 1,
                end_line: 5,
                score: 0.9 * 0.20, // quality_adjusted_score(0.9, "F") = 0.18
                raw_score: 0.9,
                chunk_index: 0,
                total_chunks: 1,
                cyclomatic_complexity: 20,
                cognitive_complexity: 15,
                tdg_score: 80.0,
                tdg_grade: "F".to_string(),
            },
            SearchResult {
                file_path: "src/good.rs".to_string(),
                symbol_name: "good_func".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn good_func()".to_string(),
                start_line: 1,
                end_line: 5,
                score: 0.7 * 1.0, // quality_adjusted_score(0.7, "A") = 0.7
                raw_score: 0.7,
                chunk_index: 0,
                total_chunks: 1,
                cyclomatic_complexity: 1,
                cognitive_complexity: 0,
                tdg_score: 10.0,
                tdg_grade: "A".to_string(),
            },
        ];

        let deduped = deduplicate_chunks(results, 10);
        assert_eq!(deduped.len(), 2);
        // A-grade symbol (score=0.7) should rank above F-grade (score=0.18)
        assert_eq!(deduped[0].symbol_name, "good_func");
        assert_eq!(deduped[1].symbol_name, "bad_func");
    }
}
