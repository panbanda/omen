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

- **Vocabulary mismatch.** You search for "database connection pooling" but the code uses `ConnectionManager` with a `pool_size` field. grep finds nothing. Semantic search finds it because TF-IDF bigrams and term overlap capture the relationship.
- **Exploratory search.** You're new to a codebase and want to find "where errors are handled." There's no single string to grep for -- error handling might use `catch`, `Result`, `rescue`, `except`, `try`, or custom error types depending on the language. Semantic search surfaces all of them.
- **Concept search.** You want to find "retry logic" or "rate limiting." These are concepts implemented in many different ways, with no consistent naming convention.

Semantic search complements grep and ripgrep. Use text search when you know what you're looking for. Use semantic search when you know what it does but not what it's called.

## How It Works

### 1. Symbol Extraction

Omen uses tree-sitter to parse source files and extract named symbols: functions, methods, classes, structs, traits, interfaces, constants, and type definitions. Each symbol includes its name, the file it lives in, its line range, and a snippet of its body.

This is syntax-aware extraction, not line-based. A function's full signature and body are captured as a unit, even if they span many lines.

### 2. AST-Aware Chunking

Long functions (over 500 characters) are split at statement boundaries within the function body. Each chunk retains its parent context -- for example, a method chunk knows which class or struct it belongs to. Short functions stay as single chunks.

The enriched text format for each chunk is `[file] Parent::name (i/n)\ncontent`, giving the TF-IDF engine focused vocabulary per document.

### 3. TF-IDF Indexing

Each chunk is indexed using a TF-IDF (Term Frequency-Inverse Document Frequency) engine with:

- **Sublinear TF** -- dampens the effect of repeated terms
- **Smooth IDF** -- prevents division by zero for rare terms
- **Bigram tokenization** -- captures two-word phrases alongside unigrams

The engine builds sparse vectors and uses L2-normalized cosine similarity for ranking. This is a pure-Rust implementation with zero external dependencies -- no models to download, no API keys, no GPU required.

### 4. Incremental Indexing

Omen tracks which files have been indexed and their last-modified timestamps. On subsequent runs, only new or changed files are re-parsed and re-indexed. The index is stored in a SQLite database at `.omen/search.db`.

### 5. Deduplication

When a symbol was split into multiple chunks, search results are deduplicated so each symbol appears at most once, using the best-scoring chunk's score.

## Commands

### Build the Index

```bash
omen search index
```

Parses all source files, extracts symbols, chunks long functions, and stores enriched text in `.omen/search.db`. Typical indexing takes 1-2 seconds.

### Force Re-index

```bash
omen search index --force
```

Deletes the existing index and rebuilds from scratch.

### Query the Index

```bash
omen search query "database connection pooling"
```

Returns the top 10 most semantically similar symbols by default.

### Adjust Result Count

```bash
omen search query "error handling" --top-k 20
```

### Filter by Minimum Score

```bash
omen search query "retry logic" --min-score 0.5
```

### Limit to Specific Files

```bash
omen search query "authentication" --files src/auth/,src/middleware/
```

### Multi-Repository Search

Search across multiple project indexes with unified IDF scoring:

```bash
omen search query "retry logic" --include-project /path/to/other-repo,/path/to/another
```

Each project must have been previously indexed (`omen search index` run in that directory). File paths in results are prefixed with the repository name for identification.

## HyDE Search

HyDE (Hypothetical Document Embedding) search lets you write a hypothetical code snippet as your query instead of a natural language description. This can produce better matches when you know roughly what the code should look like.

HyDE search is available via the MCP `semantic_search_hyde` tool. You provide a `document` parameter containing the hypothetical code snippet, and Omen matches it against the index.

## Complexity Filtering

Search results can include per-function complexity metrics (cyclomatic and cognitive complexity) when available. You can filter results to exclude overly complex functions using the `max_complexity` parameter on the MCP `semantic_search` and `semantic_search_hyde` tools.

## Performance

| Metric | Value |
|---|---|
| Index time | ~1-2s (1,400 symbols) |
| Query time | ~250ms |
| Storage | SQLite in `.omen/search.db` |
| File parsing | Parallel via rayon |
| Dependencies | Zero external (pure Rust) |
| Index updates | Incremental (changed files only) |

## Storage

The `.omen/search.db` file contains:

- Extracted symbol metadata (name, file, line range, language, parent type)
- Enriched text (the source code snippet used for TF-IDF)
- Chunk metadata (chunk index, total chunks per symbol)
- Complexity metrics (cyclomatic and cognitive, when computed)
- File modification timestamps (for incremental indexing)

To rebuild the index from scratch, run `omen search index --force` or delete `.omen/search.db` and re-run `omen search index`.
