//! Multi-repository semantic search.
//!
//! Indexes and searches across multiple repositories from a shared LanceDB cache,
//! enabling cross-repo code discovery.

use std::path::PathBuf;

use crate::config::Config;
use crate::core::{Error, FileSet, Result};

use super::cache::EmbeddingCache;
use super::embed::EmbeddingEngine;
use super::metrics::quality_adjusted_score;
use super::search::{SearchOutput, SearchResult};
use super::sync::{SyncManager, SyncStats};
use super::SearchConfig;

/// Multi-repository semantic search.
///
/// Manages a shared LanceDB cache across multiple repositories, with repo-filtered
/// search and independent indexing per repo.
pub struct MultiRepoSearch {
    cache: EmbeddingCache,
    engine: EmbeddingEngine,
    repos: Vec<(String, PathBuf)>,
}

impl MultiRepoSearch {
    /// Create a new multi-repo search with a shared cache.
    ///
    /// The cache is stored at `config.cache_path` or `~/.omen/search.lance`.
    pub fn new(config: &SearchConfig) -> Result<Self> {
        let cache_path = config.cache_path.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".omen")
                .join("search.lance")
        });

        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::analysis(format!("Failed to create cache directory: {}", e)))?;
        }

        let cache = EmbeddingCache::open(&cache_path)?;
        let engine = EmbeddingEngine::with_config(&config.provider)?;

        Ok(Self {
            cache,
            engine,
            repos: Vec::new(),
        })
    }

    /// Register a repository for indexing and search.
    ///
    /// Returns an error if `repo_id` is already registered.
    pub fn add_repo(&mut self, repo_id: &str, root_path: PathBuf) -> Result<()> {
        if self.repos.iter().any(|(id, _)| id == repo_id) {
            return Err(Error::analysis(format!(
                "Repository '{}' is already registered",
                repo_id
            )));
        }
        self.repos.push((repo_id.to_string(), root_path));
        Ok(())
    }

    /// Index a single registered repository.
    pub fn index_repo(&self, repo_id: &str, file_config: &Config) -> Result<SyncStats> {
        let root_path = self
            .repos
            .iter()
            .find(|(id, _)| id == repo_id)
            .map(|(_, path)| path)
            .ok_or_else(|| {
                Error::analysis(format!("Repository '{}' is not registered", repo_id))
            })?;

        let file_set = FileSet::from_path(root_path, file_config)?;
        let sync = SyncManager::new(&self.cache, &self.engine, repo_id.to_string());
        sync.sync(&file_set, root_path)
    }

    /// Index all registered repositories.
    pub fn index_all(&self, file_config: &Config) -> Result<Vec<(String, SyncStats)>> {
        let mut results = Vec::with_capacity(self.repos.len());
        for (repo_id, _) in &self.repos {
            let stats = self.index_repo(repo_id, file_config)?;
            results.push((repo_id.clone(), stats));
        }
        Ok(results)
    }

    /// Search across repositories.
    ///
    /// If `repo_filter` is `Some`, only search within those repos.
    /// If `None`, search all indexed data.
    pub fn search(
        &self,
        query: &str,
        top_k: usize,
        repo_filter: Option<&[&str]>,
    ) -> Result<SearchOutput> {
        let query_embedding = self.engine.embed(query)?;
        let fetch_k = top_k * 3;

        let scored = match repo_filter {
            Some(ids) => self
                .cache
                .vector_search_in_repos(&query_embedding, ids, fetch_k)?,
            None => self.cache.vector_search(&query_embedding, fetch_k)?,
        };

        let results: Vec<SearchResult> = scored
            .into_iter()
            .map(|(sym, raw_score)| {
                let adjusted = quality_adjusted_score(raw_score, &sym.tdg_grade);
                SearchResult {
                    repo_id: sym.repo_id,
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

        // Deduplicate across (repo_id, file_path, symbol_name)
        let results = super::search::deduplicate_chunks(results, top_k);
        let total_symbols = self.cache.symbol_count()?;

        Ok(SearchOutput::new(query.to_string(), total_symbols, results))
    }

    /// List registered repositories.
    pub fn repos(&self) -> &[(String, PathBuf)] {
        &self.repos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::cache::{CachedSymbol, EmbeddingCache};
    use crate::semantic::search::SearchResult;

    const DIM: usize = 384;

    fn make_symbol(repo_id: &str, file: &str, name: &str, embedding: Vec<f32>) -> CachedSymbol {
        CachedSymbol {
            file_path: file.to_string(),
            symbol_name: name.to_string(),
            symbol_type: "function".to_string(),
            signature: format!("fn {}()", name),
            start_line: 1,
            end_line: 5,
            content_hash: format!("hash_{}_{}", repo_id, name),
            embedding,
            chunk_index: 0,
            total_chunks: 1,
            cyclomatic_complexity: 0,
            cognitive_complexity: 0,
            tdg_score: 0.0,
            tdg_grade: String::new(),
            repo_id: repo_id.to_string(),
        }
    }

    #[test]
    fn test_add_repo_rejects_duplicate() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let mut multi = MultiRepoSearch {
            cache,
            engine: EmbeddingEngine::noop(),
            repos: Vec::new(),
        };

        multi.add_repo("repo-a", PathBuf::from("/tmp/a")).unwrap();
        let err = multi.add_repo("repo-a", PathBuf::from("/tmp/a2"));
        assert!(err.is_err());
    }

    #[test]
    fn test_repos_listing() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let mut multi = MultiRepoSearch {
            cache,
            engine: EmbeddingEngine::noop(),
            repos: Vec::new(),
        };

        multi.add_repo("repo-a", PathBuf::from("/tmp/a")).unwrap();
        multi.add_repo("repo-b", PathBuf::from("/tmp/b")).unwrap();

        assert_eq!(multi.repos().len(), 2);
        assert_eq!(multi.repos()[0].0, "repo-a");
        assert_eq!(multi.repos()[1].0, "repo-b");
    }

    #[test]
    fn test_repo_id_cache_roundtrip() {
        let cache = EmbeddingCache::in_memory().unwrap();

        let mut emb = vec![0.0f32; DIM];
        emb[0] = 1.0;
        let sym = make_symbol("my-repo", "src/lib.rs", "my_func", emb);

        cache.upsert_symbol(&sym).unwrap();

        let all = cache.get_all_symbols().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].repo_id, "my-repo");
    }

    #[test]
    fn test_vector_search_in_repos_filter() {
        let cache = EmbeddingCache::in_memory().unwrap();

        let mut emb_a = vec![0.0f32; DIM];
        emb_a[0] = 1.0;
        let sym_a = make_symbol("repo-a", "src/a.rs", "func_a", emb_a);

        let mut emb_b = vec![0.0f32; DIM];
        emb_b[1] = 1.0;
        let sym_b = make_symbol("repo-b", "src/b.rs", "func_b", emb_b);

        cache.upsert_symbol(&sym_a).unwrap();
        cache.upsert_symbol(&sym_b).unwrap();

        // Search only repo-a
        let mut query = vec![0.0f32; DIM];
        query[0] = 1.0;

        let results = cache
            .vector_search_in_repos(&query, &["repo-a"], 10)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.repo_id, "repo-a");

        // Search both repos
        let results = cache
            .vector_search_in_repos(&query, &["repo-a", "repo-b"], 10)
            .unwrap();
        assert_eq!(results.len(), 2);

        // Search with empty filter returns all
        let results = cache.vector_search_in_repos(&query, &[], 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_dedup_across_repos() {
        // Same symbol name in different repos should NOT be deduped
        let results = vec![
            SearchResult {
                repo_id: "repo-a".to_string(),
                file_path: "src/lib.rs".to_string(),
                symbol_name: "handler".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn handler()".to_string(),
                start_line: 1,
                end_line: 10,
                score: 0.9,
                raw_score: 0.9,
                chunk_index: 0,
                total_chunks: 1,
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 0.0,
                tdg_grade: String::new(),
            },
            SearchResult {
                repo_id: "repo-b".to_string(),
                file_path: "src/lib.rs".to_string(),
                symbol_name: "handler".to_string(),
                symbol_type: "function".to_string(),
                signature: "fn handler()".to_string(),
                start_line: 1,
                end_line: 10,
                score: 0.8,
                raw_score: 0.8,
                chunk_index: 0,
                total_chunks: 1,
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 0.0,
                tdg_grade: String::new(),
            },
        ];

        let deduped = super::super::search::deduplicate_chunks(results, 10);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn test_file_tracking_with_repo_id() {
        let cache = EmbeddingCache::in_memory().unwrap();

        // Same file path in different repos should not collide
        cache
            .record_file_indexed("src/lib.rs", "hash_a", "repo-a")
            .unwrap();
        cache
            .record_file_indexed("src/lib.rs", "hash_b", "repo-b")
            .unwrap();

        let hash_a = cache.get_file_hash("src/lib.rs", "repo-a").unwrap();
        assert_eq!(hash_a, Some("hash_a".to_string()));

        let hash_b = cache.get_file_hash("src/lib.rs", "repo-b").unwrap();
        assert_eq!(hash_b, Some("hash_b".to_string()));

        // Remove from repo-a only
        cache.remove_file("src/lib.rs", "repo-a").unwrap();
        assert!(cache
            .get_file_hash("src/lib.rs", "repo-a")
            .unwrap()
            .is_none());
        assert!(cache
            .get_file_hash("src/lib.rs", "repo-b")
            .unwrap()
            .is_some());
    }
}
