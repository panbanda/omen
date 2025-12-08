---
name: check-quality
description: Check code quality against thresholds for complexity, duplication, and defect risk. Use for pre-merge quality checks, release readiness validation, or enforcing team standards.
---

# Check Quality

Perform a pass/fail quality gate check against configurable thresholds to enforce code quality standards.

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

## Default Thresholds

| Metric | Threshold | Rationale |
|--------|-----------|-----------|
| TDG Average Grade | B or better | Maintainable code |
| Max Cyclomatic Complexity | 15 per function | Testable functions |
| Max Cognitive Complexity | 20 per function | Readable code |
| Duplication Ratio | < 5% | DRY principle |
| High-Risk Files | < 10% of codebase | Manageable risk |
| Critical SATD Items | 0 | No known critical issues |

## Workflow

### Step 1: Check TDG Grade

Use the `analyze_tdg` tool:

```
analyze_tdg(paths: ["."])
```

**Pass**: Average grade B or better (TDG score >= 60)
**Fail**: Average grade C or worse

### Step 2: Check Complexity

Use the `analyze_complexity` tool:

```
analyze_complexity(paths: ["."])
```

**Pass**: No function exceeds thresholds
**Fail**: Any function with cyclomatic > 15 or cognitive > 20

### Step 3: Check Duplication

Use the `analyze_duplicates` tool:

```
analyze_duplicates(paths: ["."])
```

**Pass**: Duplication ratio < 5%
**Fail**: Duplication ratio >= 5%

### Step 4: Check Defect Risk

Use the `analyze_defect` tool:

```
analyze_defect(paths: ["."])
```

**Pass**: < 10% of files are high-risk
**Fail**: >= 10% of files are high-risk

### Step 5: Check SATD

Use the `analyze_satd` tool:

```
analyze_satd(paths: ["."])
```

**Pass**: No critical severity items
**Fail**: Any critical SATD markers present

## Output Format

Generate a quality gate report:

```markdown
# Quality Gate Report

## Overall Status: PASS / FAIL

## Metric Results

| Metric | Threshold | Actual | Status |
|--------|-----------|--------|--------|
| TDG Grade | >= B | B+ (68) | PASS |
| Max Cyclomatic | <= 15 | 18 | FAIL |
| Max Cognitive | <= 20 | 15 | PASS |
| Duplication | < 5% | 3.2% | PASS |
| High-Risk Files | < 10% | 5% | PASS |
| Critical SATD | 0 | 2 | FAIL |

## Violations

### Complexity Violations
1. `processor.go:processPayment` - Cyclomatic: 18 (threshold: 15)
2. `handler.go:handleRequest` - Cyclomatic: 16 (threshold: 15)

### Critical SATD
1. `auth.go:45` - FIXME: Security vulnerability needs fix
2. `payment.go:123` - HACK: Race condition workaround

## Trend (vs. Last Check)

| Metric | Previous | Current | Trend |
|--------|----------|---------|-------|
| TDG Grade | C+ | B+ | Improved |
| Violations | 5 | 2 | Improved |
| Duplication | 4.1% | 3.2% | Improved |

## Remediation Required

To pass the quality gate:

1. **Reduce complexity** in `processor.go:processPayment`
   - Extract helper functions
   - Simplify conditional logic

2. **Address critical SATD**
   - Fix security issue in auth.go
   - Resolve race condition in payment.go

## Estimated Effort

- Complexity fixes: ~4 hours
- SATD remediation: ~6 hours
- **Total to pass**: ~10 hours
```

## CI/CD Integration

For automated quality gates, use Omen CLI:

```bash
# Run quality gate check
omen analyze tdg --format json | jq '.average_score >= 60'

# Check for complexity violations
omen analyze complexity --format json | jq '[.functions[] | select(.cyclomatic > 15)] | length == 0'
```

## Customizing Thresholds

Adjust thresholds based on project maturity:

### Greenfield Projects (Strict)
- TDG Grade: A
- Max Cyclomatic: 10
- Duplication: < 3%

### Legacy Projects (Relaxed)
- TDG Grade: C
- Max Cyclomatic: 20
- Duplication: < 10%

### Critical Systems (Extra Strict)
- TDG Grade: A
- Max Cyclomatic: 8
- Critical SATD: 0
- High-Risk Files: < 5%
