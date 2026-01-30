---
sidebar_position: 15
---

# Semantic Search

Semantic search lets you find code by meaning rather than by exact text matches. Instead of searching for a specific function name or string literal, you describe what you're looking for in natural language, and Omen returns the most relevant symbols.

```bash
omen search query "database connection pooling"
omen search query "error handling middleware"
omen search query "user authentication flow"
```

## Why Not grep?

Tools like `grep` and `ripgrep` search for literal patterns or regular expressions. They're fast and effective when you know the exact terms used in the code. But they fail in several common scenarios:

- **Vocabulary mismatch.** You search for "database connection pooling" but the code uses `ConnectionManager` with a `pool_size` field. grep finds nothing. Semantic search finds it because the embedding model understands the relationship between these concepts.
- **Exploratory search.** You're new to a codebase and want to find "where errors are handled." There's no single string to grep for -- error handling might use `catch`, `Result`, `rescue`, `except`, `try`, or custom error types depending on the language. Semantic search surfaces all of them.
- **Concept search.** You want to find "retry logic" or "rate limiting." These are concepts implemented in many different ways, with no consistent naming convention. Embeddings capture the semantic similarity that text matching cannot.

Semantic search complements grep and ripgrep. Use text search when you know what you're looking for. Use semantic search when you know what it does but not what it's called.

## How It Works

### 1. Symbol Extraction

Omen uses tree-sitter to parse source files and extract named symbols: functions, methods, classes, structs, traits, interfaces, constants, and type definitions. Each symbol includes its name, the file it lives in, its line range, and a snippet of its body.

This is syntax-aware extraction, not line-based. A function's full signature and body are captured as a unit, even if they span many lines.

### 2. Embedding Generation

Each extracted symbol is converted into a 384-dimensional vector embedding that captures its semantic meaning. The default embedding model is [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2), a sentence transformer that runs locally via the [candle](https://github.com/huggingface/candle) inference library.

Key properties of the default setup:

- **Local inference.** The model runs on your machine. No API keys, no network requests, no data leaving your environment.
- **No GPU required.** Candle supports CPU inference. A GPU will accelerate embedding generation but is not necessary.
- **384-dimensional vectors.** Compact enough for fast similarity computation, expressive enough for meaningful semantic distinctions.

### 3. Incremental Indexing

Omen tracks which files have been indexed and their last-modified timestamps. On subsequent runs, only new or changed files are re-parsed and re-embedded. This makes index updates fast even on large codebases.

The index is stored in a SQLite database at `.omen/search.db` in the project root.

### 4. Cosine Similarity Ranking

When you run a query, Omen embeds your query string using the same model, then computes the cosine similarity between the query vector and every symbol vector in the index. Results are ranked by similarity score and returned with their file locations.

## Commands

### Build the Index

```bash
omen search index
```

Parses all source files, extracts symbols, generates embeddings, and stores them in `.omen/search.db`. Run this once initially, then again after significant code changes (or let incremental indexing handle it).

### Query the Index

```bash
omen search query "database connection pooling"
```

Returns the top 10 most semantically similar symbols by default.

### Adjust Result Count

```bash
omen search query "error handling" --top-k 20
```

The `--top-k` flag controls how many results are returned.

## Alternative Embedding Providers

The default local model works without configuration, but Omen also supports external embedding APIs for higher-quality embeddings or consistency with an existing embedding pipeline.

Configure providers in `omen.toml`:

```toml
[search]
# Default: local all-MiniLM-L6-v2 via candle
provider = "local"

# OpenAI
# provider = "openai"
# model = "text-embedding-3-small"
# api_key_env = "OPENAI_API_KEY"

# Cohere
# provider = "cohere"
# model = "embed-english-v3.0"
# api_key_env = "COHERE_API_KEY"

# Voyage AI
# provider = "voyage"
# model = "voyage-code-2"
# api_key_env = "VOYAGE_API_KEY"
```

When using an external provider, symbol text is sent to the API for embedding. The resulting vectors are stored locally in the same SQLite database.

Voyage AI's `voyage-code-2` model is specifically trained on source code and may produce better results for code search compared to general-purpose text embedding models.

## Performance Characteristics

| Metric | Value |
|---|---|
| Storage | SQLite in `.omen/search.db` |
| File parsing | Parallel via rayon |
| Batch size | 64 symbols per embedding batch |
| Throughput (local, CPU) | ~3.5 symbols/second |
| Vector dimensions | 384 (all-MiniLM-L6-v2) |
| Index updates | Incremental (changed files only) |

For a codebase with 10,000 symbols, initial indexing takes roughly 45 minutes on CPU. Subsequent incremental updates are much faster, typically seconds to minutes depending on how many files changed.

If initial indexing speed is a concern, use an external embedding provider -- API-based embedding is significantly faster than local CPU inference, at the cost of network latency and API fees.

## Storage and Caching

The `.omen/search.db` file contains:

- Extracted symbol metadata (name, file, line range, language)
- Symbol text (the source code snippet used for embedding)
- Embedding vectors (384 floats per symbol)
- File modification timestamps (for incremental indexing)

This file can be committed to version control if you want to share the index across a team, or added to `.gitignore` if you prefer each developer to build their own. The database is portable across machines using the same embedding model.

To rebuild the index from scratch, delete `.omen/search.db` and re-run `omen search index`.
