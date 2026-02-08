//! Staleness detection and incremental indexing for semantic search.
//!
//! Detects changed files by comparing content hashes and re-indexes only what's needed.
//! Uses parallel file parsing for performance.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use blake3::Hasher;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rayon::prelude::*;

use crate::core::progress::is_tty;
use crate::core::{Error, FileSet, Result, SourceFile};
use crate::parser::{extract_functions, Parser};

use super::cache::{CachedSymbol, EmbeddingCache};
use super::chunking::{extract_chunks, format_chunk_text, Chunk};

/// Intermediate structure for a parsed chunk ready for caching.
#[derive(Clone)]
struct ParsedChunk {
    file_path: String,
    symbol_name: String,
    symbol_type: String,
    parent_name: Option<String>,
    signature: String,
    start_line: u32,
    end_line: u32,
    chunk_index: u32,
    total_chunks: u32,
    enriched_text: String,
    content_hash: String,
}

/// Result of parsing a single file.
struct ParsedFile {
    rel_path: String,
    file_hash: String,
    chunks: Vec<ParsedChunk>,
}

/// Sync manager for incremental indexing.
pub struct SyncManager<'a> {
    cache: &'a EmbeddingCache,
}

impl<'a> SyncManager<'a> {
    /// Create a new sync manager.
    pub fn new(cache: &'a EmbeddingCache) -> Self {
        Self { cache }
    }

    /// Sync the index with the current file set.
    /// Returns the number of files that were re-indexed.
    pub fn sync(&self, file_set: &FileSet, root_path: &Path) -> Result<SyncStats> {
        let mut stats = SyncStats::default();
        let show_progress = is_tty();

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
            .cloned()
            .collect();

        stats.checked = current_files.len();

        if files_to_index.is_empty() {
            return Ok(stats);
        }

        // Set up progress bars
        let multi = if show_progress {
            Some(MultiProgress::new())
        } else {
            None
        };

        let progress_style = ProgressStyle::default_bar()
            .template("{prefix:.bold} [{bar:30.cyan/blue}] {pos}/{len} {msg}")
            .expect("valid template")
            .progress_chars("#>-");

        // Phase 1: Parse all files in parallel and extract symbols
        let parse_bar = multi.as_ref().map(|m| {
            let bar = m.add(ProgressBar::new(files_to_index.len() as u64));
            bar.set_style(progress_style.clone());
            bar.set_prefix("Parsing");
            bar.set_message("files...");
            bar
        });

        let root = root_path.to_path_buf();
        let parse_counter = Arc::new(AtomicUsize::new(0));

        let parsed_files: Vec<_> = files_to_index
            .par_iter()
            .filter_map(|path| {
                let result = match parse_file(path, &root) {
                    Ok(parsed) => Some(parsed),
                    Err(e) => {
                        eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
                        None
                    }
                };

                let count = parse_counter.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref bar) = parse_bar {
                    bar.set_position(count as u64);
                }

                result
            })
            .collect();

        if let Some(bar) = parse_bar {
            bar.finish_with_message("done");
        }

        // Collect all chunks
        let mut all_chunks: Vec<ParsedChunk> = Vec::new();
        for parsed_file in &parsed_files {
            all_chunks.extend(parsed_file.chunks.iter().cloned());
        }

        // Delete existing symbols for all files being re-indexed (must happen
        // before the empty check so that removed functions don't persist).
        for parsed_file in &parsed_files {
            self.cache.delete_file_symbols(&parsed_file.rel_path)?;
        }

        if all_chunks.is_empty() {
            for parsed_file in &parsed_files {
                self.cache
                    .record_file_indexed(&parsed_file.rel_path, &parsed_file.file_hash)?;
                stats.indexed += 1;
            }
            return Ok(stats);
        }

        // Phase 2: Write all chunks to cache
        let write_bar = multi.as_ref().map(|m| {
            let bar = m.add(ProgressBar::new(all_chunks.len() as u64));
            bar.set_style(progress_style);
            bar.set_prefix("Writing");
            bar.set_message("to cache...");
            bar
        });

        for (i, chunk) in all_chunks.iter().enumerate() {
            let cached_symbol = CachedSymbol {
                file_path: chunk.file_path.clone(),
                symbol_name: chunk.symbol_name.clone(),
                symbol_type: chunk.symbol_type.clone(),
                parent_name: chunk.parent_name.clone(),
                signature: chunk.signature.clone(),
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                chunk_index: chunk.chunk_index,
                total_chunks: chunk.total_chunks,
                content_hash: chunk.content_hash.clone(),
                enriched_text: chunk.enriched_text.clone(),
            };
            self.cache.upsert_symbol(&cached_symbol)?;

            if let Some(ref bar) = write_bar {
                bar.set_position((i + 1) as u64);
            }
        }

        if let Some(bar) = write_bar {
            bar.finish_with_message("done");
        }

        // Record all files as indexed
        for parsed_file in &parsed_files {
            self.cache
                .record_file_indexed(&parsed_file.rel_path, &parsed_file.file_hash)?;
            stats.indexed += 1;
            stats.symbols += parsed_file.chunks.len();
        }

        stats.errors = files_to_index.len() - parsed_files.len();

        Ok(stats)
    }

    /// Check if a file has changed since last indexing.
    fn check_file_changed(&self, path: &Path, rel_path: &str) -> Result<bool> {
        let current_hash = hash_file(path)?;

        match self.cache.get_file_hash(rel_path)? {
            Some(cached_hash) => Ok(cached_hash != current_hash),
            None => Ok(true),
        }
    }
}

/// Parse a single file and extract chunks.
/// This is a free function to allow parallel execution with rayon.
fn parse_file(path: &Path, root_path: &Path) -> Result<ParsedFile> {
    let rel_path = path
        .strip_prefix(root_path)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let file_hash = hash_file(path)?;

    let source_file = SourceFile::load(path)?;
    let parser = Parser::new();
    let parse_result = parser.parse_source(&source_file)?;

    let functions = extract_functions(&parse_result);
    let chunks: Vec<Chunk> = extract_chunks(&parse_result, &functions, &rel_path);

    let parsed_chunks: Vec<ParsedChunk> = chunks
        .iter()
        .map(|chunk| {
            let enriched_text = format_chunk_text(chunk);
            let content_hash = hash_string(&enriched_text);
            ParsedChunk {
                file_path: chunk.file_path.clone(),
                symbol_name: chunk.symbol_name.clone(),
                symbol_type: chunk.symbol_type.clone(),
                parent_name: chunk.parent_name.clone(),
                signature: chunk.signature.clone(),
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                chunk_index: chunk.chunk_index,
                total_chunks: chunk.total_chunks,
                enriched_text,
                content_hash,
            }
        })
        .collect();

    Ok(ParsedFile {
        rel_path,
        file_hash,
        chunks: parsed_chunks,
    })
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
