//! LanceDB cache for embedding vectors and symbol metadata.
//!
//! Stores embeddings in a LanceDB table with Arrow-based schema for native vector
//! search support. Replaces the previous SQLite-based cache.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arrow_array::{
    ArrayRef, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator, StringArray,
    UInt32Array,
};
use arrow_schema::{DataType, Field, FieldRef, Schema};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use tokio::runtime::Runtime;

use crate::core::{Error, Result};

/// Embedding dimension (384 for all-MiniLM-L6-v2 / BGE-small-en-v1.5).
const EMBEDDING_DIM: i32 = 384;

/// Table name for symbols.
const SYMBOLS_TABLE: &str = "symbols";

/// Table name for file tracking.
const FILES_TABLE: &str = "files";

/// LanceDB cache for embeddings.
pub struct EmbeddingCache {
    db: lancedb::Connection,
    rt: Arc<Runtime>,
}

/// A cached symbol with its embedding and quality metrics.
#[derive(Debug, Clone)]
pub struct CachedSymbol {
    pub file_path: String,
    pub symbol_name: String,
    pub symbol_type: String,
    pub signature: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content_hash: String,
    pub embedding: Vec<f32>,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub cyclomatic_complexity: u32,
    pub cognitive_complexity: u32,
    pub tdg_score: f32,
    pub tdg_grade: String,
}

fn symbols_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("file_path", DataType::Utf8, false),
        Field::new("symbol_name", DataType::Utf8, false),
        Field::new("symbol_type", DataType::Utf8, false),
        Field::new("signature", DataType::Utf8, false),
        Field::new("start_line", DataType::UInt32, false),
        Field::new("end_line", DataType::UInt32, false),
        Field::new("content_hash", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                EMBEDDING_DIM,
            ),
            false,
        ),
        Field::new("chunk_index", DataType::UInt32, false),
        Field::new("total_chunks", DataType::UInt32, false),
        Field::new("cyclomatic_complexity", DataType::UInt32, false),
        Field::new("cognitive_complexity", DataType::UInt32, false),
        Field::new("tdg_score", DataType::Float32, false),
        Field::new("tdg_grade", DataType::Utf8, false),
    ]))
}

fn files_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("file_path", DataType::Utf8, false),
        Field::new("file_hash", DataType::Utf8, false),
    ]))
}

fn build_embedding_array(embeddings: &[Vec<f32>]) -> FixedSizeListArray {
    let flat: Vec<f32> = embeddings.iter().flat_map(|e| e.iter().copied()).collect();
    let values: ArrayRef = Arc::new(Float32Array::from(flat));
    let field: FieldRef = Arc::new(Field::new("item", DataType::Float32, true));
    FixedSizeListArray::new(field, EMBEDDING_DIM, values, None)
}

fn symbols_to_batch(
    symbols: &[CachedSymbol],
) -> std::result::Result<RecordBatch, arrow_schema::ArrowError> {
    let schema = symbols_schema();
    let file_paths: Vec<&str> = symbols.iter().map(|s| s.file_path.as_str()).collect();
    let symbol_names: Vec<&str> = symbols.iter().map(|s| s.symbol_name.as_str()).collect();
    let symbol_types: Vec<&str> = symbols.iter().map(|s| s.symbol_type.as_str()).collect();
    let signatures: Vec<&str> = symbols.iter().map(|s| s.signature.as_str()).collect();
    let start_lines: Vec<u32> = symbols.iter().map(|s| s.start_line).collect();
    let end_lines: Vec<u32> = symbols.iter().map(|s| s.end_line).collect();
    let content_hashes: Vec<&str> = symbols.iter().map(|s| s.content_hash.as_str()).collect();
    let embeddings: Vec<Vec<f32>> = symbols.iter().map(|s| s.embedding.clone()).collect();
    let chunk_indices: Vec<u32> = symbols.iter().map(|s| s.chunk_index).collect();
    let total_chunks: Vec<u32> = symbols.iter().map(|s| s.total_chunks).collect();
    let cyclomatic: Vec<u32> = symbols.iter().map(|s| s.cyclomatic_complexity).collect();
    let cognitive: Vec<u32> = symbols.iter().map(|s| s.cognitive_complexity).collect();
    let tdg_scores: Vec<f32> = symbols.iter().map(|s| s.tdg_score).collect();
    let tdg_grades: Vec<&str> = symbols.iter().map(|s| s.tdg_grade.as_str()).collect();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(file_paths)),
            Arc::new(StringArray::from(symbol_names)),
            Arc::new(StringArray::from(symbol_types)),
            Arc::new(StringArray::from(signatures)),
            Arc::new(UInt32Array::from(start_lines)),
            Arc::new(UInt32Array::from(end_lines)),
            Arc::new(StringArray::from(content_hashes)),
            Arc::new(build_embedding_array(&embeddings)),
            Arc::new(UInt32Array::from(chunk_indices)),
            Arc::new(UInt32Array::from(total_chunks)),
            Arc::new(UInt32Array::from(cyclomatic)),
            Arc::new(UInt32Array::from(cognitive)),
            Arc::new(Float32Array::from(tdg_scores)),
            Arc::new(StringArray::from(tdg_grades)),
        ],
    )
}

