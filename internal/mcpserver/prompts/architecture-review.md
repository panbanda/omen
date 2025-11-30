---
description: Analyze architectural health including module coupling, cohesion metrics, hidden dependencies, and design smells.
---

# Architecture Review

Analyze the architectural health of this codebase.

## Instructions

1. Use `analyze_graph` with `scope: "module"` and `include_metrics: true` for dependency analysis
2. Use `analyze_cohesion` to check OO design quality (CK metrics)
3. Use `analyze_temporal_coupling` to find hidden dependencies not visible in imports
4. Use `analyze_ownership` to check if architecture aligns with team structure

## Architectural Concerns

Look for:
- **High Coupling**: Modules with many dependencies (high CBO, high out-degree)
- **Low Cohesion**: Classes doing unrelated things (high LCOM)
- **Hidden Dependencies**: Temporal coupling without import relationships
- **God Classes**: High WMC combined with high LCOM
- **Deep Hierarchies**: DIT > 4 indicates overly complex inheritance
- **Conway's Law Violations**: Code ownership that doesn't match module boundaries

## Output Format

Provide an architecture assessment with:
1. **Health Score**: Overall architectural health (Good/Fair/Poor)
2. **Dependency Analysis**: Module coupling and potential cycles
3. **Design Smells**: Classes violating CK metric thresholds
4. **Hidden Couplings**: Temporal dependencies that suggest missing abstractions
5. **Recommendations**: Specific architectural improvements to consider
