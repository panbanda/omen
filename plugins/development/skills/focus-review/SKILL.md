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
# Get the change set of the branch: the changed symbols with the most effect first
omen -f json diff --target main --top 20

# Get structural outline of changed files
omen -f json outline --compact --top 30

# Check blast radius of one changed symbol. Use a `key` from `symbols` of the change set.
omen -f json impact <symbol-key> --depth 2

# Check risk of changed files
omen -f json defect --glob "{{.changed_files}}"

# Check complexity
omen -f json complexity --glob "{{.changed_files}}"

# Check for new debt
omen -f json satd --glob "{{.changed_files}}"
```

## Read the Change Set

The `diff` command gives these keys for a review:

- `summary`: the counts of the full change set. `--top` does not change them.
- `symbols`: the changed symbols. Symbols that are not test code come first, then changed and removed symbols, then the symbols with more transitive callers.
- `symbols[].signature_before` and `symbols[].signature_after`: a difference is an interface change. Examine each entry of `direct_callers`.
- `symbols[].complexity_before` and `symbols[].complexity_after`: an increase shows where the logic grew.
- `symbols[].test_caller_count`: the value 0 on a changed symbol means that no test calls the symbol directly.
- `new_findings`: the new stubs and the new debt comments.
- `security`: the sinks, the entry points, and the paths that the branch adds or removes.

If `symbols_omitted` is present, use `--offset` to get the next page.

## Review Checklist

- [ ] Changed or removed symbols with a signature change: Examine each direct caller.
- [ ] Changed symbols with `test_caller_count` 0 and many transitive callers: Ask for a test.
- [ ] New sinks or new paths in `security`: Examine the input that reaches them.
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
