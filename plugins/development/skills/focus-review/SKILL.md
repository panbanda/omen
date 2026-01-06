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

Review focus for: `{{.changed_files}}`

## Quick Start

```bash
# Check risk of changed files
omen defect -p "{{.changed_files}}" -f json

# Check complexity
omen complexity -p "{{.changed_files}}" -f json

# Check for new debt
omen satd -p "{{.changed_files}}" -f json
```

## Review Checklist

- [ ] Files with defect prob > 0.7: Extra scrutiny
- [ ] Functions with cyclomatic > 15: Simplification needed
- [ ] New HACK/FIXME: Justification required
- [ ] Orphaned code: Remove

## Comment Format

```
**[PRIORITY]**: file:line
What: Description
Why: Impact
Fix: Suggestion
```
