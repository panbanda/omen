# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Development Commands

```bash
# Setup git hooks (run once after clone)
lefthook install

# Format code
cargo fmt

# Run linter
cargo clippy --all-targets --all-features -- -D warnings

# Run all tests
cargo test

# Run tests with coverage
cargo llvm-cov --all-features --ignore-filename-regex 'main\.rs$'

# Build release binary
cargo build --release

# Run a single test
cargo test test_complexity_simple

# Run tests for a specific module
cargo test analyzers::complexity
```

## Architecture

Omen is a multi-language code analysis CLI built in Rust. It uses tree-sitter for parsing source code across 13 languages.

### Module Structure

```
src/
  cli/           - CLI entry point using clap
  config/        - Configuration loading and schema
  core/          - Core types and traits
  analyzers/     - Analysis implementations
    complexity.rs - Cyclomatic and cognitive complexity
    satd.rs       - Self-admitted technical debt
    deadcode.rs   - Unused code detection
    churn.rs      - Git history file churn
    duplicates.rs - Code clone detection (MinHash+LSH)
    defect.rs     - Defect probability (PMAT)
    changes.rs    - JIT commit-level risk
    tdg.rs        - Technical Debt Gradient
    graph.rs      - Dependency graph (Mermaid)
    hotspot.rs    - High churn + complexity
    temporal.rs   - Temporal coupling
    ownership.rs  - Code ownership and bus factor
    cohesion.rs   - CK metrics (WMC, CBO, RFC, LCOM4, DIT, NOC)
    repomap.rs    - PageRank-ranked symbols
    outline.rs    - File and symbol outlines
    impact.rs     - Symbol blast-radius analysis
    smells.rs     - Architectural smells (Tarjan SCC)
    flags.rs      - Feature flag detection
    mutation/    - Mutation testing (21 operators, parallel execution)
  semantic/      - TF-IDF indexing, search, cache, and multi-repo support
  context.rs     - Agent-oriented repository context
  symbol.rs      - Symbol lookup and relationship reports
  git/           - Git operations (log, blame, diff)
  parser/        - Tree-sitter wrapper
  mcp/           - MCP server for LLM integration
  output/        - Output formatting (JSON/Markdown/text)
  report/        - minijinja report rendering and embedded HTML template
  score/         - Repository health scoring
```

### Key Patterns

**Analyzer pattern**: Each analyzer module follows the same structure:
1. Public `analyze()` function taking path and options
2. Returns a result struct with analysis data
3. Implements `Serialize` for JSON output
4. Uses rayon for parallel file processing

**Multi-language parsing**: `parser/mod.rs` contains `Language` enum and `Parser` struct. Add new language support by:
1. Adding variant to `Language` enum
2. Implementing tree-sitter grammar in `parser()`
3. Adding node types in extraction functions

**Concurrent file processing**: Uses rayon's parallel iterators:
```rust
files.par_iter()
    .filter_map(|path| analyze_file(path).ok())
    .collect()
```

**Configuration**: Automatic discovery loads TOML from `omen.toml` and `.omen/omen.toml`. An explicit `--config` path supports TOML, YAML, or JSON. Environment variables with the `OMEN_` prefix override file values. Config types use `#[serde(deny_unknown_fields)]`, so unknown keys are errors. See `omen.example.toml` for a representative configuration.

**MCP server**: JSON-RPC server in `mcp/` module exposing all analyzers as tools for LLM integration. Tool names are bare analyzer names (e.g., `complexity`, `satd`, `temporal`, `outline`, `impact`, `get_symbol`) -- no prefix. All tools support `limit`/`offset` envelope pagination (default limit: 50). `McpServer::tool_names()` is the single source of truth; the manifest reads from it.

**`--since` flag**: `score trend --since` defaults to `"all"`; `report generate --since` defaults to `"1y"`. The value `"all"` is handled by `is_since_all()` in `src/git/log.rs`, which causes `parse_since_to_days()` to return `None` (no time limit). Duration values like `3m`, `6m`, and `1y` still work.

### CLI Commands

Top-level commands (flat structure):
- `complexity` - Cyclomatic and cognitive complexity
- `satd` - Self-admitted technical debt
- `deadcode` - Unused code detection
- `churn` - Git history file churn
- `clones` - Code clone detection
- `defect` - Defect probability prediction
- `changes` - Commit-level change risk (JIT)
- `diff` - Branch diff risk analysis
- `tdg` - Technical Debt Gradient
- `graph` - Dependency graph
- `hotspot` - High churn + complexity files
- `temporal` - Temporal coupling
- `ownership` - Code ownership and bus factor
- `cohesion` - CK object-oriented metrics
- `repomap` - PageRank-ranked symbol map
- `smells` - Architectural smell detection
- `flags` - Feature flag detection
- `mutation` - Mutation testing (21 operators across 5 languages)
- `score` - Repository health score
- `all` - Run all analyzers
- `context` - Deep context for LLMs
- `outline` - Token-cheap file map: imports, classes, top-level functions
- `impact` - Blast-radius analysis for a symbol (transitive callers/callees)
- `symbol` - One-call symbol report: source, location, callers/callees, complexity
- `report` - HTML health reports
- `search` - Semantic symbol search (`index` and `query`)
- `mcp` - Start MCP server

