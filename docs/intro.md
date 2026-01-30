---
sidebar_position: 1
slug: /
---

# Introduction

Omen is a multi-language code analysis CLI built in Rust. It uses tree-sitter to parse source code across 13 languages and runs 19 analyzers that surface complexity, technical debt, defect risk, code clones, dependency problems, and more.

## Why Omen Exists

AI is writing more code than ever. Copilots, chat assistants, and autonomous agents generate functions, modules, and entire features at a pace that outstrips most teams' ability to review what's being produced. The code compiles. The tests pass. But nobody checked whether the new function doubled the cyclomatic complexity of a critical module, introduced a dependency cycle, or duplicated logic that already exists three directories away.

Omen exists because AI writes code without knowing where the landmines are. It gives both humans and AI assistants a structured, quantitative view of codebase health -- the kind of information that's difficult to see by reading diffs and easy to miss in review.

## The Name

An omen is a sign of things to come. Code analysis is the same idea applied to software: patterns in the code today predict the bugs, slowdowns, and maintenance burdens of tomorrow. High complexity, tight coupling, self-admitted technical debt, and duplicated logic are all omens. This tool surfaces them before they become incidents.

## Key Capabilities

### 19 Analyzers

Omen ships with a broad set of analyzers covering structural, historical, and predictive dimensions of code quality:

| Category | Analyzers |
|---|---|
| Structural | Complexity, coupling, cohesion, code smells, dead code |
| Duplication | Code clone detection (Type 1, 2, and 3 clones) |
| Technical debt | SATD detection, technical debt gradient, dependency analysis |
| Predictive | Defect prediction, hotspot detection, churn analysis |
| Testing | Mutation testing, coverage analysis |
| Composite | Repository score, dependency graph |
| Discovery | Semantic search, file statistics |

### 13 Languages

Go, Rust, Python, TypeScript, JavaScript, TSX, JSX, Java, C, C++, C#, Ruby, PHP, and Bash. Language detection is automatic. All parsing is done through tree-sitter grammars, so analysis is syntax-aware rather than regex-based.

### MCP Server Integration

Omen includes a built-in MCP (Model Context Protocol) server, allowing LLMs and AI assistants to query code analysis results directly. This turns Omen into a tool that AI agents can call during planning, code generation, and review workflows.

### Semantic Search

Natural language code discovery powered by local vector embeddings. Omen extracts symbols from the codebase, generates embeddings using all-MiniLM-L6-v2 via the candle inference library, and indexes them in a local SQLite database. No API keys required for the default provider.

### Mutation Testing

Omen can inject controlled mutations into source code and run your test suite to evaluate test effectiveness. This goes beyond line coverage to answer the harder question: would your tests actually catch a bug here?

### Remote Repository Scanning

Analyze any public Git repository without cloning it manually. Omen handles the clone, runs the analysis, and cleans up. Useful for evaluating dependencies, open source libraries, or repositories you don't have checked out locally.

## Who It's For

**Developers using AI assistants.** If you're generating code with Copilot, Claude, or similar tools, Omen gives you a fast way to check what the AI produced against structural quality metrics. Pair it with MCP integration to give your AI assistant direct access to codebase health data.

**Teams tracking code health.** Omen's repository score provides a single composite metric (0-100) that can be tracked over time, broken down by component, and used in sprint retrospectives or architecture reviews.

**CI/CD quality gates.** Run `omen score` in your pipeline and fail the build if the score drops below a threshold. Omen's JSON output integrates with any CI system, and its exit codes support gate logic directly.
