---
sidebar_position: 1
---

# Analyzers Overview

Omen ships with 19 analyzers that cover structural quality, defect risk, historical patterns, dependency health, and specialized concerns. Each analyzer is available as a top-level subcommand and can be run independently or as part of a full suite via `omen all`.

All analyzers parse source code through tree-sitter grammars, producing syntax-aware results rather than regex-based approximations. Output is available as formatted tables (default) or JSON (`-f json`).

## Complexity & Quality

Analyzers that measure the structural and cognitive properties of the code itself.

| Analyzer | Command | What It Measures |
|----------|---------|------------------|
| [Complexity](./complexity.md) | `omen complexity` | Cyclomatic and cognitive complexity per function, nesting depth |
| [CK Metrics (Cohesion)](./cohesion.md) | `omen cohesion` | Chidamber-Kemerer OO metrics: WMC, CBO, RFC, LCOM4, DIT, NOC |
| [Technical Debt Gradient](./tdg.md) | `omen tdg` | Composite file health score (0-100) across 9 weighted dimensions |
| [Architectural Smells](./smells.md) | `omen smells` | God classes, data clumps, feature envy, cyclic dependencies |

## Risk & Defects

Analyzers that predict where bugs are likely to appear based on code structure, change patterns, and historical defect signals.

| Analyzer | Command | What It Measures |
|----------|---------|------------------|
| [Defect Prediction](./defect-prediction.md) | `omen defect` | Probability of defects using complexity, churn, and ownership signals |
| [Change Risk](./change-risk.md) | `omen changes` | Risk score for recent modifications based on size, complexity delta, and file history |
| [Diff Analysis](./diff-analysis.md) | `omen diff` | Structural analysis of uncommitted or branch changes |
| [Hotspots](./hotspots.md) | `omen hotspot` | Files with both high complexity and high change frequency |

## History & Ownership

Analyzers that use Git history to surface patterns invisible in a single snapshot.

| Analyzer | Command | What It Measures |
|----------|---------|------------------|
| [Churn](./churn.md) | `omen churn` | Change frequency and volume per file over time |
| [Temporal Coupling](./temporal-coupling.md) | `omen temporal` | Files that change together, suggesting hidden dependencies |
| [Ownership](./ownership.md) | `omen ownership` | Contributor distribution per file, bus factor |

## Structure & Dependencies

Analyzers that examine relationships between code units: imports, call graphs, duplication, and reachability.

| Analyzer | Command | What It Measures |
|----------|---------|------------------|
| [Dependency Graph](./dependency-graph.md) | `omen graph` | Import/dependency relationships, coupling metrics, cycles |
| [Dead Code](./dead-code.md) | `omen deadcode` | Unreachable functions, unused exports, orphaned modules |
| [Code Clones](./code-clones.md) | `omen clones` | Duplicated code blocks (Type 1, 2, and 3 clones) |
| [Repository Map](./repomap.md) | `omen repomap` | Structural map of modules, symbols, and relationships |

## Specialized

Analyzers focused on specific concerns that cut across the categories above.

| Analyzer | Command | What It Measures |
|----------|---------|------------------|
| [SATD Detection](./satd.md) | `omen satd` | Self-Admitted Technical Debt in comments (TODO, FIXME, HACK, etc.) |
| [Feature Flags](./feature-flags.md) | `omen flags` | Feature flag usage, stale flags, flag-related complexity |
| [Mutation Testing](./mutation-testing.md) | `omen mutation` | Test suite effectiveness by injecting controlled code mutations |

## Running Multiple Analyzers

Run all analyzers at once:

```bash
omen all
```

Run a subset by chaining commands:

```bash
omen complexity && omen cohesion && omen smells
```

All commands accept the same global flags:

| Flag | Description |
|------|-------------|
| `-p <path>` | Target path (local directory, file, or `owner/repo` for remote) |
| `-f json` | JSON output |
| `--language <lang>` | Filter to a specific language |
| `--config <path>` | Path to `omen.toml` configuration file |
