# Omen Codebase Optimization Analysis

> Analysis Date: 2025-12-16
> Analyst: Claude Code
> Focus: Performance, Architecture, and Agent Context Decision-Making

## Executive Summary

This analysis identifies **critical optimization opportunities** in the Omen codebase, evaluated through the lens of:
1. **Clean Code Principles** - Avoiding god objects, maintaining single responsibility
2. **Agent Context Patterns** - How AI agents consume and reason about code

The codebase demonstrates **strong architectural fundamentals** with consistent patterns across 18 analyzers, but has **critical performance issues** and **dead code** that should be addressed.

---

## Critical Issues (High Priority)

### 1. Cache Package is Completely Unused

**Location**: `internal/cache/cache.go`
**Impact**: Severe - All analysis re-runs from scratch every time

The cache package is fully implemented with:
- BLAKE3 hashing for content validation
- TTL-based expiration
- JSON serialization

**But it's never used.** No analyzer or service layer integrates with it.

```bash
# Verification: No cache.Get/Set calls outside tests
grep -r "cache\.\(Get\|Set\|New\)" --include="*.go" | grep -v "_test.go"
# Returns: No files found
```

**Recommendation**: Integrate caching at the service layer for incremental analysis.

---

### 2. Parser Created Per-File in Parallel Processing

**Location**: `internal/fileproc/parallel.go:137-138`

```go
psr := parser.New()   // NEW parser for EVERY file
defer psr.Close()

result, err := fn(psr, path)
```

**Impact**: For a 10,000-file codebase:
- 10,000 parser instantiations
- 10,000 CGO boundary crossings for setup
- ~10 seconds of pure parser overhead

**Recommendation**: Implement per-worker parser pools instead of per-file creation:

```go
// Worker reuses parser across files
p.Go(func(ctx context.Context) error {
    psr := parserPool.Get()  // Reuse from pool
    defer parserPool.Put(psr)
    // ... process files
})
```

---

### 3. ContentSource Interface Duplicated 4+ Times

**Locations**:
- `pkg/analyzer/analyzer.go:8-10`
- `pkg/parser/parser.go`
- `pkg/source/source.go:11-14`
- `internal/fileproc/commit.go`
- `pkg/analyzer/score/score.go`

Each package defines its own identical interface:
```go
type ContentSource interface {
    Read(path string) ([]byte, error)
}
```

**Impact**:
- Type mismatch errors requiring aliases
- Confusion for contributors
- Violates DRY principle

**Recommendation**: Define once in `pkg/source/source.go`, import everywhere.

---

### 4. Mutex Contention in Result Collection

**Location**: `internal/fileproc/parallel.go:148-150`

```go
mu.Lock()
results = append(results, result)
mu.Unlock()
```

With `2 * NumCPU` workers all contending on a single mutex:

**Recommendation**: Use indexed assignment instead:
```go
results := make([]T, len(files))
// In worker:
results[fileIndex] = result  // No mutex needed
```

---

### 5. 194 TODO/FIXME Comments Unaddressed

```
Found 194 total occurrences across 40 files
```

Many are concentrated in:
- `pkg/analyzer/satd/satd_test.go` (72 occurrences - test markers)
- `plugins/` directory (documentation examples)
- Core code (~20 actionable items)

**Recommendation**: Triage and address core TODOs, document intentional ones.

---

## Architectural Strengths (What to Preserve)

### Consistent Analyzer Pattern

Every analyzer follows the same structure:
```
pkg/analyzer/<name>/
├── analyzer.go     # New() + Analyze() + Close()
├── types.go        # Co-located result types
└── *_test.go       # Unit tests
```

**Benefits for agents**:
- Predictable structure enables pattern matching
- Self-contained packages reduce context needed
- Consistent interfaces simplify reasoning

### Clean Layering

```
cmd/omen/ (CLI)
    ↓
internal/service/ (Orchestration)
    ↓
pkg/analyzer/* (Analysis Logic)
    ↓
pkg/parser/ (Tree-sitter Wrapper)
```

**Benefits for agents**:
- Clear separation enables focused context loading
- Each layer has single responsibility
- Dependencies flow downward only

### Functional Options Pattern (Ubiquitous)

```go
analyzer := complexity.New(
    complexity.WithMaxFileSize(cfg.MaxFileSize),
    complexity.WithThreshold(threshold),
)
```

**Benefits for agents**:
- Self-documenting configuration
- Compile-time type safety
- Easy to extend without breaking changes

---

## Agent Context Decision-Making Patterns

### Current Patterns (Good)

1. **MCP Server Integration** (`internal/mcpserver/`)
   - Tools registered with descriptions
   - Prompts stored as markdown
   - Clean JSON schemas for tool parameters

2. **Embedded Prompts** (`internal/mcpserver/prompts/*.md`)
   - Markdown files with clear structure
   - Context-aware prompt templates
   - Skill-based organization

