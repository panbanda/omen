//! Query engine for semantic search using TF-IDF.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::Result;

use super::cache::EmbeddingCache;
use super::tfidf::{DocMeta, TfidfEngine};

/// A search result with similarity score and optional quality metrics.
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
    /// Cyclomatic complexity (if computed during indexing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cyclomatic_complexity: Option<u32>,
    /// Cognitive complexity (if computed during indexing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cognitive_complexity: Option<u32>,
}

/// Search engine for semantic code search.
pub struct SearchEngine<'a> {
    cache: &'a EmbeddingCache,
}

impl<'a> SearchEngine<'a> {
    /// Create a new search engine.
    pub fn new(cache: &'a EmbeddingCache) -> Self {
        Self { cache }
    }

    /// Build a TF-IDF engine from cached symbols.
    fn build_tfidf(&self) -> Result<TfidfEngine> {
        let symbols = self.cache.get_all_symbols()?;
        let docs: Vec<_> = symbols
            .into_iter()
            .map(|sym| {
                let meta = DocMeta {
                    file_path: sym.file_path,
                    symbol_name: sym.symbol_name,
                    symbol_type: sym.symbol_type,
                    signature: sym.signature,
                    start_line: sym.start_line,
                    end_line: sym.end_line,
                    cyclomatic_complexity: sym.cyclomatic_complexity,
                    cognitive_complexity: sym.cognitive_complexity,
                };
                (sym.enriched_text, meta)
            })
            .collect();
        Ok(TfidfEngine::fit(&docs))
    }

