//! SQLite cache for symbol metadata and enriched text.
//!
//! Stores symbols keyed by (file_path, symbol_name, content_hash) for efficient
//! retrieval and staleness detection.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::core::{Error, Result};

/// SQLite cache for semantic search symbols.
pub struct EmbeddingCache {
    conn: Connection,
}

const UPSERT_SYMBOL_SQL: &str = r#"
    INSERT INTO symbols (file_path, symbol_name, symbol_type, parent_name, signature, start_line, end_line, chunk_index, total_chunks, content_hash, enriched_text, cyclomatic_complexity, cognitive_complexity, definition_line, definition_ordinal)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
    ON CONFLICT(file_path, symbol_name, definition_line, definition_ordinal, chunk_index) DO UPDATE SET
        symbol_type = excluded.symbol_type,
        parent_name = excluded.parent_name,
        signature = excluded.signature,
        start_line = excluded.start_line,
        end_line = excluded.end_line,
        total_chunks = excluded.total_chunks,
        content_hash = excluded.content_hash,
        enriched_text = excluded.enriched_text,
        cyclomatic_complexity = excluded.cyclomatic_complexity,
        cognitive_complexity = excluded.cognitive_complexity,
        created_at = CURRENT_TIMESTAMP
"#;

const SCHEMA_SQL: &str = r#"
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
        cyclomatic_complexity INTEGER,
        cognitive_complexity INTEGER,
        definition_line INTEGER NOT NULL,
        definition_ordinal INTEGER NOT NULL DEFAULT 0,
        created_at TEXT DEFAULT CURRENT_TIMESTAMP,
        UNIQUE(file_path, symbol_name, definition_line, definition_ordinal, chunk_index)
    );

    CREATE INDEX IF NOT EXISTS idx_symbols_file_path ON symbols(file_path);
    CREATE INDEX IF NOT EXISTS idx_symbols_content_hash ON symbols(content_hash);

    CREATE TABLE IF NOT EXISTS files (
        file_path TEXT PRIMARY KEY,
        file_hash TEXT NOT NULL,
        indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
    );
"#;

const RECORD_FILE_SQL: &str = r#"
    INSERT INTO files (file_path, file_hash)
    VALUES (?1, ?2)
    ON CONFLICT(file_path) DO UPDATE SET
        file_hash = excluded.file_hash,
        indexed_at = CURRENT_TIMESTAMP
"#;

fn upsert_symbol_on(conn: &Connection, symbol: &CachedSymbol) -> Result<()> {
    conn.prepare_cached(UPSERT_SYMBOL_SQL)
        .and_then(|mut stmt| {
            stmt.execute(params![
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
                symbol.cyclomatic_complexity,
                symbol.cognitive_complexity,
                symbol.definition_line,
                symbol.definition_ordinal,
            ])
        })
        .map_err(|e| Error::analysis(format!("Failed to upsert symbol: {e}")))?;
    Ok(())
}

fn delete_file_symbols_on(conn: &Connection, file_path: &str) -> Result<()> {
    conn.prepare_cached("DELETE FROM symbols WHERE file_path = ?1")
        .and_then(|mut stmt| stmt.execute(params![file_path]))
        .map_err(|e| Error::analysis(format!("Failed to delete symbols: {e}")))?;
    Ok(())
}

fn record_file_indexed_on(conn: &Connection, file_path: &str, file_hash: &str) -> Result<()> {
    conn.prepare_cached(RECORD_FILE_SQL)
        .and_then(|mut stmt| stmt.execute(params![file_path, file_hash]))
        .map_err(|e| Error::analysis(format!("Failed to record file indexed: {e}")))?;
    Ok(())
}

pub(crate) struct CacheTransaction<'connection> {
    transaction: Transaction<'connection>,
}

