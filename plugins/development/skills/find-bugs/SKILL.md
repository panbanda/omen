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

Search for bugs in: `{{.paths}}`

## Quick Start

```bash
# Get high-risk files (threshold 0.8 = high risk)
omen -f json defect

# Find hotspots
omen -f json hotspot

# Check for explicit markers
omen -f json satd | jq '.items[] | select(.category == "defect")'
```

## Priority Order

1. **Defect probability above 0.8**: Investigate first.
2. **Hotspot score above 0.5**: Review the complex logic.
3. **BUG and FIXME markers**: These are known issues.
4. **Temporal coupling**: Check the related files.

## In Each File

- Look for functions with a cognitive complexity above 20.
- Check the error-handling paths.
- Review the boundary conditions.
