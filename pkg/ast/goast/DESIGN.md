# Go AST Provider Design

## Problem Statement

Some analyzers (deadcode, cohesion) benefit from Go's type information to reduce
false positives. The tree-sitter provider cannot resolve types or cross-file references.

## Trade-offs Considered

| Approach | Pros | Cons |
|----------|------|------|
| go/parser only | Fast, simple | No type info |
| go/types directly | Full semantic info | Requires build |
| golang.org/x/tools/go/packages | Handles modules | Slower startup |

## Decision

Use `golang.org/x/tools/go/packages` with appropriate LoadMode:

- `NeedSyntax` for AST access
- `NeedTypes` + `NeedTypesInfo` for type resolution
- Lazy loading to avoid upfront cost

## When to Use

Use the Go AST provider when you need:

- Accurate dead code detection (cross-package references)
- Interface satisfaction checks
- Method set computation
- Type-aware refactoring

## Performance Characteristics

- Cold start: ~200-500ms for medium project (loading types)
- Memory: ~50-100MB for 100k LOC project
- Cached: Subsequent parses much faster

## Limitations

- Only works for valid Go code (no error recovery)
- Requires go.mod and resolvable dependencies
- Cannot analyze incomplete packages
