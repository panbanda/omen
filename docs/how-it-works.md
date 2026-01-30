---
sidebar_position: 3
---

# How It Works

This page explains the internal architecture of Omen: how code is parsed, how analyzers are structured, how concurrency is managed, and how results flow from source files to output.

## Architecture Overview

Omen's analysis pipeline has five stages:

```
File Discovery -> Language Detection -> Tree-sitter Parsing -> Analyzer Execution -> Output Formatting
```

Each stage is designed to be fast, parallel where possible, and independent enough that individual analyzers can be run in isolation or composed together.

## File Discovery with FileSet

The first stage collects the set of files to analyze. Omen uses a `FileSet` abstraction that walks the target directory, respects `.gitignore` rules, applies language filters, and excludes common non-source directories (vendor, node_modules, build artifacts).

The result is a flat list of file paths with their detected languages, ready for parallel processing.

## Language Detection

Omen identifies languages by file extension, mapping each to one of the 13 supported tree-sitter grammars:

| Language | Extensions |
|---|---|
| Go | `.go` |
| Rust | `.rs` |
| Python | `.py`, `.pyi` |
| TypeScript | `.ts` |
| JavaScript | `.js`, `.mjs`, `.cjs` |
| TSX | `.tsx` |
| JSX | `.jsx` |
| Java | `.java` |
| C | `.c`, `.h` |
| C++ | `.cpp`, `.cc`, `.cxx`, `.hpp` |
| C# | `.cs` |
| Ruby | `.rb` |
| PHP | `.php` |
| Bash | `.sh`, `.bash` |

Files with unrecognized extensions are skipped. There is no heuristic-based detection -- extension mapping is deterministic.

## Tree-sitter Parsing

Omen uses [tree-sitter](https://tree-sitter.github.io/tree-sitter/) for all source code parsing. Tree-sitter produces a concrete syntax tree (CST) for each file, giving analyzers access to the full syntactic structure without writing language-specific parsers.

Key properties of this approach:

- **Syntax-aware analysis.** Analyzers operate on AST nodes (functions, classes, loops, conditionals), not lines of text. This eliminates false positives from comments, strings, and formatting.
- **Error-tolerant parsing.** Tree-sitter can produce a partial tree even when the source has syntax errors. Analysis continues on the valid portions.
- **Consistent API across languages.** Each grammar exposes a tree of typed nodes. Analyzers query node types (e.g., `function_definition`, `if_statement`, `class_declaration`) and traverse the tree using the same API regardless of language.

Parsing is done per-file. The parsed tree is passed to whichever analyzers need it, then discarded. Omen does not maintain a persistent AST cache.

## The Analyzer Trait

Every analyzer in Omen implements a common pattern: an `analyze()` function that takes a path (and options) and returns a result struct that implements `Serialize`.

```rust
// Simplified illustration of the pattern
pub struct ComplexityAnalyzer;

impl ComplexityAnalyzer {
    pub fn analyze(path: &Path, options: &AnalyzerOptions) -> Result<ComplexityResult> {
        let file_set = FileSet::new(path, options)?;
        let results: Vec<FileComplexity> = file_set
            .par_iter()
            .map(|file| analyze_file(file))
            .collect();

        Ok(ComplexityResult { files: results })
    }
}
```

This structure gives Omen several properties:

- **Composability.** The `all` command simply runs every analyzer and collects results. The `score` command runs a subset and feeds their outputs into a scoring function.
- **Independence.** Each analyzer owns its logic end-to-end. There are no shared mutable state concerns between analyzers.
- **Serialization.** Every result struct implements `Serialize`, so any analyzer's output can be rendered as JSON, table, or plain text through the same formatting layer.

## Parallelism with Rayon

File-level parallelism is handled by [rayon](https://docs.rs/rayon/latest/rayon/). When an analyzer processes a `FileSet`, it uses `par_iter()` to distribute file analysis across available CPU cores.

This means:

- A codebase with 1,000 files doesn't take 1,000x longer than a single file. Work is distributed across a thread pool.
- Individual file analysis is single-threaded (tree-sitter parsing is not thread-safe per parser instance), but many files are analyzed concurrently.
- The rayon thread pool is initialized once and shared across the process lifetime.

For most codebases, file-level parallelism is sufficient to saturate available cores. The bottleneck is typically I/O (reading files from disk) rather than CPU.

## Git Integration with gix

Several analyzers need repository history: hotspot detection, churn analysis, defect prediction, and score trend tracking. Omen uses [gix](https://docs.rs/gix/latest/gix/) (a pure-Rust Git implementation) to read commit history, diffs, and blame information.

Using gix rather than shelling out to `git` has two advantages:

- **No dependency on a system Git installation.** Omen works in minimal containers and CI environments where git may not be installed.
- **Structured access to Git data.** Commit metadata, file diffs, and blame output are available as Rust types, not strings to parse.

History-based analyzers read from the Git object database directly. They do not modify the working tree or create temporary branches.

## Flat CLI Structure

Omen uses a flat command structure where each analyzer is a top-level subcommand:

```
omen complexity
omen coupling
omen clones
omen score
omen search query "..."
```

There are no deeply nested subcommand hierarchies. Global flags (`-p` for path, `-f` for format, `--language` for filtering) are consistent across all commands. This keeps the CLI predictable: if you know the analyzer name, you know the command.

## Output Formatting

All commands support multiple output formats:

| Format | Flag | Use Case |
|---|---|---|
| Table | `-f table` (default) | Human-readable terminal output |
| JSON | `-f json` | CI/CD integration, scripting, piping to jq |
| CSV | `-f csv` | Spreadsheet import, data analysis |

The formatting layer receives serialized result structs and renders them. Analyzers are unaware of the output format -- they produce data, and the CLI layer handles presentation.
