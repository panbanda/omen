//! SQLite cache for symbol metadata and enriched text.
//!
//! Stores symbols keyed by (file_path, symbol_name, content_hash) for efficient
//! retrieval and staleness detection.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::core::{Error, Result};

/// SQLite cache for semantic search symbols.
pub struct EmbeddingCache {
    conn: Connection,
}

/// A cached symbol with its enriched text for TF-IDF search.
#[derive(Debug, Clone)]
pub struct CachedSymbol {
    pub file_path: String,
    pub symbol_name: String,
    pub symbol_type: String,
    pub parent_name: Option<String>,
    pub signature: String,
    pub start_line: u32,
    pub end_line: u32,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub content_hash: String,
    pub enriched_text: String,
}

fn row_to_symbol(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedSymbol> {
    Ok(CachedSymbol {
        file_path: row.get(0)?,
        symbol_name: row.get(1)?,
        symbol_type: row.get(2)?,
        parent_name: row.get(3)?,
        signature: row.get(4)?,
        start_line: row.get(5)?,
        end_line: row.get(6)?,
        chunk_index: row.get(7)?,
        total_chunks: row.get(8)?,
        content_hash: row.get(9)?,
        enriched_text: row.get(10)?,
    })
}

impl EmbeddingCache {
    /// Open or create a cache at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| {
            Error::analysis(format!(
                "Failed to open cache database {}: {}",
                path.display(),
                e
            ))
        })?;

        let cache = Self { conn };
        cache.migrate_if_needed()?;
        cache.init_schema()?;
        Ok(cache)
    }

    /// Create an in-memory cache (useful for testing).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::analysis(format!("Failed to create in-memory database: {}", e)))?;

        let cache = Self { conn };
        cache.init_schema()?;
        Ok(cache)
    }

    /// Detect old schema and drop tables to force re-index.
    ///
    /// Triggers migration when:
    /// - `embedding` column exists (pre-TF-IDF schema)
    /// - `chunk_index` column is missing (pre-chunking schema)
    fn migrate_if_needed(&self) -> Result<()> {
        let columns: Vec<String> = self
            .conn
            .prepare("PRAGMA table_info(symbols)")
            .map(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(1))
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        let needs_migration = !columns.is_empty()
            && (columns.iter().any(|name| name == "embedding")
                || !columns.iter().any(|name| name == "chunk_index"));

        if needs_migration {
            self.conn
                .execute_batch("DROP TABLE IF EXISTS symbols; DROP TABLE IF EXISTS files;")
                .map_err(|e| Error::analysis(format!("Failed to migrate old schema: {}", e)))?;
        }

        Ok(())
    }

    /// Initialize the database schema.
    fn init_schema(&self) -> Result<()> {
        self.conn
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS symbols (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    file_path TEXT NOT NULL,
                    symbol_name TEXT NOT NULL,
                    symbol_type TEXT NOT NULL,
                    parent_name TEXT,
                    signature TEXT NOT NULL,
                    start_line INTEGER NOT NULL,
                    end_line INTEGER NOT NULL,
                    chunk_index INTEGER NOT NULL DEFAULT 0,
                    total_chunks INTEGER NOT NULL DEFAULT 1,
                    content_hash TEXT NOT NULL,
                    enriched_text TEXT NOT NULL,
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    UNIQUE(file_path, symbol_name, chunk_index)
                );

                CREATE INDEX IF NOT EXISTS idx_symbols_file_path ON symbols(file_path);
                CREATE INDEX IF NOT EXISTS idx_symbols_content_hash ON symbols(content_hash);

                CREATE TABLE IF NOT EXISTS files (
                    file_path TEXT PRIMARY KEY,
                    file_hash TEXT NOT NULL,
                    indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
                );
            "#,
            )
            .map_err(|e| Error::analysis(format!("Failed to initialize cache schema: {}", e)))?;
        Ok(())
    }

    /// Insert or update a symbol in the cache.
    pub fn upsert_symbol(&self, symbol: &CachedSymbol) -> Result<()> {
        self.conn
            .execute(
                r#"
                INSERT INTO symbols (file_path, symbol_name, symbol_type, parent_name, signature, start_line, end_line, chunk_index, total_chunks, content_hash, enriched_text)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT(file_path, symbol_name, chunk_index) DO UPDATE SET
                    symbol_type = excluded.symbol_type,
                    parent_name = excluded.parent_name,
                    signature = excluded.signature,
                    start_line = excluded.start_line,
                    end_line = excluded.end_line,
                    total_chunks = excluded.total_chunks,
                    content_hash = excluded.content_hash,
                    enriched_text = excluded.enriched_text,
                    created_at = CURRENT_TIMESTAMP
            "#,
                params![
                    symbol.file_path,
                    symbol.symbol_name,
                    symbol.symbol_type,
                    symbol.parent_name,
                    symbol.signature,
                    symbol.start_line,
                    symbol.end_line,
                    symbol.chunk_index,
                    symbol.total_chunks,
                    symbol.content_hash,
                    symbol.enriched_text,
                ],
            )
            .map_err(|e| Error::analysis(format!("Failed to upsert symbol: {}", e)))?;

        Ok(())
    }

    /// Get a symbol from the cache by file path, symbol name, and chunk index.
    pub fn get_symbol(
        &self,
        file_path: &str,
        symbol_name: &str,
        chunk_index: u32,
    ) -> Result<Option<CachedSymbol>> {
        let result = self
            .conn
            .query_row(
                "SELECT file_path, symbol_name, symbol_type, parent_name, signature, start_line, end_line, chunk_index, total_chunks, content_hash, enriched_text FROM symbols WHERE file_path = ?1 AND symbol_name = ?2 AND chunk_index = ?3",
                params![file_path, symbol_name, chunk_index],
                row_to_symbol,
            )
            .optional()
            .map_err(|e| Error::analysis(format!("Failed to get symbol: {}", e)))?;

        Ok(result)
    }

    /// Get all symbols from the cache.
    pub fn get_all_symbols(&self) -> Result<Vec<CachedSymbol>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path, symbol_name, symbol_type, parent_name, signature, start_line, end_line, chunk_index, total_chunks, content_hash, enriched_text FROM symbols")
            .map_err(|e| Error::analysis(format!("Failed to prepare query: {}", e)))?;

        let symbols = stmt
            .query_map([], row_to_symbol)
            .map_err(|e| Error::analysis(format!("Failed to query symbols: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(symbols)
    }

    /// Get all symbols for a specific file.
    pub fn get_symbols_for_file(&self, file_path: &str) -> Result<Vec<CachedSymbol>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path, symbol_name, symbol_type, parent_name, signature, start_line, end_line, chunk_index, total_chunks, content_hash, enriched_text FROM symbols WHERE file_path = ?1")
            .map_err(|e| Error::analysis(format!("Failed to prepare query: {}", e)))?;

        let symbols = stmt
            .query_map(params![file_path], row_to_symbol)
            .map_err(|e| Error::analysis(format!("Failed to query symbols: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(symbols)
    }

    /// Delete all symbols for a file.
    pub fn delete_file_symbols(&self, file_path: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM symbols WHERE file_path = ?1",
                params![file_path],
            )
            .map_err(|e| Error::analysis(format!("Failed to delete symbols: {}", e)))?;
        Ok(())
    }

    /// Record that a file has been indexed.
    pub fn record_file_indexed(&self, file_path: &str, file_hash: &str) -> Result<()> {
        self.conn
            .execute(
                r#"
                INSERT INTO files (file_path, file_hash)
                VALUES (?1, ?2)
                ON CONFLICT(file_path) DO UPDATE SET
                    file_hash = excluded.file_hash,
                    indexed_at = CURRENT_TIMESTAMP
            "#,
                params![file_path, file_hash],
            )
            .map_err(|e| Error::analysis(format!("Failed to record file indexed: {}", e)))?;
        Ok(())
    }

    /// Get the stored hash for a file.
    pub fn get_file_hash(&self, file_path: &str) -> Result<Option<String>> {
        let result = self
            .conn
            .query_row(
                "SELECT file_hash FROM files WHERE file_path = ?1",
                params![file_path],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| Error::analysis(format!("Failed to get file hash: {}", e)))?;

        Ok(result)
    }

    /// Remove a file from the files table.
    pub fn remove_file(&self, file_path: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM files WHERE file_path = ?1", params![file_path])
            .map_err(|e| Error::analysis(format!("Failed to remove file: {}", e)))?;
        self.delete_file_symbols(file_path)?;
        Ok(())
    }

    /// Get all indexed file paths.
    pub fn get_all_indexed_files(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path FROM files")
            .map_err(|e| Error::analysis(format!("Failed to prepare query: {}", e)))?;

        let files = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| Error::analysis(format!("Failed to query files: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(files)
    }

    /// Get the number of cached symbols.
    pub fn symbol_count(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
            .map_err(|e| Error::analysis(format!("Failed to count symbols: {}", e)))?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_symbol() -> CachedSymbol {
        CachedSymbol {
            file_path: "src/main.rs".to_string(),
            symbol_name: "test_func".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn test_func()".to_string(),
            start_line: 1,
            end_line: 5,
            chunk_index: 0,
            total_chunks: 1,
            content_hash: "abc123".to_string(),
            enriched_text: "[src/main.rs] test_func\nfn test_func() { }".to_string(),
        }
    }

    #[test]
    fn test_cache_creation() {
        let cache = EmbeddingCache::in_memory().unwrap();
        assert_eq!(cache.symbol_count().unwrap(), 0);
    }

    #[test]
    fn test_upsert_and_get_symbol() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let symbol = create_test_symbol();

        cache.upsert_symbol(&symbol).unwrap();

        let retrieved = cache
            .get_symbol(&symbol.file_path, &symbol.symbol_name, 0)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.file_path, symbol.file_path);
        assert_eq!(retrieved.symbol_name, symbol.symbol_name);
        assert_eq!(retrieved.enriched_text, symbol.enriched_text);
        assert_eq!(retrieved.chunk_index, 0);
        assert_eq!(retrieved.total_chunks, 1);
        assert!(retrieved.parent_name.is_none());
    }

    #[test]
    fn test_upsert_updates_existing() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let mut symbol = create_test_symbol();

        cache.upsert_symbol(&symbol).unwrap();

        symbol.content_hash = "new_hash".to_string();
        symbol.enriched_text = "updated text".to_string();
        cache.upsert_symbol(&symbol).unwrap();

        let retrieved = cache
            .get_symbol(&symbol.file_path, &symbol.symbol_name, 0)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.content_hash, "new_hash");
        assert_eq!(retrieved.enriched_text, "updated text");
        assert_eq!(cache.symbol_count().unwrap(), 1);
    }

    #[test]
    fn test_get_nonexistent_symbol() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let result = cache.get_symbol("nonexistent", "func", 0).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_all_symbols() {
        let cache = EmbeddingCache::in_memory().unwrap();

        let symbol1 = CachedSymbol {
            file_path: "src/a.rs".to_string(),
            symbol_name: "func_a".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn func_a()".to_string(),
            start_line: 1,
            end_line: 5,
            chunk_index: 0,
            total_chunks: 1,
            content_hash: "hash1".to_string(),
            enriched_text: "[src/a.rs] func_a\nfn func_a() {}".to_string(),
        };

        let symbol2 = CachedSymbol {
            file_path: "src/b.rs".to_string(),
            symbol_name: "func_b".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn func_b()".to_string(),
            start_line: 1,
            end_line: 5,
            chunk_index: 0,
            total_chunks: 1,
            content_hash: "hash2".to_string(),
            enriched_text: "[src/b.rs] func_b\nfn func_b() {}".to_string(),
        };

        cache.upsert_symbol(&symbol1).unwrap();
        cache.upsert_symbol(&symbol2).unwrap();

        let all_symbols = cache.get_all_symbols().unwrap();
        assert_eq!(all_symbols.len(), 2);
    }

    #[test]
    fn test_get_symbols_for_file() {
        let cache = EmbeddingCache::in_memory().unwrap();

        let symbol1 = CachedSymbol {
            file_path: "src/main.rs".to_string(),
            symbol_name: "func_a".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn func_a()".to_string(),
            start_line: 1,
            end_line: 5,
            chunk_index: 0,
            total_chunks: 1,
            content_hash: "hash1".to_string(),
            enriched_text: "text1".to_string(),
        };

        let symbol2 = CachedSymbol {
            file_path: "src/main.rs".to_string(),
            symbol_name: "func_b".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn func_b()".to_string(),
            start_line: 10,
            end_line: 15,
            chunk_index: 0,
            total_chunks: 1,
            content_hash: "hash2".to_string(),
            enriched_text: "text2".to_string(),
        };

        let symbol3 = CachedSymbol {
            file_path: "src/other.rs".to_string(),
            symbol_name: "func_c".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn func_c()".to_string(),
            start_line: 1,
            end_line: 5,
            chunk_index: 0,
            total_chunks: 1,
            content_hash: "hash3".to_string(),
            enriched_text: "text3".to_string(),
        };

        cache.upsert_symbol(&symbol1).unwrap();
        cache.upsert_symbol(&symbol2).unwrap();
        cache.upsert_symbol(&symbol3).unwrap();

        let main_symbols = cache.get_symbols_for_file("src/main.rs").unwrap();
        assert_eq!(main_symbols.len(), 2);
    }

    #[test]
    fn test_delete_file_symbols() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let symbol = create_test_symbol();

        cache.upsert_symbol(&symbol).unwrap();
        assert_eq!(cache.symbol_count().unwrap(), 1);

        cache.delete_file_symbols(&symbol.file_path).unwrap();
        assert_eq!(cache.symbol_count().unwrap(), 0);
    }

    #[test]
    fn test_file_hash_tracking() {
        let cache = EmbeddingCache::in_memory().unwrap();

        cache.record_file_indexed("src/main.rs", "hash123").unwrap();

        let hash = cache.get_file_hash("src/main.rs").unwrap();
        assert_eq!(hash, Some("hash123".to_string()));

        let missing = cache.get_file_hash("nonexistent.rs").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_remove_file() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let symbol = create_test_symbol();

        cache.upsert_symbol(&symbol).unwrap();
        cache
            .record_file_indexed(&symbol.file_path, "hash123")
            .unwrap();

        cache.remove_file(&symbol.file_path).unwrap();

        assert_eq!(cache.symbol_count().unwrap(), 0);
        assert!(cache.get_file_hash(&symbol.file_path).unwrap().is_none());
    }

    #[test]
    fn test_get_all_indexed_files() {
        let cache = EmbeddingCache::in_memory().unwrap();

        cache.record_file_indexed("src/a.rs", "hash1").unwrap();
        cache.record_file_indexed("src/b.rs", "hash2").unwrap();

        let files = cache.get_all_indexed_files().unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"src/a.rs".to_string()));
        assert!(files.contains(&"src/b.rs".to_string()));
    }

    #[test]
    fn test_migration_from_old_schema() {
        // Simulate old schema with embedding column
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE symbols (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                symbol_name TEXT NOT NULL,
                symbol_type TEXT NOT NULL,
                signature TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content_hash TEXT NOT NULL,
                embedding BLOB NOT NULL,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(file_path, symbol_name)
            );
            CREATE TABLE files (
                file_path TEXT PRIMARY KEY,
                file_hash TEXT NOT NULL,
                indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
        "#,
        )
        .unwrap();

        // Wrap in EmbeddingCache -- migration should drop and recreate
        let cache = EmbeddingCache { conn };
        cache.migrate_if_needed().unwrap();
        cache.init_schema().unwrap();

        // Should work with new schema
        let symbol = CachedSymbol {
            file_path: "test.rs".to_string(),
            symbol_name: "foo".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn foo()".to_string(),
            start_line: 1,
            end_line: 3,
            chunk_index: 0,
            total_chunks: 1,
            content_hash: "h".to_string(),
            enriched_text: "text".to_string(),
        };
        cache.upsert_symbol(&symbol).unwrap();
        assert_eq!(cache.symbol_count().unwrap(), 1);
    }

    #[test]
    fn test_migration_from_pre_chunking_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE symbols (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                symbol_name TEXT NOT NULL,
                symbol_type TEXT NOT NULL,
                signature TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content_hash TEXT NOT NULL,
                enriched_text TEXT NOT NULL,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(file_path, symbol_name)
            );
            CREATE TABLE files (
                file_path TEXT PRIMARY KEY,
                file_hash TEXT NOT NULL,
                indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
        "#,
        )
        .unwrap();

        let cache = EmbeddingCache { conn };
        cache.migrate_if_needed().unwrap();
        cache.init_schema().unwrap();

        // Should accept chunk fields
        let symbol = CachedSymbol {
            file_path: "test.rs".to_string(),
            symbol_name: "bar".to_string(),
            symbol_type: "function".to_string(),
            parent_name: Some("Foo".to_string()),
            signature: "fn bar()".to_string(),
            start_line: 5,
            end_line: 10,
            chunk_index: 1,
            total_chunks: 3,
            content_hash: "h".to_string(),
            enriched_text: "text".to_string(),
        };
        cache.upsert_symbol(&symbol).unwrap();
        let retrieved = cache.get_symbol("test.rs", "bar", 1).unwrap().unwrap();
        assert_eq!(retrieved.parent_name.as_deref(), Some("Foo"));
        assert_eq!(retrieved.chunk_index, 1);
        assert_eq!(retrieved.total_chunks, 3);
    }

    #[test]
    fn test_multiple_chunks_same_symbol() {
        let cache = EmbeddingCache::in_memory().unwrap();

        for i in 0..3 {
            cache
                .upsert_symbol(&CachedSymbol {
                    file_path: "src/lib.rs".to_string(),
                    symbol_name: "big_func".to_string(),
                    symbol_type: "function".to_string(),
                    parent_name: Some("MyStruct".to_string()),
                    signature: "fn big_func()".to_string(),
                    start_line: 10 + i * 20,
                    end_line: 10 + (i + 1) * 20,
                    chunk_index: i,
                    total_chunks: 3,
                    content_hash: format!("h{i}"),
                    enriched_text: format!("chunk {i}"),
                })
                .unwrap();
        }

        assert_eq!(cache.symbol_count().unwrap(), 3);

        let syms = cache.get_symbols_for_file("src/lib.rs").unwrap();
        assert_eq!(syms.len(), 3);

        // Delete all for the file
        cache.delete_file_symbols("src/lib.rs").unwrap();
        assert_eq!(cache.symbol_count().unwrap(), 0);
    }
}
