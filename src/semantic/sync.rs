//! Staleness detection and incremental indexing for semantic search.
//!
//! Detects changed files by comparing content hashes and re-indexes only what's needed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use blake3::Hasher;

use crate::core::{Error, FileSet, Result, SourceFile};
use crate::parser::{extract_functions, Parser};

use super::cache::{CachedSymbol, EmbeddingCache};
use super::embed::{format_symbol_text, EmbeddingEngine};

/// Sync manager for incremental indexing.
pub struct SyncManager<'a> {
    cache: &'a EmbeddingCache,
    engine: &'a EmbeddingEngine,
    parser: Parser,
}

impl<'a> SyncManager<'a> {
    /// Create a new sync manager.
    pub fn new(cache: &'a EmbeddingCache, engine: &'a EmbeddingEngine) -> Self {
        Self {
            cache,
            engine,
            parser: Parser::new(),
        }
    }

    /// Sync the index with the current file set.
    /// Returns the number of files that were re-indexed.
    pub fn sync(&self, file_set: &FileSet, root_path: &Path) -> Result<SyncStats> {
        let mut stats = SyncStats::default();

        // Get current files
        let current_files: HashSet<PathBuf> = file_set.files().iter().cloned().collect();

        // Get indexed files
        let indexed_files: HashSet<String> =
            self.cache.get_all_indexed_files()?.into_iter().collect();

        // Find files to remove (deleted or moved)
        for indexed_path in &indexed_files {
            let full_path = root_path.join(indexed_path);
            if !current_files.contains(&full_path) {
                self.cache.remove_file(indexed_path)?;
                stats.removed += 1;
            }
        }

        // Check each current file for changes
        let files_to_index: Vec<_> = current_files
            .iter()
            .filter(|path| {
                let rel_path = path
                    .strip_prefix(root_path)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();

                self.check_file_changed(path, &rel_path).unwrap_or(true)
            })
            .collect();

        stats.checked = current_files.len();

        // Index changed files (sequential for now due to SQLite constraints)
        for path in files_to_index {
            match self.index_file(path, root_path) {
                Ok(symbols) => {
                    stats.indexed += 1;
                    stats.symbols += symbols;
                }
                Err(e) => {
                    eprintln!("Warning: Failed to index {}: {}", path.display(), e);
                    stats.errors += 1;
                }
            }
        }

        Ok(stats)
    }

    /// Check if a file has changed since last indexing.
    fn check_file_changed(&self, path: &Path, rel_path: &str) -> Result<bool> {
        let current_hash = hash_file(path)?;

        match self.cache.get_file_hash(rel_path)? {
            Some(cached_hash) => Ok(cached_hash != current_hash),
            None => Ok(true), // Not indexed yet
        }
    }

    /// Index a single file.
    fn index_file(&self, path: &Path, root_path: &Path) -> Result<usize> {
        let rel_path = path
            .strip_prefix(root_path)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let file_hash = hash_file(path)?;

        // Parse the file
        let source_file = SourceFile::load(path)?;
        let parse_result = self.parser.parse_source(&source_file)?;

        // Extract functions
        let functions = extract_functions(&parse_result);

        // Delete existing symbols for this file
        self.cache.delete_file_symbols(&rel_path)?;

        let source_str = String::from_utf8_lossy(&source_file.content);

        // Generate embeddings and cache symbols
        let mut symbol_count = 0;
        for func in &functions {
            let text = format_symbol_text(func, &source_str);
            let content_hash = hash_string(&text);

            let embedding = self.engine.embed(&text)?;

            let cached_symbol = CachedSymbol {
                file_path: rel_path.clone(),
                symbol_name: func.name.clone(),
                symbol_type: "function".to_string(),
                signature: func.signature.clone(),
                start_line: func.start_line,
                end_line: func.end_line,
                content_hash,
                embedding,
            };

            self.cache.upsert_symbol(&cached_symbol)?;
            symbol_count += 1;
        }

        // Record file as indexed
        self.cache.record_file_indexed(&rel_path, &file_hash)?;

        Ok(symbol_count)
    }
}

/// Statistics from a sync operation.
#[derive(Debug, Default, Clone)]
pub struct SyncStats {
    /// Number of files checked.
    pub checked: usize,
    /// Number of files indexed (new or changed).
    pub indexed: usize,
    /// Number of files removed from index.
    pub removed: usize,
    /// Number of symbols indexed.
    pub symbols: usize,
    /// Number of errors encountered.
    pub errors: usize,
}

/// Hash a file's contents.
pub fn hash_file(path: &Path) -> Result<String> {
    let contents = std::fs::read(path)
        .map_err(|e| Error::analysis(format!("Failed to read {}: {}", path.display(), e)))?;
    Ok(hash_bytes(&contents))
}

/// Hash a byte slice.
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize().to_hex().to_string()
}

/// Hash a string.
pub fn hash_string(s: &str) -> String {
    hash_bytes(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_string_deterministic() {
        let hash1 = hash_string("hello world");
        let hash2 = hash_string("hello world");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_string_different_inputs() {
        let hash1 = hash_string("hello");
        let hash2 = hash_string("world");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_bytes() {
        let hash = hash_bytes(b"test data");
        assert!(!hash.is_empty());
        // Blake3 produces 64 hex characters
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_sync_stats_default() {
        let stats = SyncStats::default();
        assert_eq!(stats.checked, 0);
        assert_eq!(stats.indexed, 0);
        assert_eq!(stats.removed, 0);
        assert_eq!(stats.symbols, 0);
        assert_eq!(stats.errors, 0);
    }
}
