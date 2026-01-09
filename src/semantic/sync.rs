//! Staleness detection and incremental indexing for semantic search.
//!
//! Detects changed files by comparing content hashes and re-indexes only what's needed.
//! Uses parallel file parsing and batch embedding for performance.

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
use super::embed::{format_symbol_text, EmbeddingEngine};

/// Maximum batch size for embedding generation.
const EMBEDDING_BATCH_SIZE: usize = 64;

/// Intermediate structure for a parsed symbol before embedding.
#[derive(Clone)]
struct ParsedSymbol {
    file_path: String,
    symbol_name: String,
    signature: String,
    start_line: u32,
    end_line: u32,
    text: String,
    content_hash: String,
}

/// Result of parsing a single file.
struct ParsedFile {
    rel_path: String,
    file_hash: String,
    symbols: Vec<ParsedSymbol>,
}

/// Sync manager for incremental indexing.
pub struct SyncManager<'a> {
    cache: &'a EmbeddingCache,
    engine: &'a EmbeddingEngine,
}

impl<'a> SyncManager<'a> {
    /// Create a new sync manager.
    pub fn new(cache: &'a EmbeddingCache, engine: &'a EmbeddingEngine) -> Self {
        Self { cache, engine }
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

        // Set up multi-progress for the three phases
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

                // Update progress
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

        // Collect all symbols and their texts for batch embedding
        let mut all_symbols: Vec<ParsedSymbol> = Vec::new();
        for parsed_file in &parsed_files {
            all_symbols.extend(parsed_file.symbols.iter().cloned());
        }

        if all_symbols.is_empty() {
            // No symbols to embed, but still record files as indexed
            for parsed_file in &parsed_files {
                self.cache
                    .record_file_indexed(&parsed_file.rel_path, &parsed_file.file_hash)?;
                stats.indexed += 1;
            }
            return Ok(stats);
        }

        // Phase 2: Batch embed all symbols
        let embed_bar = multi.as_ref().map(|m| {
            let num_batches = all_symbols.len().div_ceil(EMBEDDING_BATCH_SIZE);
            let bar = m.add(ProgressBar::new(num_batches as u64));
            bar.set_style(progress_style.clone());
            bar.set_prefix("Embedding");
            bar.set_message(format!("{} symbols...", all_symbols.len()));
            bar
        });

        let texts: Vec<String> = all_symbols.iter().map(|s| s.text.clone()).collect();
        let embeddings = self.embed_in_batches_with_progress(&texts, embed_bar.as_ref())?;

        if let Some(bar) = embed_bar {
            bar.finish_with_message("done");
        }

        // Phase 3: Write all symbols to cache
        let write_bar = multi.as_ref().map(|m| {
            let bar = m.add(ProgressBar::new(all_symbols.len() as u64));
            bar.set_style(progress_style);
            bar.set_prefix("Writing");
            bar.set_message("to cache...");
            bar
        });

        // First, delete existing symbols for all files being re-indexed
        for parsed_file in &parsed_files {
            self.cache.delete_file_symbols(&parsed_file.rel_path)?;
        }

        // Insert all symbols with their embeddings
        for (i, (symbol, embedding)) in all_symbols.iter().zip(embeddings.into_iter()).enumerate() {
            let cached_symbol = CachedSymbol {
                file_path: symbol.file_path.clone(),
                symbol_name: symbol.symbol_name.clone(),
                symbol_type: "function".to_string(),
                signature: symbol.signature.clone(),
                start_line: symbol.start_line,
                end_line: symbol.end_line,
                content_hash: symbol.content_hash.clone(),
                embedding,
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
            stats.symbols += parsed_file.symbols.len();
        }

        stats.errors = files_to_index.len() - parsed_files.len();

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

    /// Embed texts in batches with optional progress bar.
    fn embed_in_batches_with_progress(
        &self,
        texts: &[String],
        progress: Option<&ProgressBar>,
    ) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_embeddings = Vec::with_capacity(texts.len());

        for (i, chunk) in texts.chunks(EMBEDDING_BATCH_SIZE).enumerate() {
            let batch_embeddings = self.engine.embed_batch(chunk)?;
            all_embeddings.extend(batch_embeddings);

            if let Some(bar) = progress {
                bar.set_position((i + 1) as u64);
            }
        }

        Ok(all_embeddings)
    }
}

/// Parse a single file and extract symbols (no embedding yet).
/// This is a free function to allow parallel execution with rayon.
fn parse_file(path: &Path, root_path: &Path) -> Result<ParsedFile> {
    let rel_path = path
        .strip_prefix(root_path)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let file_hash = hash_file(path)?;

    // Parse the file
    let source_file = SourceFile::load(path)?;
    let parser = Parser::new();
    let parse_result = parser.parse_source(&source_file)?;

    // Extract functions
    let functions = extract_functions(&parse_result);
    let source_str = String::from_utf8_lossy(&source_file.content);

    // Create parsed symbols
    let symbols: Vec<ParsedSymbol> = functions
        .iter()
        .map(|func| {
            let text = format_symbol_text(func, &source_str);
            let content_hash = hash_string(&text);
            ParsedSymbol {
                file_path: rel_path.clone(),
                symbol_name: func.name.clone(),
                signature: func.signature.clone(),
                start_line: func.start_line,
                end_line: func.end_line,
                text,
                content_hash,
            }
        })
        .collect();

    Ok(ParsedFile {
        rel_path,
        file_hash,
        symbols,
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
