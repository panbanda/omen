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
- **Halstead Metrics** - Software science measurements for effort and bug estimation

## What Each Analyzer Does

<details>
<summary><strong>Complexity Analysis</strong> - How hard your code is to understand and test</summary>

There are two types of complexity:

- **Cyclomatic Complexity** counts the number of different paths through your code. Every `if`, `for`, `while`, or `switch` creates a new path. A function with cyclomatic complexity of 10 means there are 10 different ways to run through it. The higher the number, the more test cases you need to cover all scenarios.

- **Cognitive Complexity** measures how hard code is for a human to read. It penalizes deeply nested code (like an `if` inside a `for` inside another `if`) more than flat code. Two functions can have the same cyclomatic complexity, but the one with deeper nesting will have higher cognitive complexity because it's harder to keep track of.

**Why it matters:** Research shows that complex code has more bugs and takes longer to fix. [McCabe's original 1976 paper](https://ieeexplore.ieee.org/document/1702388) found that functions with complexity over 10 are significantly harder to maintain. [SonarSource's cognitive complexity](https://www.sonarsource.com/docs/CognitiveComplexity.pdf) builds on this by measuring what actually confuses developers.

**Rule of thumb:** Keep cyclomatic complexity under 10 and cognitive complexity under 15 per function.

</details>

<details>
<summary><strong>Self-Admitted Technical Debt (SATD)</strong> - Comments where developers admit they took shortcuts</summary>

When developers write `TODO: fix this later` or `HACK: this is terrible but works`, they're creating technical debt and admitting it. Omen finds these comments and groups them by type:

| Category | Markers | What it means |
|----------|---------|---------------|
| Design | HACK, KLUDGE, SMELL | Architecture shortcuts that need rethinking |
| Defect | BUG, FIXME, BROKEN | Known bugs that haven't been fixed |
| Requirement | TODO, FEAT | Missing features or incomplete implementations |
| Test | FAILING, SKIP, DISABLED | Tests that are broken or turned off |
| Performance | SLOW, OPTIMIZE, PERF | Code that works but needs to be faster |
| Security | SECURITY, VULN, UNSAFE | Known security issues |

**Why it matters:** [Potdar and Shihab's 2014 study](https://ieeexplore.ieee.org/document/6976084) found that SATD comments often stay in codebases for years. The longer they stay, the harder they are to fix because people forget the context. [Maldonado and Shihab (2015)](https://ieeexplore.ieee.org/document/7180116) showed that design debt is the most common and most dangerous type.

**Rule of thumb:** Review SATD weekly. If a TODO is older than 6 months, either fix it or delete it.

</details>

<details>
<summary><strong>Dead Code Detection</strong> - Code that exists but never runs</summary>

Dead code includes:
- Functions that are never called
- Variables that are assigned but never used
- Classes that are never instantiated
- Code after a `return` statement that can never execute

**Why it matters:** Dead code isn't just clutter. It confuses new developers who think it must be important. It increases build times and binary sizes. Worst of all, it can hide bugs - if someone "fixes" dead code thinking it runs, they've wasted time. [Romano et al. (2020)](https://ieeexplore.ieee.org/document/9054810) found that dead code is a strong predictor of other code quality problems.

**Rule of thumb:** Delete dead code. Version control means you can always get it back if needed.

</details>

<details>
<summary><strong>Git Churn Analysis</strong> - How often files change over time</summary>

Churn looks at your git history and counts:
- How many times each file was modified
- How many lines were added and deleted
- Which files change together

Files with high churn are "hotspots" - they're constantly being touched, which could mean they're:
- Central to the system (everyone needs to modify them)
- Poorly designed (constant bug fixes)
- Missing good abstractions (features keep getting bolted on)

**Why it matters:** [Nagappan and Ball's 2005 research at Microsoft](https://www.microsoft.com/en-us/research/publication/use-of-relative-code-churn-measures-to-predict-system-defect-density/) found that code churn is one of the best predictors of bugs. Files that change a lot tend to have more defects. Combined with complexity data, churn helps you find the files that are both complicated AND frequently modified - your highest-risk code.

**Rule of thumb:** If a file has high churn AND high complexity, prioritize refactoring it.

</details>

<details>
<summary><strong>Code Clone Detection</strong> - Duplicated code that appears in multiple places</summary>

There are three types of clones:

| Type | Description | Example |
|------|-------------|---------|
| Type-1 | Exact copies (maybe different whitespace/comments) | Copy-pasted code |
| Type-2 | Same structure, different names | Same function with renamed variables |
| Type-3 | Similar code with some modifications | Functions that do almost the same thing |

**Why it matters:** When you fix a bug in one copy, you have to remember to fix all the other copies too. [Juergens et al. (2009)](https://ieeexplore.ieee.org/document/5069475) found that cloned code has significantly more bugs because fixes don't get applied consistently. The more clones you have, the more likely you'll miss one during updates.

**Rule of thumb:** Anything copied more than twice should probably be a shared function. Aim for duplication ratio under 5%.

</details>

<details>
<summary><strong>Defect Prediction</strong> - The likelihood that a file contains bugs</summary>

Omen combines multiple signals to predict defect probability:
- Complexity (complex code = more bugs)
- Churn (frequently changed code = more bugs)
- Size (bigger files = more bugs)
- Age (newer code = more bugs, counterintuitively)
- Coupling (code with many dependencies = more bugs)

Each file gets a risk score from 0% to 100%.

**Why it matters:** You can't review everything equally. [Menzies et al. (2007)](https://ieeexplore.ieee.org/document/4343755) showed that defect prediction helps teams focus testing and code review on the files most likely to have problems. [Rahman et al. (2014)](https://dl.acm.org/doi/10.1145/2597073.2597104) found that even simple models outperform random file selection for finding bugs.

**Rule of thumb:** Prioritize code review for files with >70% defect probability.

</details>

<details>
<summary><strong>Technical Debt Gradient (TDG)</strong> - A composite "health score" for each file</summary>

TDG combines multiple metrics into a single score (0-5 scale, lower is better):

| Component | Weight | What it measures |
|-----------|--------|------------------|
| Complexity | 30% | Cyclomatic and cognitive complexity |
| Churn | 35% | How often the file changes |
| Coupling | 15% | Dependencies on other modules |
| Domain Risk | 10% | Critical areas like auth, payments, crypto |
| Duplication | 10% | Amount of cloned code |

Scores are classified as:
- **Normal** (< 1.5): Healthy code
- **Warning** (1.5 - 2.5): Needs attention
- **Critical** (> 2.5): Prioritize for refactoring

**Why it matters:** Technical debt is like financial debt - a little is fine, too much kills you. [Cunningham coined the term in 1992](https://dl.acm.org/doi/10.1145/157709.157715), and [Kruchten et al. (2012)](https://ieeexplore.ieee.org/document/6225999) formalized how to measure and manage it. TDG gives you a single number to track over time and compare across files.

**Rule of thumb:** Fix critical TDG files before adding new features. Track average TDG over time - it should go down, not up.

</details>

<details>
<summary><strong>Dependency Graph</strong> - How your modules connect to each other</summary>

Omen builds a graph showing which files import which other files, then calculates:
- **PageRank**: Which files are most "central" (many things depend on them)
- **Betweenness**: Which files are "bridges" between different parts of the codebase
- **Coupling**: How interconnected modules are

**Why it matters:** Highly coupled code is fragile - changing one file breaks many others. [Parnas's 1972 paper on modularity](https://dl.acm.org/doi/10.1145/361598.361623) established that good software design minimizes dependencies between modules. The dependency graph shows you where your architecture is clean and where it's tangled.

**Rule of thumb:** Files with high PageRank should be especially stable and well-tested. Consider breaking up files that appear as "bridges" everywhere.

</details>

<details>
<summary><strong>Halstead Metrics</strong> - Software complexity based on operators and operands</summary>

[Maurice Halstead developed these metrics in 1977](https://ieeexplore.ieee.org/book/6276903) to measure programs like physical objects:

| Metric | Formula | What it means |
|--------|---------|---------------|
| Vocabulary | n1 + n2 | Unique operators + unique operands |
| Length | N1 + N2 | Total operators + total operands |
| Volume | N * log2(n) | Size of the implementation |
| Difficulty | (n1/2) * (N2/n2) | How hard to write and understand |
| Effort | Volume * Difficulty | Mental effort required |
| Time | Effort / 18 | Estimated coding time in seconds |
| Bugs | Effort^(2/3) / 3000 | Estimated number of bugs |

**Why it matters:** Halstead metrics give you objective measurements for comparing different implementations of the same functionality. They can estimate how long code took to write and predict how many bugs it might contain.

**Rule of thumb:** Use Halstead for comparing alternative implementations. Lower effort and predicted bugs = better.

</details>

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

# Analyze git churn (last 30 days)
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
churn_days = 30
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
