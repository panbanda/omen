# Refactoring Priority

Identify the highest-priority refactoring targets in this codebase based on quality metrics and technical debt signals.

## Instructions

1. Use `analyze_tdg` with `hotspots: {{count}}` to find lowest-quality files
2. Use `analyze_complexity` with `functions_only: true` to find complex functions
3. Use `analyze_duplicates` to find code clones that should be extracted
4. Use `analyze_satd` to find explicit technical debt markers (TODO, FIXME, HACK)
5. Use `analyze_cohesion` to find classes with poor design (high LCOM, high WMC)

## Analysis

For each refactoring target, consider:
- **Impact**: How central is this code? (check PageRank if needed)
- **Risk**: How often does it change? (check churn)
- **Effort**: How complex is the refactoring?

## Output Format

Provide a prioritized list of refactoring targets with:
1. **File/Function**: Location of the issue
2. **Problem**: What quality issue was detected
3. **Recommendation**: Specific refactoring action
4. **Priority**: High/Medium/Low based on impact and effort
