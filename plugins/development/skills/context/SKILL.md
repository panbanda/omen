---
name: context
description: Get complexity and debt metrics before editing code
usage: /context <file-or-symbol>
arguments:
  - name: focus
    description: File path or symbol name
    required: true
---

# Context Skill

Get metrics before modifying: `{{.focus}}`

## Quick Start

```bash
omen context --focus "{{.focus}}" --format json
```

## What You Get

- **Complexity**: Cyclomatic and cognitive scores per function
- **Debt**: TODO/FIXME/HACK markers with severity
- **Risk**: Combined assessment for the target

## Thresholds

| Metric | Safe | Warning | Danger |
|--------|------|---------|--------|
| Cyclomatic | <10 | 10-20 | >20 |
| Cognitive | <15 | 15-30 | >30 |

## Next Actions

1. **High complexity**: Consider refactoring before changes
2. **Critical debt**: Address security/bug markers first
3. **Clean**: Proceed with your modification
