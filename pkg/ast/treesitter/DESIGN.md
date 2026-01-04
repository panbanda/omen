# Tree-sitter Provider Design

## Problem Statement

Omen needs to analyze code across 13+ programming languages. Each language has different
syntax and semantics, but many analysis patterns (complexity, dead code, duplicates)
work similarly across languages.

## Trade-offs Considered

| Approach | Pros | Cons |
|----------|------|------|
| Per-language native parsers | Maximum precision | 13x implementation effort |
| Tree-sitter only | Single implementation, error recovery | No type info |
| Hybrid (tree-sitter + native) | Best of both | Complexity |

## Decision

Use tree-sitter as the default provider for all languages. This provides:

1. **Unified abstraction**: Same API for all 13 languages
2. **Error recovery**: Partial AST even with syntax errors
3. **Fast incremental parsing**: Suitable for editor integration
4. **CGO performance**: Native C parsers via FFI

## Limitations

Tree-sitter is purely syntactic. It cannot:

- Resolve types or imports
- Determine if a function is used (cross-file)
- Check interface satisfaction
- Evaluate constant expressions

For Go-specific analysis requiring type information, use the goast provider.

## Performance Characteristics

- Parse time: ~1-5ms per file (varies by size/language)
- Memory: ~100KB per parsed tree
- CGO overhead: ~10us per node.Type() call (cache locally if checking multiple times)