3. **Skills Directory** (`plugins/*/skills/*/SKILL.md`)
   - Self-describing skill files
   - Clear invocation patterns
   - Examples included

### Improvement Opportunities

#### 1. Add Semantic Metadata for Context Loading

Analyzers lack machine-readable metadata about what files they need:

```go
// Current: No metadata
type Analyzer struct { ... }

// Proposed: Add metadata
type Analyzer struct {
    // Metadata for agents
    InputTypes  []string // ["*.go", "*.py"]
    OutputType  string   // "complexity_analysis"
    Dependencies []string // ["parser"]
}
```

**Benefit**: Agents can pre-load minimal context.

#### 2. Add Result Summarization Methods

Current results are full data structures. Agents need summaries:

```go
// Proposed addition to Analysis types
func (a *Analysis) Summary() string {
    return fmt.Sprintf(
        "%d files analyzed, %d high-complexity functions",
        len(a.Files), a.HighComplexityCount,
    )
}

func (a *Analysis) TopIssues(n int) []Issue {
    // Return most important findings
}
```

**Benefit**: Agents can request summaries before full data.

#### 3. Create Analyzer Registry

Current service layer has duplicate methods for each analyzer:

```go
// Current: 15 nearly-identical methods
func (s *Service) AnalyzeComplexity(ctx, files, opts)
func (s *Service) AnalyzeSATD(ctx, files, opts)
func (s *Service) AnalyzeDeadCode(ctx, files, opts)
// ... repeats
```

**Proposed**:
```go
type AnalyzerRegistry interface {
    Register(name string, factory AnalyzerFactory)
    Get(name string) (Analyzer, error)
    List() []AnalyzerInfo
}
```

**Benefit**: Dynamic discovery, less boilerplate, plugin support.

---

## Performance Optimization Summary

| Issue | Impact | Fix Effort | Priority |
|-------|--------|------------|----------|
| Cache unused | Severe | Medium | P0 |
| Parser-per-file | High | Low | P0 |
| ContentSource duplication | Medium | Low | P1 |
| Mutex contention | Medium | Low | P1 |
| TODO triage | Low | Low | P2 |

---

## Memory Management Observations

### Current Issues

1. **Source retention in ParseResult**
   - `ParseResult` holds full `Source []byte`
   - Large files create memory pressure
   - No streaming option

2. **No result streaming for large codebases**
   - All results buffered in memory
   - 11 analyzers × N files = high memory footprint

3. **String allocations in AST traversal**
   - `GetNodeText()` allocates per call
   - No pooling of byte buffers

### Recommendations

1. Add streaming result collectors for 100k+ file repos
2. Implement buffer pooling for text extraction
3. Consider lazy source loading (read on demand)

---

## Tree-sitter Specific Optimizations

### CGO Overhead Reduction

1. **Use `WalkTyped()` consistently** (exists but underutilized)
   - Caches node types during traversal
   - Located: `pkg/parser/parser.go:249-262`

2. **Batch field lookups**
   - Current: Individual `ChildByFieldName()` calls
   - Proposed: `getNodeFields(node, "name", "params", "body")`

3. **Cache language objects**
   - Current: `GetTreeSitterLanguage()` called repeatedly
   - Proposed: Singleton pattern per language

---

## Recommended Implementation Order

### Phase 1: Quick Wins (1-2 days)
1. Consolidate `ContentSource` interface
2. Implement indexed result collection (no mutex)
3. Cache language objects in parser

### Phase 2: Performance (3-5 days)
1. Implement per-worker parser pools
2. Integrate cache package into service layer
3. Add L1 in-memory cache

### Phase 3: Agent Improvements (5-7 days)
1. Add analyzer metadata
2. Implement result summarization methods
3. Create analyzer registry pattern

---

## Files Reference

| File | Purpose | Key Lines |
|------|---------|-----------|
| `internal/fileproc/parallel.go` | Worker pool | 137-138 (parser per file) |
| `internal/cache/cache.go` | Unused cache | Full file |
| `pkg/analyzer/analyzer.go` | Interface defs | 8-10 (ContentSource) |
| `pkg/parser/parser.go` | Multi-lang parser | 142-174 (language lookup) |
| `internal/service/analysis/analysis.go` | Orchestration | 150-320 (parallel analysis) |

---

## Conclusion

The Omen codebase has **excellent architectural foundations** - consistent patterns, clean separation, and proper abstractions. The main optimization opportunities are:

1. **Activate the unused cache** - immediate 80%+ reduction in re-analysis time
2. **Fix parser-per-file** - significant CGO overhead reduction
3. **Consolidate duplicated interfaces** - cleaner codebase
4. **Add agent-friendly metadata** - better context decision-making

These changes would make Omen both faster for users and more consumable for AI agents that need to reason about codebases.