    /// Search for symbols matching the query.
    ///
    /// When a symbol has multiple chunks, only the best-scoring chunk is
    /// returned so each symbol appears at most once in the results.
    pub fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchResult>> {
        let tfidf = self.build_tfidf()?;
        // Fetch extra results before dedup since chunks will be collapsed
        let raw_results = tfidf.search(query, top_k.saturating_mul(3));
        let deduped = deduplicate_chunks(raw_results);
        Ok(deduped.into_iter().take(top_k).collect())
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

    /// Search with combined filters: min score and max complexity.
    pub fn search_filtered(
        &self,
        query: &str,
        top_k: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<SearchResult>> {
        let results = self.search(query, top_k)?;
        Ok(results
            .into_iter()
            .filter(|r| r.score >= filters.min_score)
            .filter(|r| {
                filters
                    .max_complexity
                    .is_none_or(|max| r.cyclomatic_complexity.is_none_or(|c| c <= max))
            })
            .collect())
    }

    /// Search within specific files.
    pub fn search_in_files(
        &self,
        query: &str,
        file_paths: &[&str],
        top_k: usize,
    ) -> Result<Vec<SearchResult>> {
        let tfidf = self.build_tfidf()?;
        let raw_results = tfidf.search_in_files(query, file_paths, top_k.saturating_mul(3));
        let deduped = deduplicate_chunks(raw_results);
        Ok(deduped.into_iter().take(top_k).collect())
    }
}

fn to_search_result((meta, score): (DocMeta, f32)) -> SearchResult {
    SearchResult {
        file_path: meta.file_path,
        symbol_name: meta.symbol_name,
        symbol_type: meta.symbol_type,
        signature: meta.signature,
        start_line: meta.start_line,
        end_line: meta.end_line,
        score,
        cyclomatic_complexity: meta.cyclomatic_complexity,
        cognitive_complexity: meta.cognitive_complexity,
    }
}

/// Filters for search results.
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Minimum similarity score (0-1).
    pub min_score: f32,
    /// Maximum cyclomatic complexity. Symbols above this are excluded.
    pub max_complexity: Option<u32>,
}

/// Collapse multiple chunks of the same symbol into a single result,
/// keeping the highest score. The key is (file_path, symbol_name).
fn deduplicate_chunks(results: Vec<(DocMeta, f32)>) -> Vec<SearchResult> {
    let mut best: HashMap<(String, String), SearchResult> = HashMap::new();
    // Track insertion order to preserve ranking stability
    let mut order: Vec<(String, String)> = Vec::new();

    for (meta, score) in results {
        let key = (meta.file_path.clone(), meta.symbol_name.clone());
        match best.get(&key) {
            Some(existing) if existing.score >= score => {}
            _ => {
                if !best.contains_key(&key) {
                    order.push(key.clone());
                }
                best.insert(key, to_search_result((meta, score)));
            }
        }
    }

    order
        .into_iter()
        .filter_map(|key| best.remove(&key))
        .collect()
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
    use crate::semantic::cache::CachedSymbol;

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
            cyclomatic_complexity: Some(5),
            cognitive_complexity: Some(3),
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
                cyclomatic_complexity: None,
                cognitive_complexity: None,
            }],
        };

        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("test query"));
        assert!(json.contains("100"));
    }

    fn test_symbol(file_path: &str, name: &str, enriched_text: &str, hash: &str) -> CachedSymbol {
        CachedSymbol {
            file_path: file_path.to_string(),
            symbol_name: name.to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: format!("fn {name}()"),
            start_line: 1,
            end_line: 5,
            chunk_index: 0,
            total_chunks: 1,
            content_hash: hash.to_string(),
            enriched_text: enriched_text.to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
        }
    }

    #[test]
    fn test_search_engine_with_cache() {
        let cache = EmbeddingCache::in_memory().unwrap();

        cache
            .upsert_symbol(&test_symbol(
                "src/parser.rs",
                "parse_source_code",
                "[src/parser.rs] parse_source_code\nfn parse_source_code(source: &str) { tree_sitter::parse(source) }",
                "h1",
            ))
            .unwrap();

        cache
            .upsert_symbol(&test_symbol(
                "src/output.rs",
                "format_json",
                "[src/output.rs] format_json\nfn format_json(data: &Value) { serde_json::to_string(data) }",
                "h2",
            ))
            .unwrap();

        let engine = SearchEngine::new(&cache);
        let results = engine.search("parse source code", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].symbol_name, "parse_source_code");
    }

    #[test]
    fn test_search_with_threshold() {
        let cache = EmbeddingCache::in_memory().unwrap();

        cache
            .upsert_symbol(&test_symbol(
                "test.rs",
                "foo",
                "[test.rs] foo\nfn foo() {}",
                "h",
            ))
            .unwrap();

        let engine = SearchEngine::new(&cache);
        let results = engine
            .search_with_threshold("completely unrelated query about elephants", 10, 0.99)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_in_files() {
        let cache = EmbeddingCache::in_memory().unwrap();

        cache
            .upsert_symbol(&test_symbol(
                "src/a.rs",
                "parse_a",
                "[src/a.rs] parse_a\nfn parse_a() { parse }",
                "h1",
            ))
            .unwrap();

        cache
            .upsert_symbol(&test_symbol(
                "src/b.rs",
                "parse_b",
                "[src/b.rs] parse_b\nfn parse_b() { parse }",
                "h2",
            ))
            .unwrap();

        let engine = SearchEngine::new(&cache);
        let results = engine.search_in_files("parse", &["src/a.rs"], 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "src/a.rs");
    }

    #[test]
    fn test_deduplicate_chunks() {
        let cache = EmbeddingCache::in_memory().unwrap();

        // Insert two chunks of the same function with different content
        for i in 0..2 {
            cache
                .upsert_symbol(&CachedSymbol {
                    file_path: "src/lib.rs".to_string(),
                    symbol_name: "big_func".to_string(),
                    symbol_type: "function".to_string(),
                    parent_name: Some("MyStruct".to_string()),
                    signature: "fn big_func()".to_string(),
                    start_line: 1 + i * 10,
                    end_line: 10 + i * 10,
                    chunk_index: i,
                    total_chunks: 2,
                    content_hash: format!("h{i}"),
                    enriched_text: format!(
                        "[src/lib.rs] MyStruct::big_func ({}/2)\nfn big_func() {{ chunk {i} with big_func code }}",
                        i + 1
                    ),
                    cyclomatic_complexity: None,
                    cognitive_complexity: None,
                })
                .unwrap();
        }

        // Insert a different function
        cache
            .upsert_symbol(&test_symbol(
                "src/lib.rs",
                "other_func",
                "[src/lib.rs] other_func\nfn other_func() {}",
                "h3",
            ))
            .unwrap();

        let engine = SearchEngine::new(&cache);
        let results = engine.search("big_func", 10).unwrap();

        // big_func should appear once (best chunk), not twice
        let big_func_count = results
            .iter()
            .filter(|r| r.symbol_name == "big_func")
            .count();
        assert_eq!(big_func_count, 1);
    }

    #[test]
    fn test_search_filtered_by_complexity() {
        let cache = EmbeddingCache::in_memory().unwrap();

        // Low complexity function
        cache
            .upsert_symbol(&CachedSymbol {
                file_path: "src/a.rs".to_string(),
                symbol_name: "simple_parse".to_string(),
                symbol_type: "function".to_string(),
                parent_name: None,
                signature: "fn simple_parse()".to_string(),
                start_line: 1,
                end_line: 5,
                chunk_index: 0,
                total_chunks: 1,
                content_hash: "h1".to_string(),
                enriched_text: "[src/a.rs] simple_parse\nfn simple_parse() { parse }".to_string(),
                cyclomatic_complexity: Some(2),
                cognitive_complexity: Some(1),
            })
            .unwrap();

        // High complexity function
        cache
            .upsert_symbol(&CachedSymbol {
                file_path: "src/b.rs".to_string(),
                symbol_name: "complex_parse".to_string(),
                symbol_type: "function".to_string(),
                parent_name: None,
                signature: "fn complex_parse()".to_string(),
                start_line: 1,
                end_line: 100,
                chunk_index: 0,
                total_chunks: 1,
                content_hash: "h2".to_string(),
                enriched_text: "[src/b.rs] complex_parse\nfn complex_parse() { parse }".to_string(),
                cyclomatic_complexity: Some(25),
                cognitive_complexity: Some(30),
            })
            .unwrap();

        let engine = SearchEngine::new(&cache);

        // Without filter: both appear
        let all = engine.search("parse", 10).unwrap();
        assert_eq!(all.len(), 2);

        // With max_complexity=10: only simple_parse
        let filters = SearchFilters {
            min_score: 0.0,
            max_complexity: Some(10),
        };
        let filtered = engine.search_filtered("parse", 10, &filters).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].symbol_name, "simple_parse");
        assert_eq!(filtered[0].cyclomatic_complexity, Some(2));
    }
}
