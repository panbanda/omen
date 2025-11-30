# Change Impact Analysis

Analyze the potential impact of changes to specific files or functions in this codebase.

## Instructions

1. Use `analyze_graph` with `scope: "function"` and `include_metrics: true` to see what depends on the target
2. Use `analyze_temporal_coupling` to find files that historically change together with the target
3. Use `analyze_ownership` to identify who should review changes to this area

## Analysis Steps

For the target file/function:
1. **Direct Dependencies**: What directly calls or imports this code?
2. **Transitive Impact**: What depends on the direct dependencies?
3. **Historical Co-changes**: What files typically change when this file changes?
4. **PageRank**: How central is this code? High PageRank = more careful changes needed.

## Output Format

Provide an impact report with:
1. **Direct Dependents**: Files/functions that directly use the target
2. **Ripple Effect**: Estimate of how many files could be affected
3. **Co-change Candidates**: Files that historically change together (likely need updates)
4. **Risk Assessment**: Low/Medium/High based on centrality and coupling
5. **Reviewers**: Suggested reviewers based on code ownership
