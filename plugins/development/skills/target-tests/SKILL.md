---
name: target-tests
description: Identify files and functions most needing test coverage based on risk, complexity, and churn. Use when prioritizing test writing or improving coverage strategically.
---

# Target Tests

Identify the targets that give the most value for more test coverage. Combine the risk, complexity, and behavioral signals.

## Prerequisites

The Omen CLI must be installed and available in PATH.

## Workflow

### Step 1: Identify the High-Risk Files

Run the defect prediction analysis:

```bash
omen -f json defect
```

A file with a high defect probability is statistically likely to contain bugs.

### Step 2: Find the Hotspots

Run the hotspot analysis:

```bash
omen -f json hotspot
```

Code with high churn and high complexity needs protection against regressions.

### Step 3: Analyze the Complexity

Run the complexity analysis:

```bash
omen -f json complexity
```

A complex function has many code paths. Each path needs coverage.

### Step 4: Check the Ownership

Run the ownership analysis:

```bash
omen -f json ownership
```

Code with one owner needs tests as documentation. Other developers will maintain the code later.

## Test Priority Matrix

Set the priority from the combined signals:

| Priority | Signals | Why Test |
|----------|---------|----------|
| Critical | High risk + High complexity + High churn | Bugs likely, changes often, hard to verify |
| High | High complexity + Single owner | Many paths, tests = documentation |
| Medium | High churn + Moderate complexity | Changes often, needs regression protection |
| Lower | Low complexity + Low churn | Stable, simple code |

## Test Case Estimation

Use the cyclomatic complexity to estimate the number of test cases:

| Cyclomatic Complexity | Minimum Test Cases |
|----------------------|-------------------|
| 1-5 | 2-5 tests |
| 6-10 | 6-15 tests |
| 11-20 | 16-40 tests |
| 21+ | Consider refactoring first |

## Output Format

Make a test targeting report in this format:

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

Write the tests that give the most value:

1. **Risk reduction**: Test the code with a high defect probability first.
2. **Regression protection**: Test the code that changes often.
3. **Documentation**: Test complex code that has one owner.
4. **Refactoring enablement**: Test before a major change.
