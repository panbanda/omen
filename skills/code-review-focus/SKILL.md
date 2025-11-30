---
name: code-review-focus
description: |
  Identify what to focus on when reviewing code changes, including complexity deltas, duplication, and risk assessment. Use this skill when reviewing pull requests or preparing code for review.
---

# Code Review Focus

Identify the highest-priority areas to focus on during code review by analyzing complexity, duplication, and risk signals.

## When to Use

- Reviewing a pull request
- Preparing code for review
- Prioritizing review effort on large PRs
- Identifying potential issues before merge

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

### Step 1: Analyze Changed File Complexity

Use the `analyze_complexity` tool on the changed files:

```
analyze_complexity(paths: ["path/to/changed/file1.go", "path/to/changed/file2.go"])
```

Look for:
- Functions with cyclomatic complexity > 10
- Functions with cognitive complexity > 15
- Large increases from previous version

### Step 2: Check for New Duplicates

Use the `analyze_duplicates` tool to find introduced clones:

```
analyze_duplicates(paths: ["."])
```

Review if the PR introduces code that duplicates existing code.

### Step 3: Check for Dead Code

Use the `analyze_deadcode` tool to find unused code:

```
analyze_deadcode(paths: ["."])
```

Catch dead code before it's merged.

### Step 4: Assess Risk

Use the `analyze_defect` tool to check risk level:

```
analyze_defect(paths: ["path/to/changed/files"])
```

Higher defect probability = more scrutiny needed.

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
