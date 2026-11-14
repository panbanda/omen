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

Get the metrics before you change: `{{.focus}}`

## Quick Start

```bash
# Get structural outline of the target file
omen -f json outline --compact --file "{{.focus}}"

# Inspect a specific symbol's definition and callers
omen -f json symbol "{{.focus}}"

# Get full deep context for LLM-assisted editing
omen -f json context
```

## What You Get

- **Complexity**: The cyclomatic and cognitive scores of each function.
- **Debt**: The TODO, FIXME, and HACK markers with their severity.
- **Risk**: The combined assessment for the target.

## Thresholds

| Metric | Safe | Warning | Danger |
|--------|------|---------|--------|
| Cyclomatic | <10 | 10-20 | >20 |
| Cognitive | <15 | 15-30 | >30 |

## Next Actions

1. **High complexity**: Refactor before you make changes.
2. **Critical debt**: Address the security and bug markers first.
3. **Clean**: Continue with your change.
