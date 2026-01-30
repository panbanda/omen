---
sidebar_position: 16
---

# Repository Map

The repository map analyzer produces a compact structural index of the codebase, listing every significant symbol (function, class, method, interface, type) ranked by structural importance using PageRank. It is designed to fit within LLM context windows, providing an AI assistant with a high-level map of the codebase without requiring it to read every file.

## How It Works

Omen parses every source file with tree-sitter and extracts symbol definitions: functions, methods, classes, interfaces, structs, traits, enums, type aliases, and constants. For each symbol, it records:

- **Name and kind** (function, class, method, interface, etc.)
- **File location and line number**
- **Signature** (parameters and return type where available)
- **In-degree and out-degree** (how many symbols reference it and how many it references)

These symbols and their reference relationships form a directed graph. Omen runs PageRank on this graph to assign each symbol an importance score. The output is sorted by PageRank score, placing the most structurally important symbols first.

## Command

```bash
omen repomap
```

### Common Options

```bash
# Analyze a specific directory
omen -p ./src repomap

# JSON output
omen -f json repomap

# Filter by language
omen repomap --language go

# Remote repository
omen -p gin-gonic/gin repomap
```

### Usage with LLM context

The repository map is most commonly used to provide context to an LLM. The `omen context` command includes a `--repo-map` flag for this purpose:

```bash
# Include top 50 symbols by PageRank in LLM context
omen context --repo-map --top 50

# Combine with other context sources
omen context --repo-map --top 30 --file src/core/engine.go
```

This produces output suitable for pasting into a prompt or piping to an LLM-based tool.

## PageRank for Code Symbols

PageRank assigns high scores to symbols that are referenced by many other important symbols. In a code dependency graph:

- A utility function called by dozens of files gets a high score because many symbols depend on it.
- A core interface implemented by many classes gets a high score because the implementations reference it.
- A leaf function called only once gets a low score regardless of its complexity.

This ranking is more useful than simple reference counting because it accounts for the importance of the callers, not just their number. A function referenced once by the system's central engine is more important than a function referenced five times by test helpers.

### Sparse power iteration

Omen implements PageRank using sparse power iteration, which operates in O(E) time per iteration where E is the number of edges (references between symbols). This avoids the O(V^2) cost of dense matrix methods and allows the algorithm to handle large codebases efficiently.

Performance characteristics:

| Codebase Size | Symbols | Typical Time |
|--------------|---------|--------------|
| Small (< 1,000 files) | ~2,000 | < 1 second |
| Medium (1,000-5,000 files) | ~10,000 | 2-5 seconds |
| Large (5,000-15,000 files) | ~25,000 | 10-30 seconds |

The algorithm converges in 20-50 iterations for most codebases. Memory usage is proportional to the number of edges, not the square of the number of nodes.

## Example Output

```
Repository Map
==============

  Rank   Score    Kind        Name                            File                          Line   In   Out
  1      0.0341   interface   Handler                         src/core/handler.go           12     34   2
  2      0.0287   function    ProcessRequest                  src/core/engine.go            45     28   8
  3      0.0245   struct      Config                          src/config/config.go          8      22   0
  4      0.0198   function    Validate                        src/utils/validate.go         15     19   3
  5      0.0156   class       UserService                     src/services/user.ts          23     15   6
  6      0.0142   method      UserService.create              src/services/user.ts          45     12   4
  7      0.0131   function    format_response                 src/utils/format.py           10     14   1
  8      0.0098   trait       Serializable                    src/core/traits.rs            5      11   0
  9      0.0087   type        RequestContext                  src/types/context.ts          18     9    3
  10     0.0076   function    connect                         src/db/connection.go          22     8    2
```

In JSON format:

```json
{
  "symbols": [
    {
      "rank": 1,
      "pagerank": 0.0341,
      "kind": "interface",
      "name": "Handler",
      "path": "src/core/handler.go",
      "line": 12,
      "signature": "type Handler interface { ServeHTTP(ResponseWriter, *Request) }",
      "in_degree": 34,
      "out_degree": 2
    }
  ],
  "total_symbols": 1847,
  "files_analyzed": 234
}
```

## Practical Applications

### LLM context preparation

LLMs have limited context windows. Including the full source of every file in a large codebase is impractical. The repository map solves this by providing a ranked summary: the top N symbols by importance give the LLM a structural overview of the codebase, allowing it to understand which functions, classes, and interfaces matter most.

This is particularly useful for:

- **Code generation.** The LLM can see what interfaces exist before generating implementations.
- **Code review.** The LLM understands which components are central and which are peripheral.
- **Architecture questions.** The LLM can answer "what are the main abstractions in this codebase?" directly from the map.

```bash
# Pipe repository map into an LLM prompt
echo "Given this codebase structure:" > prompt.txt
omen context --repo-map --top 50 >> prompt.txt
echo "How should I implement a caching layer?" >> prompt.txt
```

### Onboarding

The repository map gives new team members a prioritized reading list. Instead of randomly exploring files, they can start with the highest-ranked symbols -- the structural backbone of the system.

### Architecture documentation

The ranked symbol list, combined with in-degree and out-degree data, reveals the architecture implicitly. High-PageRank interfaces and abstract classes are the system's extension points. High-in-degree utility functions are the shared infrastructure. Symbols with high out-degree are orchestrators that coordinate multiple subsystems.

### Identifying god objects

Symbols with unusually high in-degree may be god objects -- classes or modules that have accumulated too many responsibilities. If a single class has an in-degree of 50 in a codebase where the average is 5, it is worth examining whether it should be decomposed.

### Detecting orphaned code

Symbols with zero in-degree and zero out-degree are isolated from the rest of the system. They may be dead code, test utilities, or entry points. Cross-referencing with the dead code analyzer confirms which case applies.

## Relationship to Other Analyzers

The repository map shares its PageRank computation with the dependency graph analyzer (`omen graph`), but operates at symbol granularity rather than file granularity. The two analyses are complementary:

| Analyzer | Granularity | Best For |
|----------|------------|----------|
| `omen graph` | File-level | Understanding module relationships, detecting cycles, evaluating coupling |
| `omen repomap` | Symbol-level | Understanding the internal structure of modules, ranking individual functions and classes |

## Research Background

**Brin and Page (1998).** "The Anatomy of a Large-Scale Hypertextual Web Search Engine" -- The PageRank algorithm, applied here to code symbols rather than web pages. The core insight is that importance propagates through a graph: a symbol is important not just because it is referenced often, but because it is referenced by other important symbols. This produces a more meaningful ranking than raw reference counts.

The application of PageRank to code structure is well-established in the software engineering literature. Tools like Google's internal code search and several academic code comprehension tools use variants of link analysis to rank code elements by structural importance. Omen's contribution is making this analysis available as a CLI tool with output designed specifically for LLM consumption.