fn batch_to_symbols(batch: &RecordBatch) -> Vec<CachedSymbol> {
    let file_paths = batch
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("column 0 is StringArray");
    let symbol_names = batch
        .column(1)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("column 1 is StringArray");
    let symbol_types = batch
        .column(2)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("column 2 is StringArray");
    let signatures = batch
        .column(3)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("column 3 is StringArray");
    let start_lines = batch
        .column(4)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .expect("column 4 is UInt32Array");
    let end_lines = batch
        .column(5)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .expect("column 5 is UInt32Array");
    let content_hashes = batch
        .column(6)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("column 6 is StringArray");
    let embedding_list = batch
        .column(7)
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .expect("column 7 is FixedSizeListArray");
    let chunk_indices = batch
        .column(8)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .expect("column 8 is UInt32Array");
    let total_chunks_col = batch
        .column(9)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .expect("column 9 is UInt32Array");
    let cyclomatic_col = batch
        .column(10)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .expect("column 10 is UInt32Array");
    let cognitive_col = batch
        .column(11)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .expect("column 11 is UInt32Array");
    let tdg_score_col = batch
        .column(12)
        .as_any()
        .downcast_ref::<Float32Array>()
        .expect("column 12 is Float32Array");
    let tdg_grade_col = batch
        .column(13)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("column 13 is StringArray");

    (0..batch.num_rows())
        .map(|i| {
            let emb_values = embedding_list
                .value(i)
                .as_any()
                .downcast_ref::<Float32Array>()
                .expect("embedding values are Float32Array")
                .values()
                .to_vec();

            CachedSymbol {
                file_path: file_paths.value(i).to_string(),
                symbol_name: symbol_names.value(i).to_string(),
                symbol_type: symbol_types.value(i).to_string(),
                signature: signatures.value(i).to_string(),
                start_line: start_lines.value(i),
                end_line: end_lines.value(i),
                content_hash: content_hashes.value(i).to_string(),
                embedding: emb_values,
                chunk_index: chunk_indices.value(i),
                total_chunks: total_chunks_col.value(i),
                cyclomatic_complexity: cyclomatic_col.value(i),
                cognitive_complexity: cognitive_col.value(i),
                tdg_score: tdg_score_col.value(i),
                tdg_grade: tdg_grade_col.value(i).to_string(),
            }
        })
        .collect()
}

fn batch_to_file_entries(batch: &RecordBatch) -> Vec<(String, String)> {
    let file_paths = batch
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("column 0 is StringArray");
    let file_hashes = batch
        .column(1)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("column 1 is StringArray");

    (0..batch.num_rows())
        .map(|i| {
            (
                file_paths.value(i).to_string(),
                file_hashes.value(i).to_string(),
            )
        })
        .collect()
}

