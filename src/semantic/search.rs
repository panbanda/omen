//! Query engine for semantic search using TF-IDF.

use serde::{Deserialize, Serialize};

use crate::core::Result;

use super::cache::EmbeddingCache;
use super::tfidf::{DocMeta, TfidfEngine};

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
                };
                (sym.enriched_text, meta)
            })
            .collect();
        Ok(TfidfEngine::fit(&docs))
    }

    /// Search for symbols matching the query.
    pub fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchResult>> {
        let tfidf = self.build_tfidf()?;
        let results = tfidf.search(query, top_k);
        Ok(results.into_iter().map(to_search_result).collect())
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
        let tfidf = self.build_tfidf()?;
        let results = tfidf.search_in_files(query, file_paths, top_k);
        Ok(results.into_iter().map(to_search_result).collect())
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

    #[test]
    fn test_search_engine_with_cache() {
        let cache = EmbeddingCache::in_memory().unwrap();

        cache
            .upsert_symbol(&CachedSymbol {
                file_path: "src/parser.rs".to_string(),
                symbol_name: "parse_source_code".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn parse_source_code()".to_string(),
                start_line: 1,
                end_line: 10,
                content_hash: "h1".to_string(),
                enriched_text: "[src/parser.rs] parse_source_code\nfn parse_source_code(source: &str) { tree_sitter::parse(source) }".to_string(),
            })
            .unwrap();

        cache
            .upsert_symbol(&CachedSymbol {
                file_path: "src/output.rs".to_string(),
                symbol_name: "format_json".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn format_json()".to_string(),
                start_line: 1,
                end_line: 5,
                content_hash: "h2".to_string(),
                enriched_text: "[src/output.rs] format_json\nfn format_json(data: &Value) { serde_json::to_string(data) }".to_string(),
            })
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
            .upsert_symbol(&CachedSymbol {
                file_path: "test.rs".to_string(),
                symbol_name: "foo".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn foo()".to_string(),
                start_line: 1,
                end_line: 3,
                content_hash: "h".to_string(),
                enriched_text: "[test.rs] foo\nfn foo() {}".to_string(),
            })
            .unwrap();

        let engine = SearchEngine::new(&cache);
        // Very high threshold should filter out most results
        let results = engine
            .search_with_threshold("completely unrelated query about elephants", 10, 0.99)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_in_files() {
        let cache = EmbeddingCache::in_memory().unwrap();

        cache
            .upsert_symbol(&CachedSymbol {
                file_path: "src/a.rs".to_string(),
                symbol_name: "parse_a".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn parse_a()".to_string(),
                start_line: 1,
                end_line: 5,
                content_hash: "h1".to_string(),
                enriched_text: "[src/a.rs] parse_a\nfn parse_a() { parse }".to_string(),
            })
            .unwrap();

        cache
            .upsert_symbol(&CachedSymbol {
                file_path: "src/b.rs".to_string(),
                symbol_name: "parse_b".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn parse_b()".to_string(),
                start_line: 1,
                end_line: 5,
                content_hash: "h2".to_string(),
                enriched_text: "[src/b.rs] parse_b\nfn parse_b() { parse }".to_string(),
            })
            .unwrap();

        let engine = SearchEngine::new(&cache);
        let results = engine.search_in_files("parse", &["src/a.rs"], 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "src/a.rs");
    }
}
