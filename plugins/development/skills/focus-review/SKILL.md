---
name: focus-review
description: Identify high-priority areas to focus on during code review. Use when reviewing large PRs, preparing code for review, or prioritizing review effort based on complexity and risk.
---

# Focus Review

Identify the highest-priority areas to focus on during code review by analyzing commit risk, complexity, duplication, and defect signals.

## Prerequisites

Omen must be available as an MCP server. Add to Claude Code settings:

```json
{
  "mcpServers": {
    "omen": {
      "command": "omen",
      "args": ["mcp"]
    }
  }
}
```

## Workflow

### Step 1: Analyze Commit Risk (JIT)

Use the `analyze_changes` tool to score commit risk:

```
analyze_changes(paths: ["."], days: 90)
```

JIT defect prediction (Kamei et al. 2013) scores commits based on:
- Lines added/deleted
- Files touched
- Fix patterns in commit message
- Author experience on these files
- Entropy of changes

High-risk commits (>0.7) need senior review.

### Step 2: Analyze Changed File Complexity

Use the `analyze_complexity` tool on the changed files:

```
analyze_complexity(paths: ["path/to/changed/file1.go", "path/to/changed/file2.go"])
```

Look for:
- Functions with cyclomatic complexity > 10
- Functions with cognitive complexity > 15
- Large increases from previous version

### Step 3: Check for New Duplicates

Use the `analyze_duplicates` tool to find introduced clones:

```
analyze_duplicates(paths: ["."])
```

Review if the PR introduces code that duplicates existing code.

### Step 4: Check for Dead Code

Use the `analyze_deadcode` tool to find unused code:

```
analyze_deadcode(paths: ["."])
```

Catch dead code before it's merged.

### Step 5: Assess File Risk

Use the `analyze_defect` tool to check risk level:

```
analyze_defect(paths: ["path/to/changed/files"])
```

Higher defect probability = more scrutiny needed.

### Step 6: Check for New Tech Debt

Use the `analyze_satd` tool:

```
analyze_satd(paths: ["path/to/changed/files"])
```

New TODO/FIXME/HACK markers should be justified or have tickets.

## Review Priority Matrix

Focus review effort based on signals:

| Priority | Signals | Review Depth |
|----------|---------|--------------|
| Critical | New code with complexity > 20 | Line-by-line |
| High | Touches high-risk file + adds complexity | Detailed review |
| Medium | Moderate complexity, no duplication | Standard review |
| Low | Simple changes, well-tested areas | Quick scan |

## Output Format

Generate a review focus guide:

```markdown
# Code Review Focus: PR #123

## High-Priority Review Areas

### `src/payment/processor.go` (CRITICAL)
- **New function** `processRefund` - Cyclomatic: 18, Cognitive: 22
- Touches existing high-risk code (defect probability: 0.75)
- **Focus on**: Error handling, edge cases, transaction safety

### `src/api/handlers.go` (HIGH)
- **Modified function** `handleCheckout` - Complexity increased +5
- **Focus on**: Input validation, state management

## Potential Issues Detected

### Duplication
- Lines 45-67 in `processor.go` duplicate `validator.go:23-45`
- **Suggestion**: Extract to shared function

### Dead Code
- `unusedHelper()` in `utils.go` appears unused after this PR
- **Suggestion**: Remove if confirmed unused

## Lower Priority (Quick Scan)
- `tests/processor_test.go` - Test additions, standard review
- `docs/api.md` - Documentation update

## Suggested Review Order
1. `processor.go:processRefund` - Most complex, highest risk
2. `handlers.go:handleCheckout` - Complexity increase
3. Verify duplication is intentional or refactor
4. Quick scan of tests and docs
```

## Review Checklist

For high-priority areas, verify:

- [ ] Error handling covers all failure modes
- [ ] Edge cases are handled
- [ ] No implicit dependencies introduced
- [ ] Tests cover new complexity
- [ ] No security vulnerabilities (injection, auth bypass)
- [ ] Performance implications considered