impl EmbeddingCache {
    /// Open or create an embedding cache at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let rt = Arc::new(
            Runtime::new()
                .map_err(|e| Error::analysis(format!("Failed to create tokio runtime: {}", e)))?,
        );

        let uri = path.to_string_lossy().to_string();

        let db = rt.block_on(async {
            lancedb::connect(&uri)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open LanceDB at {}: {}", uri, e)))
        })?;

        let cache = Self { db, rt };
        cache.ensure_tables()?;
        Ok(cache)
    }

    /// Create an in-memory cache (useful for testing).
    pub fn in_memory() -> Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);

        let dir =
            std::env::temp_dir().join(format!("omen-lancedb-test-{}-{}", std::process::id(), id));
        // Clean up any previous run
        let _ = std::fs::remove_dir_all(&dir);

        Self::open(&dir)
    }

    fn ensure_tables(&self) -> Result<()> {
        // Each table creation must be in a separate block_on call.
        // LanceDB's table registry is not updated within the same async context.
        self.ensure_table(SYMBOLS_TABLE, symbols_schema())?;
        self.ensure_table(FILES_TABLE, files_schema())?;
        Ok(())
    }

    fn ensure_table(&self, name: &str, schema: Arc<Schema>) -> Result<()> {
        let result = self
            .rt
            .block_on(async { self.db.create_empty_table(name, schema).execute().await });
        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("already exists") {
                    Ok(())
                } else {
                    Err(Error::analysis(format!(
                        "Failed to create {} table: {}",
                        name, msg
                    )))
                }
            }
        }
    }

    /// Insert or update a symbol in the cache.
    pub fn upsert_symbol(&self, symbol: &CachedSymbol) -> Result<()> {
        self.rt.block_on(async {
            let table = self
                .db
                .open_table(SYMBOLS_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open symbols table: {}", e)))?;

            // Delete existing entry if present
            let filter = format!(
                "file_path = '{}' AND symbol_name = '{}'",
                escape_sql(&symbol.file_path),
                escape_sql(&symbol.symbol_name)
            );
            let _ = table.delete(&filter).await;

            // Insert new
            let batch = symbols_to_batch(std::slice::from_ref(symbol))
                .map_err(|e| Error::analysis(format!("Failed to build record batch: {}", e)))?;
            let schema = symbols_schema();
            let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
            table
                .add(Box::new(batches))
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to insert symbol: {}", e)))?;

            Ok(())
        })
    }

    /// Insert symbols in bulk.
    pub fn insert_symbols(&self, symbols: &[CachedSymbol]) -> Result<()> {
        if symbols.is_empty() {
            return Ok(());
        }

        self.rt.block_on(async {
            let table = self
                .db
                .open_table(SYMBOLS_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open symbols table: {}", e)))?;

            let batch = symbols_to_batch(symbols)
                .map_err(|e| Error::analysis(format!("Failed to build record batch: {}", e)))?;
            let schema = symbols_schema();
            let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
            table
                .add(Box::new(batches))
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to insert symbols: {}", e)))?;

            Ok(())
        })
    }

    /// Get a symbol from the cache by file path and symbol name.
    pub fn get_symbol(&self, file_path: &str, symbol_name: &str) -> Result<Option<CachedSymbol>> {
        let symbols = self.query_symbols(&format!(
            "file_path = '{}' AND symbol_name = '{}'",
            escape_sql(file_path),
            escape_sql(symbol_name)
        ))?;
        Ok(symbols.into_iter().next())
    }

    /// Get all symbols from the cache.
    pub fn get_all_symbols(&self) -> Result<Vec<CachedSymbol>> {
        self.query_symbols_all()
    }

    /// Get all symbols for a specific file.
    pub fn get_symbols_for_file(&self, file_path: &str) -> Result<Vec<CachedSymbol>> {
        self.query_symbols(&format!("file_path = '{}'", escape_sql(file_path)))
    }

    /// Delete all symbols for a file.
    pub fn delete_file_symbols(&self, file_path: &str) -> Result<()> {
        self.rt.block_on(async {
            let table = self
                .db
                .open_table(SYMBOLS_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open symbols table: {}", e)))?;

            let filter = format!("file_path = '{}'", escape_sql(file_path));
            let _ = table.delete(&filter).await;
            Ok(())
        })
    }

    /// Record that a file has been indexed.
    pub fn record_file_indexed(&self, file_path: &str, file_hash: &str) -> Result<()> {
        self.rt.block_on(async {
            let table = self
                .db
                .open_table(FILES_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open files table: {}", e)))?;

            // Delete existing entry
            let filter = format!("file_path = '{}'", escape_sql(file_path));
            let _ = table.delete(&filter).await;

            // Insert new
            let schema = files_schema();
            let batch = RecordBatch::try_new(
                schema.clone(),
                vec![
                    Arc::new(StringArray::from(vec![file_path])),
                    Arc::new(StringArray::from(vec![file_hash])),
                ],
            )
            .map_err(|e| Error::analysis(format!("Failed to build file record: {}", e)))?;

            let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
            table
                .add(Box::new(batches))
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to insert file record: {}", e)))?;

            Ok(())
        })
    }

    /// Get the stored hash for a file.
    pub fn get_file_hash(&self, file_path: &str) -> Result<Option<String>> {
        let entries = self.query_files(&format!("file_path = '{}'", escape_sql(file_path)))?;
        Ok(entries.into_iter().next().map(|(_, hash)| hash))
    }

    /// Remove a file from the files table and its symbols.
    pub fn remove_file(&self, file_path: &str) -> Result<()> {
        self.delete_file_symbols(file_path)?;

        self.rt.block_on(async {
            let table = self
                .db
                .open_table(FILES_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open files table: {}", e)))?;

            let filter = format!("file_path = '{}'", escape_sql(file_path));
            let _ = table.delete(&filter).await;
            Ok(())
        })
    }

    /// Get all indexed file paths.
    pub fn get_all_indexed_files(&self) -> Result<Vec<String>> {
        let entries = self.query_files_all()?;
        Ok(entries.into_iter().map(|(path, _)| path).collect())
    }

    /// Get the number of cached symbols.
    pub fn symbol_count(&self) -> Result<usize> {
        self.rt.block_on(async {
            let table = self
                .db
                .open_table(SYMBOLS_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open symbols table: {}", e)))?;

            table
                .count_rows(None)
                .await
                .map_err(|e| Error::analysis(format!("Failed to count symbols: {}", e)))
        })
    }

    /// Perform vector search for nearest neighbors.
    pub fn vector_search(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<(CachedSymbol, f32)>> {
        self.rt.block_on(async {
            let table = self
                .db
                .open_table(SYMBOLS_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open symbols table: {}", e)))?;

            let count = table
                .count_rows(None)
                .await
                .map_err(|e| Error::analysis(format!("Failed to count rows: {}", e)))?;

            if count == 0 {
                return Ok(Vec::new());
            }

            let results = table
                .vector_search(query_embedding.to_vec())
                .map_err(|e| Error::analysis(format!("Failed to create vector query: {}", e)))?
                .limit(top_k)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Vector search failed: {}", e)))?
                .try_collect::<Vec<_>>()
                .await
                .map_err(|e| Error::analysis(format!("Failed to collect search results: {}", e)))?;

            let mut scored_symbols = Vec::new();
            for batch in &results {
                let symbols = batch_to_symbols(batch);

                // LanceDB returns _distance column
                let distance_col = batch
                    .column_by_name("_distance")
                    .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

                for (i, symbol) in symbols.into_iter().enumerate() {
                    let score = if let Some(distances) = distance_col {
                        // Convert L2 distance to similarity: 1 / (1 + distance)
                        1.0 / (1.0 + distances.value(i))
                    } else {
                        0.0
                    };
                    scored_symbols.push((symbol, score));
                }
            }

            Ok(scored_symbols)
        })
    }

    /// Perform vector search within specific files.
    pub fn vector_search_in_files(
        &self,
        query_embedding: &[f32],
        file_paths: &[&str],
        top_k: usize,
    ) -> Result<Vec<(CachedSymbol, f32)>> {
        if file_paths.is_empty() {
            return self.vector_search(query_embedding, top_k);
        }

        self.rt.block_on(async {
            let table = self
                .db
                .open_table(SYMBOLS_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open symbols table: {}", e)))?;

            let count = table
                .count_rows(None)
                .await
                .map_err(|e| Error::analysis(format!("Failed to count rows: {}", e)))?;

            if count == 0 {
                return Ok(Vec::new());
            }

            let file_list: Vec<String> = file_paths
                .iter()
                .map(|f| format!("'{}'", escape_sql(f)))
                .collect();
            let filter = format!("file_path IN ({})", file_list.join(", "));

            let results = table
                .vector_search(query_embedding.to_vec())
                .map_err(|e| Error::analysis(format!("Failed to create vector query: {}", e)))?
                .only_if(filter)
                .limit(top_k)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Vector search failed: {}", e)))?
                .try_collect::<Vec<_>>()
                .await
                .map_err(|e| Error::analysis(format!("Failed to collect search results: {}", e)))?;

            let mut scored_symbols = Vec::new();
            for batch in &results {
                let symbols = batch_to_symbols(batch);
                let distance_col = batch
                    .column_by_name("_distance")
                    .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

                for (i, symbol) in symbols.into_iter().enumerate() {
                    let score = if let Some(distances) = distance_col {
                        1.0 / (1.0 + distances.value(i))
                    } else {
                        0.0
                    };
                    scored_symbols.push((symbol, score));
                }
            }

            Ok(scored_symbols)
        })
    }

    fn query_symbols(&self, filter: &str) -> Result<Vec<CachedSymbol>> {
        self.rt.block_on(async {
            let table = self
                .db
                .open_table(SYMBOLS_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open symbols table: {}", e)))?;

            let batches = table
                .query()
                .only_if(filter)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to query symbols: {}", e)))?
                .try_collect::<Vec<_>>()
                .await
                .map_err(|e| Error::analysis(format!("Failed to collect symbols: {}", e)))?;

            let mut symbols = Vec::new();
            for batch in &batches {
                symbols.extend(batch_to_symbols(batch));
            }
            Ok(symbols)
        })
    }

    fn query_symbols_all(&self) -> Result<Vec<CachedSymbol>> {
        self.rt.block_on(async {
            let table = self
                .db
                .open_table(SYMBOLS_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open symbols table: {}", e)))?;

            let batches = table
                .query()
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to query symbols: {}", e)))?
                .try_collect::<Vec<_>>()
                .await
                .map_err(|e| Error::analysis(format!("Failed to collect symbols: {}", e)))?;

            let mut symbols = Vec::new();
            for batch in &batches {
                symbols.extend(batch_to_symbols(batch));
            }
            Ok(symbols)
        })
    }

    fn query_files(&self, filter: &str) -> Result<Vec<(String, String)>> {
        self.rt.block_on(async {
            let table = self
                .db
                .open_table(FILES_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open files table: {}", e)))?;

            let batches = table
                .query()
                .only_if(filter)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to query files: {}", e)))?
                .try_collect::<Vec<_>>()
                .await
                .map_err(|e| Error::analysis(format!("Failed to collect files: {}", e)))?;

            let mut entries = Vec::new();
            for batch in &batches {
                entries.extend(batch_to_file_entries(batch));
            }
            Ok(entries)
        })
    }

    fn query_files_all(&self) -> Result<Vec<(String, String)>> {
        self.rt.block_on(async {
            let table = self
                .db
                .open_table(FILES_TABLE)
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to open files table: {}", e)))?;

            let batches = table
                .query()
                .execute()
                .await
                .map_err(|e| Error::analysis(format!("Failed to query files: {}", e)))?
                .try_collect::<Vec<_>>()
                .await
                .map_err(|e| Error::analysis(format!("Failed to collect files: {}", e)))?;

            let mut entries = Vec::new();
            for batch in &batches {
                entries.extend(batch_to_file_entries(batch));
            }
            Ok(entries)
        })
    }
}

/// Escape single quotes in SQL filter strings.
fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_symbol() -> CachedSymbol {
        CachedSymbol {
            file_path: "src/main.rs".to_string(),
            symbol_name: "test_func".to_string(),
            symbol_type: "function".to_string(),
            signature: "fn test_func()".to_string(),
            start_line: 1,
            end_line: 5,
            content_hash: "abc123".to_string(),
            embedding: vec![0.0; EMBEDDING_DIM as usize],
            chunk_index: 0,
            total_chunks: 1,
            cyclomatic_complexity: 0,
            cognitive_complexity: 0,
            tdg_score: 0.0,
            tdg_grade: String::new(),
        }
    }

    fn create_symbol_with_embedding(file: &str, name: &str, embedding: Vec<f32>) -> CachedSymbol {
        assert_eq!(embedding.len(), EMBEDDING_DIM as usize);
        CachedSymbol {
            file_path: file.to_string(),
            symbol_name: name.to_string(),
            symbol_type: "function".to_string(),
            signature: format!("fn {}()", name),
            start_line: 1,
            end_line: 5,
            content_hash: format!("hash_{}", name),
            embedding,
            chunk_index: 0,
            total_chunks: 1,
            cyclomatic_complexity: 0,
            cognitive_complexity: 0,
            tdg_score: 0.0,
            tdg_grade: String::new(),
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
            .get_symbol(&symbol.file_path, &symbol.symbol_name)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.file_path, symbol.file_path);
        assert_eq!(retrieved.symbol_name, symbol.symbol_name);
        assert_eq!(retrieved.embedding.len(), EMBEDDING_DIM as usize);
    }

    #[test]
    fn test_upsert_updates_existing() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let mut symbol = create_test_symbol();

        cache.upsert_symbol(&symbol).unwrap();

        symbol.content_hash = "new_hash".to_string();
        cache.upsert_symbol(&symbol).unwrap();

        let retrieved = cache
            .get_symbol(&symbol.file_path, &symbol.symbol_name)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.content_hash, "new_hash");
        assert_eq!(cache.symbol_count().unwrap(), 1);
    }

    #[test]
    fn test_get_nonexistent_symbol() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let result = cache.get_symbol("nonexistent", "func").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_all_symbols() {
        let cache = EmbeddingCache::in_memory().unwrap();

        let symbol1 = CachedSymbol {
            file_path: "src/a.rs".to_string(),
            symbol_name: "func_a".to_string(),
            symbol_type: "function".to_string(),
            signature: "fn func_a()".to_string(),
            start_line: 1,
            end_line: 5,
            content_hash: "hash1".to_string(),
            embedding: vec![0.0; EMBEDDING_DIM as usize],
            chunk_index: 0,
            total_chunks: 1,
            cyclomatic_complexity: 0,
            cognitive_complexity: 0,
            tdg_score: 0.0,
            tdg_grade: String::new(),
        };

        let symbol2 = CachedSymbol {
            file_path: "src/b.rs".to_string(),
            symbol_name: "func_b".to_string(),
            symbol_type: "function".to_string(),
            signature: "fn func_b()".to_string(),
            start_line: 1,
            end_line: 5,
            content_hash: "hash2".to_string(),
            embedding: vec![0.0; EMBEDDING_DIM as usize],
            chunk_index: 0,
            total_chunks: 1,
            cyclomatic_complexity: 0,
            cognitive_complexity: 0,
            tdg_score: 0.0,
            tdg_grade: String::new(),
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
            signature: "fn func_a()".to_string(),
            start_line: 1,
            end_line: 5,
            content_hash: "hash1".to_string(),
            embedding: vec![0.0; EMBEDDING_DIM as usize],
            chunk_index: 0,
            total_chunks: 1,
            cyclomatic_complexity: 0,
            cognitive_complexity: 0,
            tdg_score: 0.0,
            tdg_grade: String::new(),
        };

        let symbol2 = CachedSymbol {
            file_path: "src/main.rs".to_string(),
            symbol_name: "func_b".to_string(),
            symbol_type: "function".to_string(),
            signature: "fn func_b()".to_string(),
            start_line: 10,
            end_line: 15,
            content_hash: "hash2".to_string(),
            embedding: vec![0.0; EMBEDDING_DIM as usize],
            chunk_index: 0,
            total_chunks: 1,
            cyclomatic_complexity: 0,
            cognitive_complexity: 0,
            tdg_score: 0.0,
            tdg_grade: String::new(),
        };

        let symbol3 = CachedSymbol {
            file_path: "src/other.rs".to_string(),
            symbol_name: "func_c".to_string(),
            symbol_type: "function".to_string(),
            signature: "fn func_c()".to_string(),
            start_line: 1,
            end_line: 5,
            content_hash: "hash3".to_string(),
            embedding: vec![0.0; EMBEDDING_DIM as usize],
            chunk_index: 0,
            total_chunks: 1,
            cyclomatic_complexity: 0,
            cognitive_complexity: 0,
            tdg_score: 0.0,
            tdg_grade: String::new(),
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
    fn test_bulk_insert() {
        let cache = EmbeddingCache::in_memory().unwrap();

        let symbols: Vec<CachedSymbol> = (0..100)
            .map(|i| CachedSymbol {
                file_path: format!("src/file_{}.rs", i),
                symbol_name: format!("func_{}", i),
                symbol_type: "function".to_string(),
                signature: format!("fn func_{}()", i),
                start_line: 1,
                end_line: 5,
                content_hash: format!("hash_{}", i),
                embedding: vec![i as f32 / 100.0; EMBEDDING_DIM as usize],
                chunk_index: 0,
                total_chunks: 1,
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 0.0,
                tdg_grade: String::new(),
            })
            .collect();

        cache.insert_symbols(&symbols).unwrap();
        assert_eq!(cache.symbol_count().unwrap(), 100);
    }

    #[test]
    fn test_vector_search() {
        let cache = EmbeddingCache::in_memory().unwrap();

        // Create two symbols with different embeddings
        let mut emb1 = vec![0.0f32; EMBEDDING_DIM as usize];
        emb1[0] = 1.0; // "points" in dimension 0

        let mut emb2 = vec![0.0f32; EMBEDDING_DIM as usize];
        emb2[1] = 1.0; // "points" in dimension 1

        let sym1 = create_symbol_with_embedding("src/a.rs", "func_a", emb1);
        let sym2 = create_symbol_with_embedding("src/b.rs", "func_b", emb2);

        cache.upsert_symbol(&sym1).unwrap();
        cache.upsert_symbol(&sym2).unwrap();

        // Search with query close to sym1
        let mut query = vec![0.0f32; EMBEDDING_DIM as usize];
        query[0] = 1.0;

        let results = cache.vector_search(&query, 2).unwrap();
        assert_eq!(results.len(), 2);
        // First result should be sym1 (closest to query)
        assert_eq!(results[0].0.symbol_name, "func_a");
        // Score should be higher for closer match
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn test_vector_search_empty_table() {
        let cache = EmbeddingCache::in_memory().unwrap();
        let query = vec![0.0f32; EMBEDDING_DIM as usize];
        let results = cache.vector_search(&query, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_escape_sql() {
        assert_eq!(escape_sql("hello"), "hello");
        assert_eq!(escape_sql("it's"), "it''s");
        assert_eq!(escape_sql("a'b'c"), "a''b''c");
    }

    #[test]
    fn test_symbols_schema() {
        let schema = symbols_schema();
        assert_eq!(schema.fields().len(), 14);
        assert_eq!(schema.field(0).name(), "file_path");
        assert_eq!(schema.field(7).name(), "embedding");
        assert_eq!(schema.field(8).name(), "chunk_index");
        assert_eq!(schema.field(9).name(), "total_chunks");
        assert_eq!(schema.field(10).name(), "cyclomatic_complexity");
        assert_eq!(schema.field(11).name(), "cognitive_complexity");
        assert_eq!(schema.field(12).name(), "tdg_score");
        assert_eq!(schema.field(13).name(), "tdg_grade");
    }

    #[test]
    fn test_record_batch_roundtrip() {
        let symbol = create_test_symbol();
        let batch = symbols_to_batch(std::slice::from_ref(&symbol)).unwrap();
        let recovered = batch_to_symbols(&batch);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].file_path, symbol.file_path);
        assert_eq!(recovered[0].symbol_name, symbol.symbol_name);
        assert_eq!(recovered[0].embedding.len(), EMBEDDING_DIM as usize);
    }
}