**Global options**: `-p/--path`, `-f/--format`, `-c/--config`, `-v/--verbose`, `-j/--jobs`, `--ref`, and `--shallow`. `--compact` is a clap global flag, so it may appear before or after a subcommand; with `-f json` it emits minified single-line JSON and is ignored for other formats.

**Pagination flags** (most analyzers): `--top N` (limit to N results), `--offset N` (skip first N results). Combine for pagination.

**Special safety flags**: `deadcode --cargo-check` executes `cargo check`, including build scripts, and is for trusted repositories only. Mutation testing refuses dirty working trees unless `mutation --allow-dirty` is supplied. MCP transport is stdio-only; `mcp --allow-external-paths` permits tool paths outside the configured repository root.

**MCP server**: `McpServer::tool_names()` defines the complete tool list: `context`, `outline`, `complexity`, `satd`, `deadcode`, `churn`, `clones`, `defect`, `changes`, `diff`, `tdg`, `graph`, `hotspot`, `temporal`, `ownership`, `cohesion`, `repomap`, `smells`, `flags`, `score`, `semantic_search`, `get_symbol`, `impact`, and `semantic_search_hyde`. Every tool honors its advertised input schema and supports `limit`/`offset` envelope pagination (default limit: 50). JSON tool output is compact.

### Report System

`omen report generate` runs the report analyzers and writes JSON data. The reporting plugin can then invoke analyst agents for narratives before `omen report render` renders the HTML with minijinja.

Key files:
- `src/report/render.rs` -- loads JSON data files + optional insight JSON files, renders HTML
- `src/report/types.rs` -- all Rust types for report data and insights (must match agent output schemas)
- `src/report/template.html` -- minijinja (Jinja2 syntax) HTML template
- `assets/report/input.css` -- Tailwind v4 + shadcn design-token source for the report stylesheet
- `src/report/report.css` -- stylesheet compiled from `assets/report/input.css`, embedded into the template as `ReportCss`
- `plugins/reporting/commands/generate-report.md` -- orchestration command (runs analyzers, spawns agents)
- `plugins/reporting/agents/` -- 12 analyst agents that produce `{section_insight: string}` JSON

When adding a new report section: add the insight type to `types.rs`, add a field to `RenderData`, add loading logic in `render.rs`, add the template section in `template.html`, and create an analyst agent.

The report stylesheet is compiled offline from `assets/report/input.css` to `src/report/report.css` (see `assets/report/README.md`; run `bun run build` after editing `input.css`), and both `report.css` and `template.html` are embedded with `include_str!`. The report is a single self-contained HTML file, and ordinary `cargo build` and `cargo install` do not require a JavaScript toolchain.

### Plugin Structure

```
plugins/
  development/
    skills/       - Claude Code skills (workflows using CLI commands)
  reporting/
    agents/       - LLM analyst agents for report insights
    commands/     - Orchestration commands (e.g., generate-report)
```

Skills use CLI commands (`omen -f json <analyzer>`) not MCP tools. Each skill has a `SKILL.md` with a YAML frontmatter block (`name`, `description`) and a workflow section.

## Development Workflow

**Always use Test-Driven Development (TDD):**

1. RED: Write a failing test first
2. Verify the test fails for the expected reason
3. GREEN: Write minimal code to pass the test
4. Verify all tests pass
5. REFACTOR: Clean up while keeping tests green

No production code without a failing test first.

### Test Organization

Tests are co-located with source files using `#[cfg(test)]` modules:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature() {
        // ...
    }
}
```

Integration tests live in `tests/` directory.

### Coverage Requirements

- Minimum 80% line coverage enforced by CI
- Run `cargo llvm-cov` to check coverage locally
- Coverage report excludes `main.rs`

### Pull Request Requirements

**Performance PRs**: When submitting a PR that claims performance improvements, include before/after benchmarks in the PR description. Run the old version and new version on a representative dataset and document the timing difference.

## Accuracy Requirements

Omen is a code analysis tool where accuracy is paramount. When making changes:

- **No sampling or approximation**: Analyze all requested data points. Do not skip, sample, or approximate to improve performance.
- **No guessing**: Use actual data from git history and file analysis. Do not estimate or infer values.
- **Deterministic results**: The same input must always produce the same output.
- **Performance through parallelization**: Improve speed by doing work in parallel, not by doing less work.

## Supported Languages

Go, Rust, Python, TypeScript, JavaScript, TSX/JSX, Java, C, C++, C#, Ruby, PHP, Bash

### Multi-language requirements

Any analyzer feature that operates on source code must be implemented and tested for all supported languages where the feature applies. This includes:

- **TDG critical defect detection**: Each language defines its own dangerous patterns detected via tree-sitter AST (not string matching). Tests must cover both detection of real calls and rejection of false positives in string literals/comments.
- **Complexity analysis**: Decision-point node types must be defined per language in `parser/queries.rs`.
- **Dead code detection**: Language-specific entry points, visibility rules, and export conventions must be handled.
- **Cohesion (CK metrics)**: Class-equivalent constructs must be defined per language (e.g. Rust struct+impl, Go struct+methods).

When adding a new language feature, write tests for every supported language before implementing.
