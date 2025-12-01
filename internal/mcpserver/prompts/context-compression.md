---
name: context-compression
title: Context Compression
description: Generate a compressed context summary of a codebase for LLM consumption using PageRank-ranked symbols
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: max_symbols
    description: Maximum symbols to include
    required: false
    default: "50"
  - name: include_graph
    description: Include dependency graph summary
    required: false
    default: "true"
---

# Context Compression

Generate a compressed codebase summary for: {{.paths}}

## When to Use

- When providing codebase context to an LLM
- When context window is limited
- To create a "map" for navigating a large codebase
- As a foundation for further exploration

## Workflow

### Step 1: PageRank Symbol Ranking
```
analyze_repo_map:
  paths: {{.paths}}
  top: {{.max_symbols}}
```
Get the most important symbols ranked by PageRank - these are the core abstractions.

### Step 2: Module Structure
```
analyze_graph:
  paths: {{.paths}}
  scope: module
  include_metrics: true
```
Get high-level module dependencies to understand architecture.

### Step 3: Entry Points
```
analyze_graph:
  paths: {{.paths}}
  scope: function
  include_metrics: true
```
Find functions with highest in-degree - these are the main entry points.

## Compression Strategy

1. **Symbol Selection**: PageRank prioritizes symbols that are most connected - changes to these affect the most code
2. **Deduplication**: Group symbols by file to reduce redundancy
3. **Hierarchy**: Present modules before functions before variables
4. **Signatures Only**: Include function signatures, not implementations

## Output

### Codebase Context: {{.paths}}

**Compression Level**: {{.max_symbols}} symbols
**Total Files**: [count]
**Total Symbols**: [count]
**Compression Ratio**: [ratio]

---

### Architecture Summary

```
[Simplified module dependency graph]
Module A --> Module B
Module A --> Module C
Module B --> Module D
```

**Core Modules**:
| Module | Files | Symbols | Role |
|--------|-------|---------|------|
| | | | |

### Entry Points

Functions with highest in-degree (most called):

| Function | File | In-Degree | Purpose |
|----------|------|-----------|---------|
| | | | |

### Hub Files

Files with highest PageRank (most central):

| File | PageRank | Key Exports |
|------|----------|-------------|
| | | |

### Core Symbols (by PageRank)

#### Types and Interfaces

```
// File: [path]
type Symbol1 struct { ... }
type Symbol2 interface { ... }
```

#### Key Functions

```
// File: [path]
func Function1(args) returns { ... }
func Function2(args) returns { ... }
```

#### Constants and Configuration

```
// File: [path]
const KEY_CONSTANT = value
var GlobalConfig = ...
```

### Symbol Index

Quick reference of all included symbols:

| Symbol | Kind | File | PageRank |
|--------|------|------|----------|
| | type/func/const | | |

### Navigation Hints

To explore further:
- **Understand X**: Read [files]
- **Modify Y**: Check dependents via `change-impact`
- **Debug Z**: Start at [entry point]

### Excluded (Low PageRank)

Symbols excluded from this summary (can be explored if needed):
- [count] utility functions
- [count] internal helpers
- [count] test fixtures

---

**Usage**: This context can be provided to an LLM to help it understand the codebase structure before making changes.
