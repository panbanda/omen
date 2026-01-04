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
omen analyze defect --paths "{{.changed_files}}" --format json

# Check complexity
omen analyze complexity --paths "{{.changed_files}}" --format json

# Check for new debt
omen analyze satd --paths "{{.changed_files}}" --format json
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
