# Semantic Search for Omen

## Objective

Add embedding-based semantic search to Omen, enabling natural language code discovery. Users should be able to query their codebase by meaning rather than exact text matches.

```bash
omen search "how do we handle authentication"
```

Returns ranked results based on semantic similarity, not keyword matching.

## Design Principles

1. **Zero configuration** - No explicit sync command. First query builds the index, subsequent queries update incrementally.
2. **Offline-first** - Local model, no API keys required.
3. **Git-aware caching** - Only re-embed changed files.
4. **Combine with PageRank** - Weight results by both semantic relevance and structural importance.

## User Experience

```bash
# First run (cold cache)
$ omen search "database connection pooling"
Indexing 8,432 symbols... done (28s)

src/db/pool.rs:45      ConnectionPool::acquire    0.94
src/db/pool.rs:112     ConnectionPool::release    0.89
src/db/config.rs:23    DatabaseConfig::pool_size  0.84
...

# Subsequent runs (warm cache)
$ omen search "error handling"
Updated 3 symbols... done (180ms)

src/error.rs:12        AppError::from             0.91
src/api/handler.rs:67  handle_error               0.88
...
```

### Command Options

```bash
omen search <query>           # Search with default settings
  -n, --limit <N>             # Max results (default: 20)
  -t, --threshold <F>         # Min similarity score 0.0-1.0 (default: 0.5)
  --pagerank-weight <F>       # Weight for PageRank blend (default: 0.3)
  --rebuild                   # Force full re-index
  -f, --format <FORMAT>       # Output: text, json, markdown
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      omen search "query"                     │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Cache Manager                            │
│  - Check .omen/embeddings.db                                │
│  - Identify stale files (git status + mtime)                │
│  - Trigger incremental update                               │
└─────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
┌──────────────────────────┐    ┌──────────────────────────┐
│    Symbol Extractor      │    │    Embedding Engine      │
│  - tree-sitter parsing   │    │  - ONNX Runtime          │
│  - Extract functions,    │    │  - all-MiniLM-L6-v2      │
│    classes, methods      │    │  - Batch embedding       │
│  - Content hashing       │    │  - 384-dim vectors       │
└──────────────────────────┘    └──────────────────────────┘
              │                               │
              └───────────────┬───────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    SQLite Cache                              │
│  .omen/embeddings.db                                        │
│  - symbols: file, name, signature, line, embedding, hash    │
│  - metadata: model version, last full sync                  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Query Engine                              │
│  - Embed query string                                       │
│  - Cosine similarity against all cached embeddings          │
│  - Blend with PageRank scores (optional)                    │
│  - Rank and return top N                                    │
└─────────────────────────────────────────────────────────────┘
```

## Data Model

### SQLite Schema

```sql
-- .omen/embeddings.db

CREATE TABLE metadata (
    key TEXT PRIMARY KEY,
    value TEXT
);
-- Keys: model_version, last_full_sync, schema_version

CREATE TABLE symbols (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL,
    symbol_name TEXT NOT NULL,
    symbol_type TEXT NOT NULL,  -- function, class, method, struct, etc.
    signature TEXT,
    line_start INTEGER,
    line_end INTEGER,
    content_hash BLOB NOT NULL,  -- xxh3 of symbol source
    embedding BLOB NOT NULL,     -- 384 x float32 = 1536 bytes
    pagerank REAL DEFAULT 0.0,   -- cached from repomap
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_symbols_file ON symbols(file_path);
CREATE INDEX idx_symbols_hash ON symbols(content_hash);
```

### Embedding Storage

Store as raw bytes (little-endian float32):

```rust
// 384 dimensions * 4 bytes = 1,536 bytes per symbol
fn serialize_embedding(embedding: &[f32; 384]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn deserialize_embedding(bytes: &[u8]) -> [f32; 384] {
    let mut embedding = [0f32; 384];
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        embedding[i] = f32::from_le_bytes(chunk.try_into().unwrap());
    }
    embedding
}
```

## Implementation Plan

### Phase 1: Core Infrastructure

1. **Add dependencies to Cargo.toml**
   - `ort` (ONNX Runtime bindings)
   - `rusqlite` with `bundled` feature
   - `tokenizers` (for model tokenization)

