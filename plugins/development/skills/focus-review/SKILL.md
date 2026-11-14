---
name: focus-review
description: Focus code review on high-risk changes
usage: /focus-review <changed-files>
arguments:
  - name: changed_files
    description: Files to review
    required: true
---

# Focus Review Skill

Focus the review on: `{{.changed_files}}`

## Quick Start

```bash
# Get structural outline of changed files
omen -f json outline --compact --top 30

# Check blast radius of changed symbols
omen -f json impact <changed-symbol> --depth 2

# Check risk of changed files
omen -p "{{.changed_files}}" -f json defect

# Check complexity
omen -p "{{.changed_files}}" -f json complexity

# Check for new debt
omen -p "{{.changed_files}}" -f json satd
```

## Review Checklist

- [ ] Files with a defect probability above 0.7: Give extra scrutiny.
- [ ] Functions with a cyclomatic complexity above 15: Simplify them.
- [ ] New HACK or FIXME markers: Require a justification.
- [ ] Orphaned code: Remove it.

## Comment Format

```
**[PRIORITY]**: file:line
What: Description
Why: Impact
Fix: Suggestion
```
