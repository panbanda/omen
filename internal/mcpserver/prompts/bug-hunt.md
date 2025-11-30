# Bug Hunt

Identify the most likely locations for bugs in this codebase using statistical defect prediction.

## Instructions

1. Use `analyze_defect` with `high_risk_only: true` to find statistically high-risk files
2. Use `analyze_hotspot` with `days: {{days}}` to find high-churn + high-complexity intersections
3. Use `analyze_temporal_coupling` to find files that change together (bugs often span coupled files)
4. Use `analyze_ownership` to find knowledge silos (single-owner files have higher risk)

## Bug Indicators

Files are more likely to contain bugs when they have:
- High defect probability (>70%)
- High hotspot score (>0.5) - both complex AND frequently changed
- Strong temporal coupling with other buggy files
- Single owner (knowledge silo)

## Output Format

Provide a risk assessment with:
1. **High-Risk Files**: Files most likely to contain bugs, ranked by probability
2. **Contributing Factors**: Why each file is risky (churn, complexity, coupling, ownership)
3. **Related Files**: Temporally coupled files that may share the bug
4. **Testing Recommendations**: Which files need the most test coverage
