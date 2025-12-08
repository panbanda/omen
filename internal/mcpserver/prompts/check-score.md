---
name: check-score
title: Check Score
description: Compute and analyze repository health score. Use for quick quality assessment, identifying improvement areas, and tracking health over time.
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
---

# Repository Health Check

Compute and analyze repository health score for: {{.paths}}

## When to Use

- Quick health assessment of a codebase
- Before/after comparison for refactoring efforts
- Identifying which areas need most attention
- Establishing baseline metrics for quality gates

## Workflow

### Step 1: Compute Score
```
score_repository:
  paths: {{.paths}}
```

### Step 2: Interpret Results

The composite score (0-100) indicates overall health:

| Score Range | Status |
|-------------|--------|
| 90-100 | Excellent health |
| 80-89 | Good health |
| 70-79 | Fair, needs attention |
| 50-69 | Poor, significant issues |
| 0-49 | Critical, immediate action needed |

### Step 3: Drill Into Problem Areas

For each component scoring below the composite, investigate:

- **Low Complexity Score (<70)**: Run `analyze_complexity` to find specific functions
- **Low Duplication Score (<80)**: Run `analyze_duplicates` to find clone groups
- **Low SATD Score (<80)**: Run `analyze_satd` to find explicit debt markers (TODO/FIXME)
- **Low TDG Score (<70)**: Run `analyze_tdg` to find files with high technical debt gradient
- **Low Coupling Score (<70)**: Run `analyze_graph` and `analyze_smells`
- **Low Smells Score (<80)**: Run `analyze_smells` for architectural issues
- **Low Cohesion Score (<70)**: Run `analyze_cohesion` to find classes with poor cohesion

For defect prediction (not included in composite score), use `analyze_defect` separately.

## Output

### Repository Health Report

**Scope**: {{.paths}}
**Score**: [score]/100

| Component | Score | Weight | Status |
|-----------|-------|--------|--------|
| Complexity | | 25% | |
| Duplication | | 20% | |
| SATD | | 10% | |
| TDG | | 15% | |
| Coupling | | 10% | |
| Smells | | 5% | |
| Cohesion | | 15% | |

### Score Breakdown

**Strengths** (components >= 80):
- [List high-scoring components]

**Areas Needing Attention** (components < 70):
- [List components with scores below 70, ordered by impact on composite]

### Recommended Actions

Priority order based on weight and improvement potential:

1. **Highest Impact**: [Component with lowest score * highest weight]
   - Specific action items

2. **Second Priority**: [Next highest impact]
   - Specific action items

3. **Third Priority**: [Third highest impact]
   - Specific action items

### Quality Gate Recommendations

For CI enforcement, consider these minimum thresholds:

```bash
omen score --min-score 70
```

Or per-component thresholds:
```bash
omen score --min-complexity 75 --min-duplication 80 --min-defect 70
```

---

**Next Steps**:
- Run detailed analysis on lowest-scoring components
- Set up CI quality gates with appropriate thresholds
- Track score trends over time with `omen score --json >> scores.jsonl`
