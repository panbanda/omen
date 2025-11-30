---
name: test-targeting
description: |
  Identify which files and functions most need additional test coverage based on risk, complexity, and churn. Use this skill when prioritizing test writing efforts or improving coverage strategically.
---

# Test Targeting

Identify the highest-value targets for additional test coverage by combining risk, complexity, and behavioral signals.

## When to Use

- Prioritizing test writing efforts
- Improving coverage strategically (not just for metrics)
- Identifying undertested risky code
- Planning a testing sprint

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

### Step 1: Identify High-Risk Files

Use the `analyze_defect` tool:

```
analyze_defect(paths: ["."])
```

Files with high defect probability are statistically likely to contain bugs.

### Step 2: Find Hotspots

Use the `analyze_hotspot` tool:

```
analyze_hotspot(paths: ["."])
```

High-churn + high-complexity code needs regression protection.

### Step 3: Analyze Complexity

Use the `analyze_complexity` tool:

```
analyze_complexity(paths: ["."])
```

Complex functions have many code paths that need coverage.

### Step 4: Check Ownership

Use the `analyze_ownership` tool:

```
analyze_ownership(paths: ["."])
```

Single-owner code needs tests as documentation for when others maintain it.

## Test Priority Matrix

Prioritize based on combined signals:

| Priority | Signals | Why Test |
|----------|---------|----------|
| Critical | High risk + High complexity + High churn | Bugs likely, changes often, hard to verify |
| High | High complexity + Single owner | Many paths, tests = documentation |
| Medium | High churn + Moderate complexity | Changes often, needs regression protection |
| Lower | Low complexity + Low churn | Stable, simple code |

## Test Case Estimation

Use cyclomatic complexity to estimate test cases needed:

| Cyclomatic Complexity | Minimum Test Cases |
|----------------------|-------------------|
| 1-5 | 2-5 tests |
| 6-10 | 6-15 tests |
| 11-20 | 16-40 tests |
| 21+ | Consider refactoring first |

## Output Format

Generate a test targeting report:

```markdown
# Test Targeting Report

## Critical Coverage Gaps

### `src/payment/processor.go:processPayment`
- **Defect probability**: 0.82
- **Cyclomatic complexity**: 18
- **Current coverage**: Unknown/Low
- **Estimated tests needed**: 25-35 test cases
- **Why**: High-risk financial logic, frequently modified

### `src/auth/validator.go:validateToken`
- **Defect probability**: 0.75
- **Cyclomatic complexity**: 12
- **Bus factor**: 1 (alice)
- **Estimated tests needed**: 15-20 test cases
- **Why**: Security-critical, single owner, needs documentation

## High-Churn Files (Regression Priority)

| File | Churn (30d) | Complexity | Priority |
|------|-------------|------------|----------|
| api/handlers.go | 45 changes | 24 | High |
| core/engine.go | 32 changes | 31 | High |
| utils/helpers.go | 28 changes | 8 | Medium |

## Knowledge Silo Files

Files where tests serve as critical documentation:

1. `legacy/importer.go` - Only bob has touched this
   - Add characterization tests before bob leaves
   - Focus on capturing current behavior

2. `integration/sync.go` - Single contributor, complex
   - Tests document the sync protocol
   - Critical for future maintainers

## Testing Strategy by Area

### Unit Tests Needed
- `processor.go` - All code paths in processPayment
- `validator.go` - All validation rules
- Edge cases in complex conditionals

### Integration Tests Needed
- `api/handlers.go` - Request/response flows
- `storage/repository.go` - Database interactions

### Characterization Tests Needed
- `legacy/` - Capture current behavior before refactoring

## Suggested Order

1. **Week 1**: Critical coverage gaps (highest risk reduction)
2. **Week 2**: High-churn regression tests
3. **Week 3**: Knowledge silo characterization tests
4. **Ongoing**: Maintain coverage on new code
```

## Testing ROI

Focus on tests that provide the most value:

1. **Risk reduction**: Test high-defect-probability code first
2. **Regression protection**: Test frequently-changing code
3. **Documentation**: Test single-owner complex code
4. **Refactoring enablement**: Test before major changes
