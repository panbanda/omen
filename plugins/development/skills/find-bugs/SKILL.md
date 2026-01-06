---
name: find-bugs
description: Locate likely bug locations in code
usage: /find-bugs [path]
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
---

# Find Bugs Skill

Hunt for bugs in: `{{.paths}}`

## Quick Start

```bash
# Get high-risk files (threshold 0.8 = high risk)
omen defect --risk-threshold 0.8 -f json

# Find hotspots
omen hotspot --days 30 -f json

# Check for explicit markers
omen satd -f json | jq '.items[] | select(.category == "defect")'
```

## Priority Order

1. **Defect probability > 0.8**: Investigate first
2. **Hotspot score > 0.5**: Review complex logic
3. **BUG/FIXME markers**: Known issues
4. **Temporal coupling**: Check related files

## In Each File

- Look for functions with cognitive > 20
- Check error handling paths
- Review boundary conditions
