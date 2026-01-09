# Semantic Search

Embedding-based semantic search for natural language code discovery.

```bash
omen search query "how do we handle authentication"
```

Returns ranked results based on semantic similarity, not keyword matching.

## Quick Start

```bash
# Build the search index
omen search index

# Search for code
omen search query "database connection pooling"

# More options
omen search query "error handling" --top-k 20 --min-score 0.5
omen search query "validation" --files src/api/,src/handlers/
```

## Commands

### `omen search index`

Build or update the search index.

```bash
omen search index [OPTIONS]

Options:
  --force    Force full re-index (ignore cache)
```

On first run, indexes all functions in the codebase. Subsequent runs only re-index changed files (detected via content hashing).

### `omen search query`

Search for code symbols.

```bash
omen search query [OPTIONS] <QUERY>

Arguments:
  <QUERY>    Natural language query

Options:
  -k, --top-k <N>           Maximum results [default: 10]
  --min-score <SCORE>       Minimum similarity (0.0-1.0) [default: 0.3]
  --files <PATHS>           Limit to specific files (comma-separated)
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                   omen search query "..."                   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Sync Manager                            │
│  - Check .omen/search.db for stale files                   │
│  - Compare content hashes                                   │
│  - Trigger incremental update                               │
└─────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
┌──────────────────────────┐    ┌──────────────────────────┐
│    Symbol Extractor      │    │    Embedding Engine      │
│  - tree-sitter parsing   │    │  - candle ML framework   │
│  - Extract functions     │    │  - all-MiniLM-L6-v2      │
│  - Parallel with rayon   │    │  - Batch embedding (64)  │
│  - Content hashing       │    │  - 384-dim vectors       │
└──────────────────────────┘    └──────────────────────────┘
              │                               │
              └───────────────┬───────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    SQLite Cache                             │
│  .omen/search.db                                            │
│  - symbols: file, name, signature, lines, embedding, hash   │
│  - files: path, content_hash, indexed_at                    │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Search Engine                            │
│  - Embed query string                                       │
│  - Cosine similarity against cached embeddings              │
│  - Rank and return top N                                    │
└─────────────────────────────────────────────────────────────┘
```

## Embedding Model

Uses [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) via the candle ML framework for local inference.

| Property | Value |
|----------|-------|
| Model | all-MiniLM-L6-v2 |
| Framework | candle (Rust) |
| Dimensions | 384 |
| Model size | ~80MB |
| Inference | CPU (no GPU required) |

The model is downloaded automatically on first use and cached to `~/.cache/omen/models/`.

### Alternative Providers

Omen also supports third-party embedding APIs for higher quality embeddings:

```toml
# omen.toml
[semantic]
provider = "openai"        # Uses OPENAI_API_KEY env var
# provider = "cohere"      # Uses COHERE_API_KEY env var
# provider = "voyage"      # Uses VOYAGE_API_KEY env var (optimized for code)
```

## Performance

### Indexing

- **Parallel file parsing** with rayon
- **Batch embedding** (64 symbols per inference call)
- **Incremental updates** via content hashing (blake3)

Benchmark on omen codebase (~1,300 symbols):
- Cold index: ~6 minutes on CPU
- Incremental update: <1 second for changed files

### Search

- Brute-force cosine similarity over all embeddings
- Query time: ~50-100ms for typical codebases (<100k symbols)
- For larger codebases, future versions may add HNSW indexing

## Storage

Index stored in `.omen/search.db` (SQLite):

| Repo Size | Symbols | Storage |
|-----------|---------|---------|
| 10k LOC | ~500 | ~1 MB |
| 100k LOC | ~5,000 | ~10 MB |
| 1M LOC | ~50,000 | ~100 MB |

## Limitations

- **Functions only** - Currently indexes function/method definitions. Classes, types, and modules not yet indexed.
- **CPU inference** - candle runs on CPU. No GPU acceleration currently.
- **No cross-repo search** - Each repository has its own index.

## MCP Integration

The semantic search is available as an MCP tool:

```json
{
  "name": "semantic_search",
  "arguments": {
    "query": "error handling middleware",
    "top_k": 10
  }
}
```
