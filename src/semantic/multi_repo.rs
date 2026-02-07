//! Multi-repo search: combine symbols from multiple project indexes into a single
//! TF-IDF corpus so that IDF values reflect the full cross-repo vocabulary.

use std::path::Path;

use crate::core::Result;

use super::cache::EmbeddingCache;
use super::search::{deduplicate_chunks, SearchResult};
use super::tfidf::{DocMeta, TfidfEngine};

/// Search across multiple project indexes simultaneously.
///
/// Opens each project's cache, loads symbols, prepends a repo label to file
/// paths, then builds a single TF-IDF engine from the combined corpus.
pub fn multi_repo_search(
    projects: &[&Path],
    query: &str,
    top_k: usize,
    min_score: f32,
) -> Result<Vec<SearchResult>> {
    let mut docs: Vec<(String, DocMeta)> = Vec::new();

    for project_path in projects {
        let cache_path = project_path.join(".omen").join("search.db");
        if !cache_path.exists() {
            continue;
        }

        let cache = EmbeddingCache::open(&cache_path)?;
        let symbols = cache.get_all_symbols()?;

        let repo_label = repo_label_from_path(project_path);

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
        return Ok(Vec::new());
    }

    let engine = TfidfEngine::fit(&docs);
    let raw_results = engine.search(query, top_k * 3);
    let deduped = deduplicate_chunks(raw_results);

    Ok(deduped
        .into_iter()
        .filter(|r| r.score >= min_score)
        .take(top_k)
        .collect())
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
        assert!(result.is_empty());
    }

    #[test]
    fn test_multi_repo_search_missing_cache() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path();
        // No .omen/search.db exists
        let result = multi_repo_search(&[path], "test query", 10, 0.0).unwrap();
        assert!(result.is_empty());
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

        let results = multi_repo_search(&[temp1.path(), temp2.path()], "parse", 10, 0.0).unwrap();

        assert_eq!(results.len(), 2);
        // Both repos should be represented
        let paths: Vec<&str> = results.iter().map(|r| r.file_path.as_str()).collect();
        assert!(paths
            .iter()
            .any(|p| p.contains(temp1.path().file_name().unwrap().to_str().unwrap())));
        assert!(paths
            .iter()
            .any(|p| p.contains(temp2.path().file_name().unwrap().to_str().unwrap())));
    }
}
