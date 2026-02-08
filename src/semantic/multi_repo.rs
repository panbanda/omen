//! Multi-repo search: combine symbols from multiple project indexes into a single
//! TF-IDF corpus so that IDF values reflect the full cross-repo vocabulary.

use std::collections::HashMap;
use std::path::Path;

use crate::core::Result;

use super::cache::EmbeddingCache;
use super::search::{deduplicate_chunks, SearchResult};
use super::tfidf::{DocMeta, TfidfEngine};

/// Result of a multi-repo search including the total indexed symbol count.
pub struct MultiRepoResult {
    pub results: Vec<SearchResult>,
    pub total_symbols: usize,
}

/// Search across multiple project indexes simultaneously.
///
/// Opens each project's cache, loads symbols, prepends a repo label to file
/// paths, then builds a single TF-IDF engine from the combined corpus.
pub fn multi_repo_search(
    projects: &[&Path],
    query: &str,
    top_k: usize,
    min_score: f32,
) -> Result<MultiRepoResult> {
    let mut docs: Vec<(String, DocMeta)> = Vec::new();

    // Track label occurrences to disambiguate same-named repos
    let mut label_counts: HashMap<String, usize> = HashMap::new();
    let mut labels: Vec<String> = Vec::new();
    for project_path in projects {
        let base = repo_label_from_path(project_path);
        let count = label_counts.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            labels.push(format!("{}-{}", base, count));
        } else {
            labels.push(base);
        }
    }

    for (project_path, repo_label) in projects.iter().zip(labels.iter()) {
        let cache_path = project_path.join(".omen").join("search.db");
        if !cache_path.exists() {
            continue;
        }

        let cache = EmbeddingCache::open(&cache_path)?;
        let symbols = cache.get_all_symbols()?;

        for sym in symbols {
            let prefixed_path = format!("[{}] {}", repo_label, sym.file_path);
            let meta = DocMeta {
                file_path: prefixed_path,
                symbol_name: sym.symbol_name,
                symbol_type: sym.symbol_type,
                signature: sym.signature,
                start_line: sym.start_line,
                end_line: sym.end_line,
                cyclomatic_complexity: sym.cyclomatic_complexity,
                cognitive_complexity: sym.cognitive_complexity,
            };
            docs.push((sym.enriched_text, meta));
        }
    }

    if docs.is_empty() {
        return Ok(MultiRepoResult {
            results: Vec::new(),
            total_symbols: 0,
        });
    }

    let total_symbols = docs.len();
    let engine = TfidfEngine::fit(&docs);
    let raw_results = engine.search(query, top_k.saturating_mul(3));
    let deduped = deduplicate_chunks(raw_results);

    let results = deduped
        .into_iter()
        .filter(|r| r.score >= min_score)
        .take(top_k)
        .collect();

    Ok(MultiRepoResult {
        results,
        total_symbols,
    })
}

