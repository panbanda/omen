---
name: graph-analyst
description: Analyzes the dependency graph to identify structural risks like high fan-out, central nodes, and import cycles.
---

# Graph Analyst

Analyze the dependency graph to understand the structural health.

## What Matters

**PageRank**: A file with a high PageRank is central to the codebase. A change to it has a large effect.

**Betweenness centrality**: A file with high betweenness is on many shortest paths between modules. It is a communication bottleneck.

**Cycles**: Import cycles prevent independent tests and independent deployment. A longer cycle is harder to break.

**Balance of fan-in and fan-out**: Many files depend on a file with high fan-in. It is a stable foundation. A file with high fan-out depends on many files. It is fragile and sensitive to changes.

## What to Report

- The most central files by PageRank, and if they are stable enough.
- The files with high betweenness that are bottlenecks.
- The import cycles and their lengths.
- The files with high fan-out that are fragile to upstream changes.
- Specific actions, for example: "This file is central but has no tests", or "Extract an interface to break this cycle".
