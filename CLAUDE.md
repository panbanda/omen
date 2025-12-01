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
go test ./internal/analyzer -run TestComplexity

# Run tests with verbose output
go test -v ./internal/analyzer/...
```

## Architecture

Omen is a multi-language code analysis CLI built in Go. It uses tree-sitter for parsing source code across 13 languages.

### Package Structure

**Public packages** (`pkg/`) - stable API for external consumers:
- `pkg/parser/` - Tree-sitter wrapper for multi-language AST parsing
- `pkg/models/` - Data structures for analysis results
- `pkg/config/` - Configuration loading (TOML/YAML/JSON via koanf)

**Internal packages** (`internal/`) - implementation details:
- `internal/analyzer/` - Analysis implementations (complexity, SATD, dead code, churn, duplicates, defect prediction, TDG, dependency graph, feature flags)
- `internal/analyzer/featureflags/` - Feature flag detection with tree-sitter queries (queries stored in `queries/<lang>/<provider>.scm`)
- `internal/fileproc/` - Concurrent file processing utilities
- `internal/scanner/` - File discovery with configurable exclusion patterns
- `internal/cache/` - Result caching with Blake3 hashing
- `internal/output/` - Output formatting (text/JSON/markdown/toon)
- `internal/progress/` - Progress bars and spinners
- `internal/mcpserver/` - MCP server implementation with tools and prompts
- `internal/service/` - High-level service layer coordinating analyzers
- `internal/vcs/` - Git operations (blame, log, diff)

**CLI** (`cmd/omen/`) - Entry point using urfave/cli/v2

### Key Patterns

**Multi-language parsing**: `pkg/parser/parser.go` contains `DetectLanguage()` which maps file extensions to tree-sitter parsers. Add new language support by extending `GetTreeSitterLanguage()`, `getFunctionNodeTypes()`, and `getClassNodeTypes()`.

**Analyzer pattern**: Each analyzer in `internal/analyzer/` follows the same structure:
1. Create analyzer with `NewXxxAnalyzer()`
2. Analyze single file with `AnalyzeFile(path)`
3. Analyze project with `AnalyzeProject(files)` or `AnalyzeProjectWithProgress(files, progressFn)`
4. Close with `Close()` to release parser resources

**Concurrent file processing**: `internal/fileproc/` provides generic parallel processing:
- `MapFiles[T]` - For AST-based analyzers (parser provided per worker)
- `ForEachFile[T]` - For non-AST operations (e.g., SATD regex scanning)
- Both use 2x NumCPU workers, optimal for mixed I/O and CGO workloads

**Configuration**: Config loaded from `omen.toml`, `.omen.toml`, or `.omen/omen.toml`. See `omen.example.toml` for all options.

**Tree-sitter queries**: Feature flag detection uses `.scm` query files in `internal/analyzer/featureflags/queries/<lang>/<provider>.scm`. Queries must capture `@flag_key` for the flag identifier. Predicates like `#match?` and `#eq?` must be placed inline within patterns, and `FilterPredicates()` must be called to evaluate them.

**MCP server**: Tools are registered in `internal/mcpserver/mcpserver.go`. Each tool has a description in `descriptions.go`. Prompts are stored as markdown files in `internal/mcpserver/prompts/` using `go:embed`.

**Skills**: Claude Code skills live in `skills/<skill-name>/SKILL.md` and are installed via `/plugin install panbanda/omen`.

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