impl CacheTransaction<'_> {
    pub(crate) fn upsert_symbol(&self, symbol: &CachedSymbol) -> Result<()> {
        upsert_symbol_on(&self.transaction, symbol)
    }

    pub(crate) fn delete_file_symbols(&self, file_path: &str) -> Result<()> {
        delete_file_symbols_on(&self.transaction, file_path)
    }

    pub(crate) fn record_file_indexed(&self, file_path: &str, file_hash: &str) -> Result<()> {
        record_file_indexed_on(&self.transaction, file_path, file_hash)
    }

    pub(crate) fn commit(self) -> Result<()> {
        self.transaction
            .commit()
            .map_err(|e| Error::analysis(format!("Failed to commit cache transaction: {e}")))
    }
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
    pub cyclomatic_complexity: Option<u32>,
    pub cognitive_complexity: Option<u32>,
    /// The first line of the definition. A chunk of a long definition has its own
    /// `start_line`, so this line is the identity of the definition.
    pub definition_line: u32,
    /// The position of the definition among the definitions of one name on one line.
    pub definition_ordinal: u32,
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
        cyclomatic_complexity: row.get(11)?,
        cognitive_complexity: row.get(12)?,
        definition_line: row.get(13)?,
        definition_ordinal: row.get(14)?,
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
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| Error::analysis(format!("Failed to configure cache database: {e}")))?;

        let cache = Self { conn };
        cache.migrate_if_needed()?;
        cache.init_schema()?;
        Ok(cache)
    }

    /// Create an in-memory cache (useful for testing).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::analysis(format!("Failed to create in-memory database: {}", e)))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| Error::analysis(format!("Failed to configure cache database: {e}")))?;

        let cache = Self { conn };
        cache.init_schema()?;
        Ok(cache)
    }

    /// Detect an old schema and migrate it.
    ///
    /// Omen drops the tables to force an index run when:
    /// - the `embedding` column exists (pre-TF-IDF schema)
    /// - the `chunk_index` column is missing (pre-chunking schema)
    /// - a column of the complexity is missing
    ///
    /// Omen keeps the symbols and adds the identity of a definition when
    /// `definition_line` or `definition_ordinal` is missing.
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

        if columns.is_empty() {
            return Ok(());
        }
        let has = |column: &str| columns.iter().any(|name| name == column);

        let needs_drop = has("embedding")
            || !has("chunk_index")
            || !has("cyclomatic_complexity")
            || !has("cognitive_complexity");
        if needs_drop {
            self.conn
                .execute_batch("DROP TABLE IF EXISTS symbols; DROP TABLE IF EXISTS files;")
                .map_err(|e| Error::analysis(format!("Failed to migrate old schema: {}", e)))?;
        } else if !has("definition_line") || !has("definition_ordinal") {
            self.add_definition_identity(has("definition_line"))?;
        }

        Ok(())
    }

    /// Copy the symbols of an old table into a table that has the identity
    /// of a definition. The copy keeps each symbol, so a query on the old
    /// cache still gives results. It deletes the file hashes, so the next
    /// index run parses each file again and finds the definitions that the old
    /// table lost.
    ///
    /// A chunk of an old table has no definition line. The `start_line` of the
    /// chunk with index 0 of the same symbol is the definition line.
    fn add_definition_identity(&self, has_definition_line: bool) -> Result<()> {
        let definition_line = if has_definition_line {
            "o.definition_line"
        } else {
            "COALESCE((SELECT first.start_line FROM symbols_old first \
             WHERE first.file_path = o.file_path AND first.symbol_name = o.symbol_name \
             AND first.chunk_index = 0), o.start_line)"
        };
        let copy = format!(
            "INSERT INTO symbols (file_path, symbol_name, symbol_type, parent_name, signature, \
             start_line, end_line, chunk_index, total_chunks, content_hash, enriched_text, \
             cyclomatic_complexity, cognitive_complexity, definition_line) \
             SELECT o.file_path, o.symbol_name, o.symbol_type, o.parent_name, o.signature, \
             o.start_line, o.end_line, o.chunk_index, o.total_chunks, o.content_hash, \
             o.enriched_text, o.cyclomatic_complexity, o.cognitive_complexity, {definition_line} \
             FROM symbols_old o ORDER BY o.id"
        );
        let fail =
            |e: rusqlite::Error| Error::analysis(format!("Failed to migrate old schema: {e}"));

        let transaction = self.conn.unchecked_transaction().map_err(fail)?;
        transaction
            .execute_batch(
                "ALTER TABLE symbols RENAME TO symbols_old; \
                 DROP INDEX IF EXISTS idx_symbols_file_path; \
                 DROP INDEX IF EXISTS idx_symbols_content_hash;",
            )
            .map_err(fail)?;
        transaction.execute_batch(SCHEMA_SQL).map_err(fail)?;
        transaction.execute_batch(&copy).map_err(fail)?;
        transaction
            .execute_batch("DROP TABLE symbols_old; DELETE FROM files;")
            .map_err(fail)?;
        transaction.commit().map_err(fail)
    }

    /// Initialize the database schema.
    fn init_schema(&self) -> Result<()> {
        self.conn
            .execute_batch(SCHEMA_SQL)
            .map_err(|e| Error::analysis(format!("Failed to initialize cache schema: {}", e)))?;
        Ok(())
    }

    /// Insert or update a symbol in the cache.
    pub fn upsert_symbol(&self, symbol: &CachedSymbol) -> Result<()> {
        upsert_symbol_on(&self.conn, symbol)
    }

    pub(crate) fn transaction(&self) -> Result<CacheTransaction<'_>> {
        let transaction = self
            .conn
            .unchecked_transaction()
            .map_err(|e| Error::analysis(format!("Failed to start cache transaction: {e}")))?;
        Ok(CacheTransaction { transaction })
    }

    /// Get a symbol from the cache by file path, symbol name, definition line, and chunk index.
    pub fn get_symbol(
        &self,
        file_path: &str,
        symbol_name: &str,
        definition_line: u32,
        chunk_index: u32,
    ) -> Result<Option<CachedSymbol>> {
        let result = self
            .conn
            .prepare_cached(
                "SELECT file_path, symbol_name, symbol_type, parent_name, signature, start_line, end_line, chunk_index, total_chunks, content_hash, enriched_text, cyclomatic_complexity, cognitive_complexity, definition_line, definition_ordinal FROM symbols WHERE file_path = ?1 AND symbol_name = ?2 AND definition_line = ?3 AND chunk_index = ?4",
            )
            .and_then(|mut stmt| {
                stmt.query_row(params![file_path, symbol_name, definition_line, chunk_index], row_to_symbol)
                    .optional()
            })
            .map_err(|e| Error::analysis(format!("Failed to get symbol: {}", e)))?;

        Ok(result)
    }

    /// Get all symbols from the cache.
    pub fn get_all_symbols(&self) -> Result<Vec<CachedSymbol>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT file_path, symbol_name, symbol_type, parent_name, signature, start_line, end_line, chunk_index, total_chunks, content_hash, enriched_text, cyclomatic_complexity, cognitive_complexity, definition_line, definition_ordinal FROM symbols ORDER BY file_path, definition_line, definition_ordinal, symbol_name, chunk_index")
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
            .prepare_cached("SELECT file_path, symbol_name, symbol_type, parent_name, signature, start_line, end_line, chunk_index, total_chunks, content_hash, enriched_text, cyclomatic_complexity, cognitive_complexity, definition_line, definition_ordinal FROM symbols WHERE file_path = ?1")
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
        delete_file_symbols_on(&self.conn, file_path)
    }

    /// Record that a file has been indexed.
    pub fn record_file_indexed(&self, file_path: &str, file_hash: &str) -> Result<()> {
        record_file_indexed_on(&self.conn, file_path, file_hash)
    }

    /// Get the stored hash for a file.
    pub fn get_file_hash(&self, file_path: &str) -> Result<Option<String>> {
        let result = self
            .conn
            .prepare_cached("SELECT file_hash FROM files WHERE file_path = ?1")
            .and_then(|mut stmt| {
                stmt.query_row(params![file_path], |row| row.get(0))
                    .optional()
            })
            .map_err(|e| Error::analysis(format!("Failed to get file hash: {}", e)))?;

        Ok(result)
    }

    /// Remove a file from the files table.
    pub fn remove_file(&self, file_path: &str) -> Result<()> {
        self.conn
            .prepare_cached("DELETE FROM files WHERE file_path = ?1")
            .and_then(|mut stmt| stmt.execute(params![file_path]))
            .map_err(|e| Error::analysis(format!("Failed to remove file: {}", e)))?;
        self.delete_file_symbols(file_path)?;
        Ok(())
    }

    /// Get all indexed file paths.
    pub fn get_all_indexed_files(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT file_path FROM files")
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
            .prepare_cached("SELECT COUNT(*) FROM symbols")
            .and_then(|mut stmt| stmt.query_row([], |row| row.get(0)))
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
            definition_line: 0,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "abc123".to_string(),
            enriched_text: "[src/main.rs] test_func\nfn test_func() { }".to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
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
            .get_symbol(&symbol.file_path, &symbol.symbol_name, 0, 0)
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
            .get_symbol(&symbol.file_path, &symbol.symbol_name, 0, 0)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.content_hash, "new_hash");
        assert_eq!(retrieved.enriched_text, "updated text");
        assert_eq!(cache.symbol_count().unwrap(), 1);
    }

    #[test]
    fn test_get_nonexistent_symbol() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let result = cache.get_symbol("nonexistent", "func", 0, 0).unwrap();
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
            definition_line: 0,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "hash1".to_string(),
            enriched_text: "[src/a.rs] func_a\nfn func_a() {}".to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
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
            definition_line: 0,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "hash2".to_string(),
            enriched_text: "[src/b.rs] func_b\nfn func_b() {}".to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
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
            definition_line: 0,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "hash1".to_string(),
            enriched_text: "text1".to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
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
            definition_line: 0,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "hash2".to_string(),
            enriched_text: "text2".to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
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
            definition_line: 0,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "hash3".to_string(),
            enriched_text: "text3".to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
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
            definition_line: 0,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "h".to_string(),
            enriched_text: "text".to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
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
            definition_line: 5,
            definition_ordinal: 0,
            total_chunks: 3,
            content_hash: "h".to_string(),
            enriched_text: "text".to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
        };
        cache.upsert_symbol(&symbol).unwrap();
        let retrieved = cache.get_symbol("test.rs", "bar", 5, 1).unwrap().unwrap();
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
                    definition_line: 0,
                    definition_ordinal: 0,
                    total_chunks: 3,
                    content_hash: format!("h{i}"),
                    enriched_text: format!("chunk {i}"),
                    cyclomatic_complexity: None,
                    cognitive_complexity: None,
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

    #[test]
    fn a_cache_without_definition_lines_is_rebuilt() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE symbols (
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
                cyclomatic_complexity INTEGER,
                cognitive_complexity INTEGER,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(file_path, symbol_name, chunk_index)
            );
            CREATE TABLE files (
                file_path TEXT PRIMARY KEY,
                file_hash TEXT NOT NULL,
                indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO files (file_path, file_hash) VALUES ('a.rs', 'old');
        "#,
        )
        .unwrap();

        let cache = EmbeddingCache { conn };
        cache.migrate_if_needed().unwrap();
        cache.init_schema().unwrap();

        assert!(cache.get_file_hash("a.rs").unwrap().is_none());
        let mut symbol = CachedSymbol {
            file_path: "a.rs".to_string(),
            symbol_name: "area".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn area()".to_string(),
            start_line: 1,
            end_line: 3,
            chunk_index: 0,
            definition_line: 1,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "h".to_string(),
            enriched_text: "text".to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
        };
        cache.upsert_symbol(&symbol).unwrap();
        symbol.definition_line = 9;
        cache.upsert_symbol(&symbol).unwrap();
        assert_eq!(cache.symbol_count().unwrap(), 2);
    }

    fn write_cache_without_definition_lines(path: &std::path::Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE symbols (
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
                cyclomatic_complexity INTEGER,
                cognitive_complexity INTEGER,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(file_path, symbol_name, chunk_index)
            );
            CREATE INDEX idx_symbols_file_path ON symbols(file_path);
            CREATE INDEX idx_symbols_content_hash ON symbols(content_hash);
            CREATE TABLE files (
                file_path TEXT PRIMARY KEY,
                file_hash TEXT NOT NULL,
                indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO symbols (file_path, symbol_name, symbol_type, signature, start_line, end_line, chunk_index, total_chunks, content_hash, enriched_text)
            VALUES ('a.rs', 'long_one', 'function', 'fn long_one()', 5, 8, 0, 2, 'h0', 'long one alpha'),
                   ('a.rs', 'long_one', 'function', 'fn long_one()', 9, 12, 1, 2, 'h1', 'long one beta'),
                   ('a.rs', 'short_one', 'function', 'fn short_one()', 20, 22, 0, 1, 'h2', 'short one gamma');
            INSERT INTO files (file_path, file_hash) VALUES ('a.rs', 'old');
        "#,
        )
        .unwrap();
    }

    #[test]
    fn an_old_cache_keeps_its_symbols_and_is_indexed_again() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("search.db");
        write_cache_without_definition_lines(&path);

        let cache = EmbeddingCache::open(&path).unwrap();

        let mut lines: Vec<(String, u32, u32)> = cache
            .get_all_symbols()
            .unwrap()
            .into_iter()
            .map(|symbol| {
                (
                    symbol.symbol_name,
                    symbol.chunk_index,
                    symbol.definition_line,
                )
            })
            .collect();
        lines.sort();
        assert_eq!(
            lines,
            vec![
                ("long_one".to_string(), 0, 5),
                ("long_one".to_string(), 1, 5),
                ("short_one".to_string(), 0, 20),
            ]
        );
        assert_eq!(cache.get_file_hash("a.rs").unwrap(), None);
    }

    #[test]
    fn a_query_on_an_old_cache_gives_results_without_an_index_run() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("search.db");
        write_cache_without_definition_lines(&path);
        let config = crate::semantic::SearchConfig {
            cache_path: Some(path),
            ..crate::semantic::SearchConfig::default()
        };

        let search = crate::semantic::SemanticSearch::new(&config, directory.path()).unwrap();
        let results = search.search("gamma", Some(10)).unwrap().results;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol_name, "short_one");
        assert_eq!(results[0].start_line, 20);
    }

    #[test]
    fn test_migration_from_pre_metric_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE symbols (
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

        let symbol = CachedSymbol {
            file_path: "test.rs".to_string(),
            symbol_name: "complex_fn".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn complex_fn()".to_string(),
            start_line: 1,
            end_line: 20,
            chunk_index: 0,
            definition_line: 0,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "h".to_string(),
            enriched_text: "text".to_string(),
            cyclomatic_complexity: Some(8),
            cognitive_complexity: Some(12),
        };
        cache.upsert_symbol(&symbol).unwrap();
        let retrieved = cache
            .get_symbol("test.rs", "complex_fn", 0, 0)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.cyclomatic_complexity, Some(8));
        assert_eq!(retrieved.cognitive_complexity, Some(12));
    }

    #[test]
    fn test_complexity_round_trip() {
        let cache = EmbeddingCache::in_memory().unwrap();

        let symbol = CachedSymbol {
            file_path: "src/lib.rs".to_string(),
            symbol_name: "complex".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn complex()".to_string(),
            start_line: 1,
            end_line: 50,
            chunk_index: 0,
            definition_line: 0,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "h".to_string(),
            enriched_text: "text".to_string(),
            cyclomatic_complexity: Some(15),
            cognitive_complexity: Some(20),
        };
        cache.upsert_symbol(&symbol).unwrap();

        let retrieved = cache
            .get_symbol("src/lib.rs", "complex", 0, 0)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.cyclomatic_complexity, Some(15));
        assert_eq!(retrieved.cognitive_complexity, Some(20));

        // None values should round-trip as None
        let simple = CachedSymbol {
            file_path: "src/lib.rs".to_string(),
            symbol_name: "simple".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn simple()".to_string(),
            start_line: 60,
            end_line: 65,
            chunk_index: 0,
            definition_line: 0,
            definition_ordinal: 0,
            total_chunks: 1,
            content_hash: "h2".to_string(),
            enriched_text: "text2".to_string(),
            cyclomatic_complexity: None,
            cognitive_complexity: None,
        };
        cache.upsert_symbol(&simple).unwrap();

        let retrieved = cache
            .get_symbol("src/lib.rs", "simple", 0, 0)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.cyclomatic_complexity, None);
        assert_eq!(retrieved.cognitive_complexity, None);
    }
}