/// Derive a short label from a project path (last path component).
fn repo_label_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_label_from_path() {
        assert_eq!(
            repo_label_from_path(Path::new("/home/user/projects/my-app")),
            "my-app"
        );
        assert_eq!(repo_label_from_path(Path::new(".")), "unknown");
        assert_eq!(repo_label_from_path(Path::new("/foo/bar/")), "bar");
        assert_eq!(repo_label_from_path(Path::new("/foo/bar")), "bar");
    }

    #[test]
    fn test_multi_repo_search_no_projects() {
        let result = multi_repo_search(&[], "test query", 10, 0.0).unwrap();
        assert!(result.results.is_empty());
        assert_eq!(result.total_symbols, 0);
    }

    #[test]
    fn test_multi_repo_search_missing_cache() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path();
        // No .omen/search.db exists
        let result = multi_repo_search(&[path], "test query", 10, 0.0).unwrap();
        assert!(result.results.is_empty());
    }

    #[test]
    fn test_multi_repo_search_combines_results() {
        use super::super::cache::CachedSymbol;

        let temp1 = tempfile::tempdir().unwrap();
        let temp2 = tempfile::tempdir().unwrap();

        // Create caches in each project
        for (temp, repo, sym_name, text) in [
            (
                &temp1,
                "repo-a",
                "parse_config",
                "[src/config.rs] parse_config\nfn parse_config() { toml::parse }",
            ),
            (
                &temp2,
                "repo-b",
                "parse_args",
                "[src/cli.rs] parse_args\nfn parse_args() { clap::parse }",
            ),
        ] {
            let omen_dir = temp.path().join(".omen");
            std::fs::create_dir_all(&omen_dir).unwrap();
            let cache = EmbeddingCache::open(&omen_dir.join("search.db")).unwrap();
            cache
                .upsert_symbol(&CachedSymbol {
                    file_path: format!("src/{repo}.rs"),
                    symbol_name: sym_name.to_string(),
                    symbol_type: "function".to_string(),
                    parent_name: None,
                    signature: format!("fn {sym_name}()"),
                    start_line: 1,
                    end_line: 5,
                    chunk_index: 0,
                    total_chunks: 1,
                    content_hash: "h".to_string(),
                    enriched_text: text.to_string(),
                    cyclomatic_complexity: None,
                    cognitive_complexity: None,
                })
                .unwrap();
        }

        let result = multi_repo_search(&[temp1.path(), temp2.path()], "parse", 10, 0.0).unwrap();

        assert_eq!(result.results.len(), 2);
        assert_eq!(result.total_symbols, 2);
        // Both repos should be represented
        let paths: Vec<&str> = result
            .results
            .iter()
            .map(|r| r.file_path.as_str())
            .collect();
        assert!(paths
            .iter()
            .any(|p| p.contains(temp1.path().file_name().unwrap().to_str().unwrap())));
        assert!(paths
            .iter()
            .any(|p| p.contains(temp2.path().file_name().unwrap().to_str().unwrap())));
    }

    #[test]
    fn test_duplicate_repo_labels_disambiguated() {
        use super::super::cache::CachedSymbol;

        let parent1 = tempfile::tempdir().unwrap();
        let parent2 = tempfile::tempdir().unwrap();

        // Both repos named "app"
        let repo1 = parent1.path().join("app");
        let repo2 = parent2.path().join("app");

        for (repo_path, sym_name, text) in [
            (&repo1, "func_a", "[src/a.rs] func_a\nfn func_a() { alpha }"),
            (&repo2, "func_b", "[src/b.rs] func_b\nfn func_b() { beta }"),
        ] {
            let omen_dir = repo_path.join(".omen");
            std::fs::create_dir_all(&omen_dir).unwrap();
            let cache = EmbeddingCache::open(&omen_dir.join("search.db")).unwrap();
            cache
                .upsert_symbol(&CachedSymbol {
                    file_path: "src/lib.rs".to_string(),
                    symbol_name: sym_name.to_string(),
                    symbol_type: "function".to_string(),
                    parent_name: None,
                    signature: format!("fn {sym_name}()"),
                    start_line: 1,
                    end_line: 5,
                    chunk_index: 0,
                    total_chunks: 1,
                    content_hash: "h".to_string(),
                    enriched_text: text.to_string(),
                    cyclomatic_complexity: None,
                    cognitive_complexity: None,
                })
                .unwrap();
        }

        let result =
            multi_repo_search(&[repo1.as_path(), repo2.as_path()], "func", 10, 0.0).unwrap();
        let paths: Vec<&str> = result
            .results
            .iter()
            .map(|r| r.file_path.as_str())
            .collect();
        // Second repo should get disambiguated label "app-2"
        assert!(paths.iter().any(|p| p.starts_with("[app]")));
        assert!(paths.iter().any(|p| p.starts_with("[app-2]")));
    }
}
