---
name: check-quality
description: Assess overall code quality
usage: /check-quality [path]
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
---

# Check Quality Skill

Assess quality of: `{{.paths}}`

## Quick Start

```bash
# Get health score (0-100)
omen score -f json

# Check complexity violations
omen complexity -f json | jq '.files[].functions[] | select(.metrics.cyclomatic > 15)'

# Find architectural smells
omen smells -f json
```

## Score Interpretation

| Score | Status | Action |
|-------|--------|--------|
| 90-100 | Excellent | Maintain |
| 80-89 | Good | Minor fixes |
| 70-79 | Fair | Address issues |
| 50-69 | Poor | Prioritize fixes |
| <50 | Critical | Immediate action |

## Key Metrics

- **Complexity**: Functions > 15 cyclomatic
- **Duplication**: Clone ratio > 5%
- **Debt**: SATD density
- **Coupling**: Circular dependencies
