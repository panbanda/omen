---
name: audit-architecture
description: Audit module coupling, cohesion, hidden dependencies, and design smells. Use when conducting architecture reviews, evaluating design decisions, or identifying structural tech debt.
---

# Audit Architecture

Analyze the structural health of a codebase by examining coupling, cohesion, hidden dependencies, and design patterns.

## Prerequisites

Omen CLI must be installed and available in PATH.

## Workflow

### Step 1: Map Module Dependencies

Run the dependency graph analysis:

```bash
omen -f json graph
```

Look for:
- Circular dependencies
- God modules (too many dependents)
- Orphan modules (no connections)

### Step 2: Detect Architectural Smells

Run the smells analysis:

```bash
omen -f json smells
```

Detects cyclic dependencies, hub modules, unstable dependencies, and central connectors using Tarjan's SCC algorithm.

### Step 3: Measure Cohesion

Run the cohesion analysis for CK metrics:

```bash
omen -f json cohesion
```

Metrics to watch:
- **LCOM** (Lack of Cohesion): > 0.7 indicates class doing too many things
- **WMC** (Weighted Methods per Class): > 30 indicates too complex
- **CBO** (Coupling Between Objects): > 10 indicates too many dependencies

### Step 4: Find Hidden Dependencies

Run the temporal coupling analysis:

```bash
omen -f json temporal
```

Files that always change together but don't have explicit imports indicate:
- Shared global state
- Implicit contracts
- Missing abstractions

### Step 5: Check Ownership Distribution

Run the ownership analysis:

```bash
omen -f json ownership
```

Look for:
- Modules with no clear owner
- Over-concentrated ownership (bus factor = 1)
- Fragmented ownership (many contributors, none dominant)

## Architecture Smells

Common issues to identify:

| Smell | Detection | Impact |
|-------|-----------|--------|
| Circular Dependency | A -> B -> C -> A in graph | Hard to test, modify |
| God Module | > 50% of code depends on it | Single point of failure |
| Shotgun Surgery | High temporal coupling | Changes ripple everywhere |
| Feature Envy | Low cohesion in classes | Wrong abstraction boundaries |
| Knowledge Silo | Bus factor = 1 | Team risk |

## Output Format

Present architecture review as:

```markdown
# Architecture Review

## Module Dependency Analysis

### Dependency Graph
[Mermaid diagram from graph analysis]

### Issues Detected

#### Circular Dependencies
- `auth/` <-> `user/` <-> `session/`
  - Impact: Cannot deploy or test independently
  - Fix: Extract shared interface to `contracts/`

#### God Modules
- `core/` - 75% of modules depend on this
  - Impact: Changes here affect everything
  - Fix: Split into focused submodules

## Cohesion Analysis

### Low Cohesion Classes (LCOM > 0.7)
| Class | LCOM | WMC | Recommendation |
|-------|------|-----|----------------|
| UserService | 0.85 | 45 | Split into UserAuth, UserProfile |
| DataProcessor | 0.78 | 38 | Extract strategies for each type |

### High Coupling (CBO > 10)
- `api/handlers.go` - CBO: 15
  - Depends on too many internal modules
  - Consider facade pattern

## Hidden Dependencies

### Temporal Coupling (> 0.8)
- `config/settings.go` <-> `cache/redis.go` (0.92)
  - No import relationship
  - Likely shares configuration assumptions
  - Fix: Explicit configuration injection

## Ownership Distribution

### Knowledge Silos
- `legacy/` - Single owner (bob), no recent contributions
  - Risk: Bob leaves, no one knows this code
  - Fix: Pair programming, documentation

### Fragmented Ownership
- `core/` - 8 contributors, none > 20%
  - Risk: No clear decision maker
  - Fix: Assign primary maintainer

## Recommendations

### Immediate Actions
1. Break circular dependency in auth/user/session
2. Document legacy/ module before Bob's vacation

### Medium Term
1. Split core/ into focused modules
2. Add explicit dependency injection for config

### Long Term
1. Establish module ownership model
2. Create architecture decision records (ADRs)
```
