---
sidebar_position: 21
---

# Supported Languages

Omen supports 13 programming languages through tree-sitter grammars. Language detection is automatic based on file extension. All parsing is syntax-aware -- analyzers operate on AST nodes, not text patterns.

## Languages and File Extensions

| Language | Extensions |
|----------|------------|
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

Files with unrecognized extensions are silently skipped. There is no heuristic-based detection.

## Filtering by Language

Use the `--language` flag to restrict analysis to a single language:

```bash
omen complexity --language rust
omen clones --language typescript
omen satd --language python
```

## Analyzer Coverage by Language

Not all analyzers apply equally to all languages. Some analyzers are universal (they work on any language with a tree-sitter grammar), while others have language-specific behavior or are limited to a subset of languages.

### Coverage Matrix

| Analyzer | Go | Rust | Python | TS | JS | TSX/JSX | Java | C | C++ | C# | Ruby | PHP | Bash |
|----------|:--:|:----:|:------:|:--:|:--:|:-------:|:----:|:-:|:---:|:--:|:----:|:---:|:----:|
| Complexity | x | x | x | x | x | x | x | x | x | x | x | x | x |
| SATD | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Dead Code | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Churn | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Code Clones | x | x | x | x | x | x | x | x | x | x | x | x | x |
| TDG | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Hotspots | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Repo Map | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Smells | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Defect Prediction | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Change Risk | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Ownership | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Temporal Coupling | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Diff Analysis | x | x | x | x | x | x | x | x | x | x | x | x | x |
| Semantic Search | x | x | x | x | x | x | x | x | x | x | x | x | x |
| CK Metrics | - | - | x | - | - | - | x | - | x | x | x | - | - |
| Dependency Graph | x | x | x | x | - | - | x | - | - | - | x | - | - |
| Mutation Testing | x | x | - | x | - | - | - | - | - | - | x | - | - |
| Feature Flags | - | - | x | x | x | x | - | - | - | - | x | - | - |

Legend: **x** = supported, **-** = not applicable or not yet implemented.

### Universal Analyzers

The following analyzers work identically across all 13 languages. They rely on general tree-sitter node types (functions, classes, comments, blocks) or on Git history rather than language-specific constructs:

- **Complexity** -- cyclomatic and cognitive complexity, nesting depth
- **SATD** -- comment scanning for technical debt markers
- **Dead Code** -- unreachable functions and unused exports
- **Churn** -- file change frequency from Git history
- **Code Clones** -- token-based duplicate detection
- **TDG** -- technical debt gradient (composite of multiple signals)
- **Hotspots** -- intersection of complexity and churn
- **Repository Map** -- structural map of modules and symbols
- **Smells** -- architectural smell detection
- **Defect Prediction** -- risk scoring based on complexity, churn, and ownership
- **Change Risk** -- risk analysis of recent modifications
- **Ownership** -- contributor distribution from Git blame
- **Temporal Coupling** -- co-change detection from Git history
- **Diff Analysis** -- structural analysis of uncommitted changes
- **Semantic Search** -- embedding-based code discovery

### CK Metrics (Cohesion)

CK metrics (Chidamber-Kemerer) are object-oriented metrics that measure class-level properties: weighted methods per class (WMC), coupling between objects (CBO), response for a class (RFC), lack of cohesion (LCOM4), depth of inheritance tree (DIT), and number of children (NOC).

These metrics are meaningful only for languages with class-based OO semantics:

- **Python** -- classes defined with `class`
- **Java** -- classes, interfaces, abstract classes
- **C++** -- classes and structs with methods
- **C#** -- classes, structs, interfaces
- **Ruby** -- classes and modules

Languages without class constructs (Go, C, Bash) or where classes are uncommon in idiomatic usage (Rust traits/structs without inheritance, PHP historically procedural) are not analyzed for CK metrics.

### Dependency Graph

The dependency graph analyzer parses import and module statements to build a directed graph of file dependencies. This requires language-specific understanding of the import system:

- **Go** -- `import` statements
- **Python** -- `import` and `from ... import` statements
- **TypeScript** -- `import` and `require` statements
- **Rust** -- `use` and `mod` statements
- **Java** -- `import` statements
- **Ruby** -- `require` and `require_relative` statements

Languages where the import mechanism is not yet implemented (C/C++ `#include`, C# `using`, PHP `use`/`require`, JavaScript without TypeScript, Bash `source`) are excluded from dependency graph analysis.

### Mutation Testing

Mutation testing injects controlled modifications into source code and runs the project's test suite to evaluate test effectiveness. The mutations are language-specific, targeting operators and constructs that are syntactically and semantically meaningful in each language:

| Language | Operators | Description |
|----------|-----------|-------------|
| **Rust** | 3 | Boundary mutations (`<` to `<=`), negation removal, return value substitution |
| **Go** | 2 | Boundary mutations, conditional negation |
| **TypeScript** | 2 | Boundary mutations, boolean literal flipping |
| **Python** | 2 | Comparison operator swaps, boolean negation |
| **Ruby** | 2 | Comparison operator swaps, conditional negation |

Mutation testing requires a working test suite and a test command. It is not currently supported for C, C++, C#, Java, PHP, JavaScript (without TypeScript), JSX, TSX, or Bash.

### Feature Flag Detection

Feature flag detection identifies usage of feature flag SDKs and custom flag patterns in code. Detection is provider-specific and currently supports:

| Language | Providers |
|----------|-----------|
| **JavaScript/TypeScript/TSX/JSX** | LaunchDarkly (`ldclient`, `@launchdarkly/node-server-sdk`), Split (`@splitsoftware/splitio`), Unleash (`unleash-client`) |
| **Python** | Unleash (`UnleashClient`) |
| **Ruby** | Flipper (`Flipper.enabled?`), environment variable flags (`ENV["FEATURE_*"]`, `ENV.fetch("FEATURE_*")`) |

Additional providers can be defined through custom tree-sitter queries in the configuration file. See [Configuration](./configuration.md) for details on the `[feature_flags.custom_providers]` section.

## Adding Language Support

Omen's language support is determined by its compiled tree-sitter grammars. Adding a new language requires adding the corresponding tree-sitter grammar crate as a dependency, implementing the extension-to-grammar mapping, and ensuring the universal analyzers handle the new grammar's node types correctly. Language-specific analyzers (dependency graph, mutation testing, feature flags) require additional per-language implementation.
