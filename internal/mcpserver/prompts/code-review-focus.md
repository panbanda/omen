---
description: Identify what to focus on when reviewing code changes, including complexity deltas, duplication, and risk assessment.
---

# Code Review Focus

Identify what to focus on when reviewing code changes in this codebase.

## Instructions

Given a set of changed files, analyze:
1. Use `analyze_complexity` on the changed files to check if complexity increased
2. Use `analyze_duplicates` to check if changes introduced code clones
3. Use `analyze_deadcode` to check if changes orphaned any code
4. Use `analyze_defect` to check if changes affected high-risk files
5. Use `analyze_graph` to understand impact on dependencies

## Review Priorities

Focus review attention based on:
- **Complexity Delta**: Did the change make code harder to understand?
- **Clone Introduction**: Did the change copy-paste instead of abstracting?
- **High-Risk Areas**: Is this change in a statistically bug-prone file?
- **Architectural Impact**: Does this change affect many dependents?

## Output Format

Provide review guidance with:
1. **Risk Summary**: Overall risk level of the change (Low/Medium/High)
2. **Focus Areas**: Specific files/functions that need careful review
3. **Complexity Concerns**: Functions that may have become too complex
4. **Duplication Warnings**: Potential code clones to address
5. **Test Recommendations**: What additional tests might be needed
