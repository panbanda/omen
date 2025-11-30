# Quality Gate Check

Perform a quality gate check to determine if the codebase meets quality thresholds.

## Instructions

1. Use `analyze_tdg` to get overall quality grade and distribution
2. Use `analyze_complexity` to check for functions exceeding thresholds
3. Use `analyze_duplicates` to check duplication ratio
4. Use `analyze_defect` to count high-risk files
5. Use `analyze_satd` to count high-severity debt items

## Quality Thresholds

Default thresholds (adjust as needed):
- **TDG Average Grade**: B or better
- **Max Cyclomatic Complexity**: 15 per function
- **Max Cognitive Complexity**: 20 per function
- **Duplication Ratio**: < 5%
- **High-Risk Files**: < 10% of codebase
- **Critical SATD Items**: 0

## Output Format

Provide a pass/fail gate report with:
1. **Overall Status**: PASS or FAIL
2. **Metric Results**: Each threshold with current value and status
3. **Violations**: Specific items that failed thresholds
4. **Trend**: If available, comparison to previous check
5. **Remediation**: For failures, what needs to change to pass
