# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Development Commands

```bash
# Setup git hooks (run once after clone)
task setup

# Format, vet, and tidy dependencies
task tidy

# Run linter
task lint

# Run all checks and tests
task test

# Build the CLI
go build -o omen ./cmd/omen

# Run a single test
go test ./pkg/analyzer/complexity -run TestComplexity

# Run tests with verbose output
go test -v ./pkg/analyzer/...

# Run tests with coverage
go test -coverprofile=/tmp/cover.out ./pkg/analyzer/complexity/... && go tool cover -func=/tmp/cover.out
```

## Architecture

Omen is a multi-language code analysis CLI built in Go. It uses tree-sitter for parsing source code across 13 languages.

### Package Structure

**Public packages** (`pkg/`) - stable API for external consumers:
- `pkg/parser/` - Tree-sitter wrapper for multi-language AST parsing
- `pkg/config/` - Configuration loading (TOML/YAML/JSON via koanf)
- `pkg/analyzer/<name>/` - Each analyzer is a self-contained package with co-located types

**Analyzer packages** (`pkg/analyzer/`):
- `changes/` - JIT commit-level change risk (Kamei et al. 2013)
- `churn/` - Git history file churn analysis
- `cohesion/` - CK object-oriented metrics (LCOM, WMC, CBO, DIT)
- `complexity/` - Cyclomatic and cognitive complexity
- `deadcode/` - Unused code detection
- `defect/` - File-level defect probability (PMAT weights)
- `duplicates/` - Code clone detection
- `featureflags/` - Feature flag detection with tree-sitter queries
- `graph/` - Dependency graph generation
- `hotspot/` - High churn + high complexity files
- `ownership/` - Code ownership and bus factor
- `repomap/` - PageRank-weighted symbol map
- `satd/` - Self-admitted technical debt detection
- `smells/` - Architectural smell detection (cycles, hubs, god components)
- `tdg/` - Technical Debt Gradient scores
- `temporal/` - Temporal coupling analysis

**Internal packages** (`internal/`) - implementation details:
- `fileproc/` - Concurrent file processing utilities
- `scanner/` - File discovery with configurable exclusion patterns
- `cache/` - Result caching with Blake3 hashing
- `output/` - Output formatting (text/JSON/markdown/toon)
- `progress/` - Progress bars and spinners
- `mcpserver/` - MCP server implementation with tools and prompts
- `service/` - High-level service layer coordinating analyzers
- `vcs/` - Git operations (blame, log, diff)
- `semantic/` - Language-aware semantic extraction for indirect function references (callbacks, decorators, dynamic dispatch)

**CLI** (`cmd/omen/`) - Entry point using spf13/cobra with persistent flag inheritance

### Key Patterns

**Analyzer pattern**: Each analyzer in `pkg/analyzer/<name>/` follows the same structure:
1. Create analyzer with `New()` plus functional options
2. Analyze single file with `AnalyzeFile(path)`
3. Analyze project with `AnalyzeProject(path)` or `AnalyzeProjectWithProgress(path, progressFn)`
4. Close with `Close()` to release parser resources
5. Types are co-located in the same package (no separate models package)

**Multi-language parsing**: `pkg/parser/parser.go` contains `DetectLanguage()` which maps file extensions to tree-sitter parsers. Add new language support by extending `GetTreeSitterLanguage()`, `getFunctionNodeTypes()`, and `getClassNodeTypes()`.

**Concurrent file processing**: `internal/fileproc/` provides generic parallel processing:
- `MapFiles[T]` - For AST-based analyzers (parser provided per worker)
- `ForEachFile[T]` - For non-AST operations (e.g., SATD regex scanning)
- Both use 2x NumCPU workers, optimal for mixed I/O and CGO workloads

**Configuration**: Config loaded from `omen.toml` or `.omen/omen.toml`. See `omen.example.toml` for all options.

**Tree-sitter queries**: Feature flag detection uses `.scm` query files in `pkg/analyzer/featureflags/queries/<lang>/<provider>.scm`. Queries must capture `@flag_key` for the flag identifier. Predicates like `#match?` and `#eq?` must be placed inline within patterns, and `FilterPredicates()` must be called to evaluate them.

**Semantic extractors**: `internal/semantic/` provides language-aware extraction of indirect function references that bypass normal call graphs. Each language has a dedicated extractor (Go, Ruby, TypeScript) with embedded tree-sitter queries. Extractors return `[]Ref` where each `Ref` has a `Name` and `Kind` (RefCallback, RefDecorator, RefFunctionValue, RefDynamicCall). Used by the deadcode analyzer to reduce false positives for framework patterns.

**MCP server**: Tools are registered in `internal/mcpserver/mcpserver.go`. Each tool has a description in `descriptions.go`. Prompts are stored as markdown files in `internal/mcpserver/prompts/` using `go:embed`.

**Skills**: Claude Code skills live in `skills/<skill-name>/SKILL.md`. Register the repository as a plugin marketplace with `/plugin marketplace add panbanda/omen`, then install skills with `/plugin install <skill-name>@omen` (e.g., `/plugin install find-bugs@omen`).

### CLI Commands

Top-level commands:
- `analyze` / `a` - Run analyzers (all if no subcommand, or specific one)
- `context` / `ctx` - Deep context generation for LLMs
- `mcp` - Start MCP server for LLM tool integration

Analyzer subcommands (`omen analyze <subcommand>`):
- `complexity` / `cx` - Cyclomatic and cognitive complexity
- `satd` / `debt` - Self-admitted technical debt detection
- `deadcode` / `dc` - Unused code detection
- `churn` - Git history analysis for file churn
- `duplicates` / `dup` - Code clone detection
- `defect` / `predict` - File-level defect probability (PMAT weights)
- `changes` / `jit` - Commit-level change risk analysis (Kamei et al. 2013)
- `diff` / `pr` - Branch diff risk analysis for PR review
- `tdg` - Technical Debt Gradient scores
- `graph` / `dag` - Dependency graph (Mermaid output)
- `hotspot` / `hs` - High churn + high complexity files
- `smells` - Architectural smell detection (cycles, hubs, god components)
- `temporal-coupling` / `tc` - Files that change together
- `ownership` / `own` / `bus-factor` - Code ownership and bus factor
- `cohesion` / `ck` - CK object-oriented metrics
- `lint-hotspot` / `lh` - Lint violation density
- `flags` / `ff` - Feature flag detection and staleness analysis

**Global flags**: `--config`, `--verbose`, `--pprof`

**Command flags** (on `analyze` and subcommands): `-f/--format` (text/json/markdown/toon), `-o/--output`, `--no-cache`

## Supported Languages

Go, Rust, Python, TypeScript, JavaScript, TSX/JSX, Java, C, C++, C#, Ruby, PHP, Bash
