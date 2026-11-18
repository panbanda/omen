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

# Inspect a specific symbol's definition and callers. An ambiguous name exits with 1 and lists
# each definition with the command that selects it.
omen -f json symbol "{{.focus}}"

# Get the deep context of the whole repository (the command has no target argument)
omen -f json context
```

## What You Get

- **Complexity**: The functions with a cyclomatic complexity of 10 or more, or a cognitive complexity of 15 or more.
- **Debt**: The TODO, FIXME, and HACK markers with their severity.
- **Risk**: The ranked symbols and risks of the whole repository. The `context` command has no target argument.

## Thresholds

| Metric | Safe | Warning | Danger |
|--------|------|---------|--------|
| Cyclomatic | <10 | 10-20 | >20 |
| Cognitive | <15 | 15-30 | >30 |

## Next Actions

1. **High complexity**: Find the risks that are in the target file. Refactor before you make changes.
2. **Critical debt**: Address the security and bug markers first.
3. **Clean**: Continue with your change.
