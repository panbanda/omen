---
description: Identify which files and functions most need additional test coverage based on risk, complexity, and churn.
---

# Test Targeting

Identify which files and functions most need additional test coverage.

## Instructions

1. Use `analyze_defect` to find high-risk files (statistically likely to have bugs)
2. Use `analyze_hotspot` to find high-churn + high-complexity code
3. Use `analyze_complexity` to find functions with many code paths (need more test cases)
4. Use `analyze_ownership` to find knowledge silos (need tests as documentation)

## Test Priority Factors

Files need more tests when they have:
- **High Defect Probability**: Statistically likely to contain bugs
- **High Cyclomatic Complexity**: Many paths to cover
- **High Churn**: Frequently changing, needs regression protection
- **Single Owner**: Tests serve as documentation when the owner is unavailable

## Output Format

Provide test recommendations with:
1. **Critical Coverage Gaps**: Files with high risk and likely low coverage
2. **Complexity Hotspots**: Functions needing thorough path coverage
3. **Regression Priorities**: High-churn files needing change detection
4. **Test Case Estimates**: Approximate test cases needed based on cyclomatic complexity
5. **Testing Strategy**: Unit vs integration test recommendations per area
