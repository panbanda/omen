---
description: Generate a comprehensive technical debt assessment including SATD, quality grades, duplication, and high-risk areas.
---

# Technical Debt Report

Generate a comprehensive technical debt assessment for this codebase.

## Instructions

1. Use `analyze_satd` to find explicit debt (TODO, FIXME, HACK comments)
2. Use `analyze_tdg` for quality scores and grade distribution
3. Use `analyze_duplicates` to quantify copy-paste debt
4. Use `analyze_defect` to find implicit debt (high-risk files)
5. Use `analyze_complexity` for functions exceeding complexity thresholds

## Debt Categories

Organize findings by type:
- **Explicit Debt**: SATD markers left by developers
- **Structural Debt**: High complexity, poor cohesion
- **Duplication Debt**: Code clones requiring synchronized maintenance
- **Design Debt**: Architectural issues (coupling, inheritance depth)
- **Test Debt**: High-risk files lacking coverage

## Output Format

Provide a debt report with:
1. **Executive Summary**: Overall debt level and trend
2. **Grade Distribution**: How many files are A/B/C/D/F quality
3. **Explicit Debt Inventory**: Categorized SATD items with severity
4. **Duplication Report**: Clone count and estimated maintenance cost
5. **High-Risk Areas**: Files that need immediate attention
6. **Remediation Priorities**: Ordered list of what to fix first
