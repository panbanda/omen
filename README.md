# Omen

<p align="center">
  <img src="assets/omen-logo.png" alt="Omen - Code Analysis CLI" width="100%">
</p>

[![Go Version](https://img.shields.io/github/go-mod/go-version/panbanda/omen)](https://go.dev/)
[![License](https://img.shields.io/github/license/panbanda/omen)](https://github.com/panbanda/omen/blob/main/LICENSE)
[![CI](https://github.com/panbanda/omen/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/panbanda/omen/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/panbanda/omen/graph/badge.svg)](https://codecov.io/gh/panbanda/omen)
[![Go Report Card](https://goreportcard.com/badge/github.com/panbanda/omen)](https://goreportcard.com/report/github.com/panbanda/omen)
[![Release](https://img.shields.io/github/v/release/panbanda/omen)](https://github.com/panbanda/omen/releases)
[![Go Reference](https://pkg.go.dev/badge/github.com/panbanda/omen.svg)](https://pkg.go.dev/github.com/panbanda/omen)
[![Snyk Security](https://snyk.io/test/github/panbanda/omen/badge.svg)](https://snyk.io/test/github/panbanda/omen)

A multi-language code analysis CLI built in Go. Omen uses tree-sitter for parsing source code across 13 languages, providing insights into complexity, technical debt, code duplication, and defect prediction.

**Why "Omen"?** An omen is a sign of things to come - good or bad. Your codebase is full of omens: low complexity and clean architecture signal smooth sailing ahead, while high churn, technical debt, and code clones warn of trouble brewing. Omen surfaces these signals so you can act before that "temporary fix" celebrates its third anniversary in production.

## Features

- **Cyclomatic & Cognitive Complexity** - Measure code complexity at function and file levels
- **Self-Admitted Technical Debt (SATD)** - Detect TODO, FIXME, HACK markers and classify severity
- **Dead Code Detection** - Find unused functions and variables
- **Git Churn Analysis** - Identify frequently changed files
- **Code Clone Detection** - Find duplicated code blocks (Type-1, Type-2, Type-3 clones)
- **Defect Prediction** - Predict defect probability using PMAT weights
- **Technical Debt Gradient (TDG)** - Composite scoring for prioritizing refactoring
- **Dependency Graph** - Generate Mermaid diagrams of module dependencies

## Supported Languages

Go, Rust, Python, TypeScript, JavaScript, TSX/JSX, Java, C, C++, C#, Ruby, PHP, Bash

## Installation

### Homebrew (macOS/Linux)

```bash
brew install panbanda/omen/omen
```

### Go Install

```bash
go install github.com/panbanda/omen/cmd/omen@latest
```

### Download Binary

Download pre-built binaries from the [releases page](https://github.com/panbanda/omen/releases).

### Build from Source

```bash
git clone https://github.com/panbanda/omen.git
cd omen
go build -o omen ./cmd/omen
```

## Quick Start

```bash
# Run all analyzers
omen analyze ./src

# Analyze complexity
omen analyze complexity ./src

# Detect technical debt
omen analyze satd ./src

# Find dead code
omen analyze deadcode ./src

# Analyze git churn (last 90 days)
omen analyze churn ./

# Detect code clones
omen analyze duplicates ./src

# Predict defect probability
omen analyze defect ./src

# Calculate TDG scores
omen analyze tdg ./src

# Generate dependency graph
omen analyze graph ./src --metrics
```

## Commands

### Top-level Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `analyze` | `a` | Run analyzers (all if no subcommand, or specific one) |
| `context` | `ctx` | Deep context generation for LLMs |

### Analyzer Subcommands (`omen analyze <subcommand>`)

| Subcommand | Alias | Description |
|------------|-------|-------------|
| `complexity` | `cx` | Cyclomatic and cognitive complexity analysis |
| `satd` | `debt` | Self-admitted technical debt detection |
| `deadcode` | `dc` | Unused code detection |
| `churn` | - | Git history analysis for file churn |
| `duplicates` | `dup` | Code clone detection |
| `defect` | `predict` | Defect probability prediction |
| `tdg` | - | Technical Debt Gradient scores |
| `graph` | `dag` | Dependency graph (Mermaid output) |
| `lint-hotspot` | `lh` | Lint violation density |

## Output Formats

All commands support multiple output formats:

```bash
omen analyze complexity ./src -f text      # Default, colored terminal output
omen analyze complexity ./src -f json      # JSON for programmatic use
omen analyze complexity ./src -f markdown  # Markdown tables
omen analyze complexity ./src -f toon      # TOON format
```

Write output to a file:

```bash
omen analyze ./src -f json -o report.json
```

## Configuration

Create `omen.toml`, `.omen.toml`, or `.omen/omen.toml`:

```toml
[exclude]
patterns = ["vendor/**", "node_modules/**", "**/*_test.go"]
dirs = [".git", "dist", "build"]

[thresholds]
cyclomatic = 10
cognitive = 15
duplicate_min_lines = 6
duplicate_similarity = 0.8
dead_code_confidence = 0.8

[analysis]
churn_days = 90
```

See [`omen.example.toml`](omen.example.toml) for all options.

## Examples

### Find Complex Functions

```bash
omen analyze complexity ./pkg --functions-only --cyclomatic-threshold 15
```

### High-Risk Files Only

```bash
omen analyze defect ./src --high-risk-only
```

### Top 5 TDG Hotspots

```bash
omen analyze tdg ./src --hotspots 5
```

### Generate LLM Context

```bash
omen context ./src --include-metrics --include-graph
```

## Contributing

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -am 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Create a Pull Request

## Acknowledgments

Omen draws heavy inspiration from [paiml-mcp-agent-toolkit](https://github.com/paiml/paiml-mcp-agent-toolkit/) - a fantastic CLI and comprehensive suite of code analysis tools for LLM workflows. If you're doing serious AI-assisted development, it's worth checking out. Omen exists as a streamlined alternative for teams who want a focused subset of analyzers without the additional dependencies. If you're looking for a Rust-focused MCP/agent generator as an alternative to Python, it's definitely worth checking out.

## License

MIT License - see [LICENSE](LICENSE) for details.
