# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Development Commands

```bash
# Format, vet, and tidy dependencies
task tidy

# Build the CLI
go build -o omen ./cmd/omen

# Run the CLI
./omen --help

# Run tests
go test ./...

# Run a single test
go test ./pkg/analyzer -run TestComplexity

# Run tests with verbose output
go test -v ./pkg/analyzer/...
```

## Architecture

Omen is a multi-language code analysis CLI built in Go. It uses tree-sitter for parsing source code across 13 languages.

### Package Structure

- `cmd/omen/` - CLI entry point using urfave/cli/v2
- `pkg/parser/` - Tree-sitter wrapper for multi-language AST parsing
- `pkg/scanner/` - File discovery with configurable exclusion patterns
- `pkg/analyzer/` - Analysis implementations (complexity, SATD, dead code, churn, duplicates, defect prediction, TDG, dependency graph)
- `pkg/models/` - Data structures for analysis results
- `pkg/config/` - Configuration loading (TOML/YAML/JSON via koanf)
- `pkg/cache/` - Result caching with Blake3 hashing
- `pkg/output/` - Output formatting (text/JSON/markdown)

### Key Patterns

**Multi-language parsing**: The `pkg/parser/parser.go` contains `DetectLanguage()` which maps file extensions to tree-sitter parsers. Add new language support here by extending the switch statements for `GetTreeSitterLanguage()`, `getFunctionNodeTypes()`, and `getClassNodeTypes()`.

**Analyzer pattern**: Each analyzer in `pkg/analyzer/` follows the same structure:
1. Create analyzer with `NewXxxAnalyzer()`
2. Analyze single file with `AnalyzeFile(path)`
3. Analyze project with `AnalyzeProject(files)`
4. Close with `Close()` to release parser resources

**Configuration**: Config is loaded from `omen.toml`, `.omen.toml`, or `.omen/omen.toml` (also supports YAML/JSON). See `omen.example.toml` for all options.

### CLI Commands

Top-level commands:
- `analyze` / `a` - Run analyzers (all if no subcommand, or specific one)
- `context` / `ctx` - Deep context generation for LLMs

Analyzer subcommands (`omen analyze <subcommand>`):
- `complexity` / `cx` - Cyclomatic and cognitive complexity
- `satd` / `debt` - Self-admitted technical debt detection
- `deadcode` / `dc` - Unused code detection
- `churn` - Git history analysis for file churn
- `duplicates` / `dup` - Code clone detection
- `defect` / `predict` - Defect probability using PMAT weights
- `tdg` - Technical Debt Gradient scores
- `graph` / `dag` - Dependency graph (Mermaid output)
- `lint-hotspot` / `lh` - Lint violation density

All commands support `-f/--format` (text/json/markdown) and `-o/--output` flags.

## Supported Languages

Go, Rust, Python, TypeScript, JavaScript, TSX/JSX, Java, C, C++, C#, Ruby, PHP, Bash