2. **Model management** (`src/semantic/model.rs`)
   - Download all-MiniLM-L6-v2 ONNX on first use
   - Cache to `~/.cache/omen/models/`
   - Verify hash on load

3. **Embedding engine** (`src/semantic/embed.rs`)
   - Load ONNX model
   - Tokenize input text
   - Run inference
   - Return 384-dim vector

### Phase 2: Cache Layer

4. **SQLite cache** (`src/semantic/cache.rs`)
   - Initialize database schema
   - CRUD operations for symbols
   - Batch insert for efficiency

5. **Staleness detection** (`src/semantic/sync.rs`)
   - Get modified files from git
   - Compare content hashes
   - Return list of files needing re-embedding

### Phase 3: Search Command

6. **Symbol extraction integration**
   - Reuse existing tree-sitter infrastructure from `repomap`
   - Extract symbol content (not just signature) for embedding

7. **Search command** (`src/cli/search.rs`)
   - Parse arguments
   - Orchestrate cache check → update → query flow
   - Format and display results

8. **Query engine** (`src/semantic/search.rs`)
   - Embed query string
   - Load all embeddings from cache
   - Compute cosine similarity
   - Optional: blend with PageRank
   - Sort and return top N

### Phase 4: Polish

9. **Progress reporting**
   - Use indicatif for indexing progress
   - Show "Updated N symbols" on incremental updates

10. **MCP integration**
    - Add `semantic_search` tool
    - Same interface as CLI

## Technical Decisions

### Why all-MiniLM-L6-v2?

| Model | Dimensions | Size | Latency | Quality |
|-------|------------|------|---------|---------|
| all-MiniLM-L6-v2 | 384 | 80MB | ~5ms | Good |
| all-mpnet-base-v2 | 768 | 420MB | ~15ms | Better |
| CodeBERT | 768 | 500MB | ~20ms | Best for code |

MiniLM is the right tradeoff for a CLI tool: small, fast, good enough. Can revisit if quality is insufficient.

### Why ONNX Runtime?

- Rust bindings via `ort` crate
- No Python dependency
- Optimized inference
- Works on CPU (M1 Mac, Linux, Windows)
- Same model format as HuggingFace exports

### Why SQLite?

- Single file, no server
- Embedded in binary via `rusqlite` bundled feature
- Fast enough for brute-force similarity on <100k vectors
- Familiar, debuggable (`sqlite3 .omen/embeddings.db`)

### Why Not a Vector Database?

For <100k symbols, brute-force cosine similarity is fast enough (~10ms). Vector databases (FAISS, Milvus, Qdrant) add complexity without meaningful benefit at this scale. Can add HNSW indexing later if needed.

### PageRank Blending

Final score combines semantic similarity with structural importance:

```
score = (1 - α) * cosine_similarity + α * normalized_pagerank
```

Default α = 0.3. This surfaces code that is both relevant to the query AND central to the codebase.

## Storage Estimates

| Repo Size | Symbols | Embedding Storage | Total DB |
|-----------|---------|-------------------|----------|
| 10k LOC | ~500 | 750 KB | ~1 MB |
| 100k LOC | ~5,000 | 7.5 MB | ~10 MB |
| 1M LOC | ~50,000 | 75 MB | ~100 MB |

Acceptable for local development.

## Open Questions

1. **What to embed?** Full function body vs signature + docstring? Longer context = better semantics but slower indexing.

2. **Batch size for embedding?** Need to benchmark. Likely 32-64 symbols per inference call.

3. **Incremental PageRank?** Currently repomap recomputes full graph. Could cache and update incrementally, or just recompute on `--rebuild`.

4. **Multi-repo support?** Each repo gets its own `.omen/embeddings.db`. No cross-repo search initially.

## Success Criteria

- Cold index of 10k symbols in <60 seconds
- Warm query in <500ms
- Relevant results in top 5 for common queries
- No external API dependencies
- Works offline after model download

## References

- [Sentence Transformers](https://www.sbert.net/)
- [ONNX Runtime Rust](https://github.com/pykeio/ort)
- [all-MiniLM-L6-v2 on HuggingFace](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2)
- [PMAT semantic search implementation](https://github.com/paiml/paiml-mcp-agent-toolkit)
