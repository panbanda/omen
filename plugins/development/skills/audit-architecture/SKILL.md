---
name: audit-architecture
description: Audit module coupling, cohesion, hidden dependencies, and design smells. Use when conducting architecture reviews, evaluating design decisions, or identifying structural tech debt.
---

# Audit Architecture

Analyze the structural health of a codebase. Examine the coupling, the cohesion, the hidden dependencies, and the design patterns.

## Prerequisites

The Omen CLI must be installed and available in PATH.

## Workflow

### Step 1: Map the Module Dependencies

Run the dependency graph analysis:

```bash
omen -f json graph
```

Look for these items:
- Circular dependencies.
- God modules (too many dependents).
- Orphan modules (no connections).

### Step 2: Detect the Architectural Smells

Run the smells analysis:

```bash
omen -f json smells
```

The command detects cyclic dependencies, hub modules, unstable dependencies, and central connectors. It uses the Tarjan SCC algorithm.

### Step 3: Measure the Cohesion

Run the cohesion analysis for the CK metrics:

```bash
omen -f json cohesion
```

Watch these metrics:
- **LCOM** (Lack of Cohesion): A value above 0.7 shows that a class does too many things.
- **WMC** (Weighted Methods per Class): A value above 30 shows that a class is too complex.
- **CBO** (Coupling Between Objects): A value above 10 shows that a class has too many dependencies.

### Step 4: Find the Hidden Dependencies

Run the temporal coupling analysis:

```bash
omen -f json temporal
```

Files that always change together but have no explicit imports show one of these problems:
- Shared global state.
- Implicit contracts.
- Missing abstractions.

### Step 5: Check the Ownership Distribution

Run the ownership analysis:

```bash
omen -f json ownership
```

Look for these items:
- Modules with no clear owner.
- Ownership that is too concentrated (bus factor = 1).
- Fragmented ownership (many contributors, none dominant).

## Architecture Smells

These are the common issues to identify:

| Smell | Detection | Impact |
|-------|-----------|--------|
| Circular Dependency | A -> B -> C -> A in graph | Hard to test, modify |
| God Module | > 50% of code depends on it | Single point of failure |
| Shotgun Surgery | High temporal coupling | Changes ripple everywhere |
| Feature Envy | Low cohesion in classes | Wrong abstraction boundaries |
| Knowledge Silo | Bus factor = 1 | Team risk |

## Output Format

Show the architecture review in this format:

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
