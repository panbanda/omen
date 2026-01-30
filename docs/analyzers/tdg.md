---
sidebar_position: 4
---

# Technical Debt Gradient (TDG)

```bash
omen tdg
```

The Technical Debt Gradient analyzer produces a composite health score for each file in the codebase, ranging from 0 (worst) to 100 (best). It aggregates nine weighted dimensions of code quality into a single number that can be tracked over time, compared across modules, and used to prioritize refactoring work.

## What Technical Debt Is

Ward Cunningham coined the "technical debt" metaphor in 1992 to describe the gap between the current state of a codebase and the state it should be in. Like financial debt, technical debt accumulates interest: each shortcut or deferred cleanup makes future changes more expensive. Unlike financial debt, technical debt is often invisible until it causes a production incident or blocks a feature that should have been simple.

The challenge with technical debt is measurement. "This code feels messy" is not actionable. The TDG analyzer replaces subjective assessments with a quantified breakdown of what specifically is wrong, how much it contributes to the overall health of the file, and how it is trending.

## Scoring Dimensions

The TDG score is composed of nine weighted components. Each component contributes a maximum number of points to the total. A perfect file scores 100.

| Component | Max Points | What It Measures |
|-----------|-----------|------------------|
| Structural Complexity | 20 | Cyclomatic complexity and nesting depth across all functions in the file |
| Semantic Complexity | 15 | Cognitive complexity aggregated across functions |
| Duplication | 15 | Ratio of cloned code to total code in the file |
| Coupling | 15 | Number of outgoing dependencies (imports, calls to external modules) |
| Hotspot | 10 | Interaction of churn and complexity (files changed often with high complexity lose points) |
| Temporal Coupling | 10 | How frequently this file changes together with unrelated files |
| Consistency | 10 | Adherence to codebase-level style patterns (naming, formatting, structure) |
| Entropy | 10 | Uniformity of code patterns within the file |
| Documentation | 5 | Comment coverage ratio |

### Structural Complexity (20 points)

Deductions are based on the worst-case cyclomatic complexity and maximum nesting depth among all functions in the file. A file where every function is below the warning thresholds keeps all 20 points. Functions exceeding the error thresholds incur the maximum penalty.

### Semantic Complexity (15 points)

Cognitive complexity captures readability problems that cyclomatic complexity misses. The scoring is based on the aggregate cognitive score across the file, normalized by line count. This prevents large files from being penalized simply for having more functions.

### Duplication (15 points)

Points are deducted proportionally to the percentage of the file that participates in clone groups (Type 1, 2, or 3 clones as detected by the clone analyzer). A file with no duplicated blocks keeps all 15 points. A file where 50% of the code is cloned loses roughly half.

### Coupling (15 points)

Measures the number of outgoing dependencies: imports, `use` statements, `require` calls, and similar constructs. Files that depend on many other modules are harder to change safely, harder to test, and harder to move. The deduction is proportional to the dependency count relative to a configurable baseline.

### Hotspot (10 points)

A hotspot is a file that is both complex and frequently changed. Neither property alone is a problem: a complex file that rarely changes is stable; a simple file that changes often is fine. But complexity combined with churn is a strong predictor of defects (see Tornhill, 2015). Points are deducted based on the product of normalized complexity and normalized churn.

### Temporal Coupling (10 points)

Files that consistently change together with other files may have hidden dependencies that the import graph does not reveal. Points are deducted for high co-change frequency with files outside the same logical module. This is computed from Git history when available.

### Consistency (10 points)

Measures how well the file conforms to patterns observed across the rest of the codebase. Inconsistent naming conventions, unusual formatting, or structural outliers lose points. This is not a linter -- it measures relative consistency rather than absolute adherence to a style guide.

### Entropy (10 points)

Measures the uniformity of patterns within the file itself. A file that uses three different error handling strategies, mixes callbacks and async/await, or has wildly varying function sizes scores lower. The score captures internal incoherence rather than cross-file inconsistency.

### Documentation (5 points)

Comment coverage as a ratio of comment lines to total lines. This is weighted lightly because comment presence is a weak signal: comments can be outdated, redundant, or misleading. But a file with zero comments on complex logic is more likely to be misunderstood.

## Letter Grades

The numeric score maps to a letter grade for quick assessment:

| Score | Grade | Interpretation |
|-------|-------|---------------|
| 90-100 | A | Healthy. Low risk, easy to maintain. |
| 80-89 | B | Good. Minor issues, manageable debt. |
| 70-79 | C | Fair. Noticeable debt accumulation. Should be addressed soon. |
| 60-69 | D | Poor. Significant debt. Active risk to reliability and velocity. |
| 0-59 | F | Critical. Immediate attention required. High defect and maintenance risk. |

## Critical Defect Detection

In addition to the composite score, TDG performs AST-based detection of language-specific dangerous patterns. These are not style issues -- they are patterns that are known to cause defects in production.

Examples by language:

- **JavaScript/TypeScript:** `==` used where `===` was likely intended; `var` in modern codebases; missing `await` on async calls
- **Python:** bare `except` clauses catching all exceptions; mutable default arguments
- **Go:** unchecked errors from functions that return `error`
- **Rust:** `.unwrap()` in non-test code; `unsafe` blocks
- **C/C++:** unchecked pointer dereference; buffer size mismatches

Critical defects are reported separately from the score and do not affect the numeric grade. They represent acute risks rather than gradual debt.

## Output

```bash
# Default output with grades
omen tdg

# JSON for tracking over time
omen -f json tdg

# Analyze a specific module
omen -p ./src/billing tdg
```

The output lists each file with its total score, letter grade, and per-dimension breakdown. Files are sorted by score ascending (worst first) so that the most problematic files appear at the top.

## Tracking Debt Over Time

The TDG score is designed to be tracked across commits. By running `omen -f json tdg` in CI and storing the results, teams can build a time series of per-file and per-module health scores. This makes it possible to answer questions like:

- Is technical debt increasing or decreasing in this module?
- Which files have degraded the most in the last sprint?
- Are refactoring efforts actually improving the scores?

## References

- Cunningham, W. (1992). "The WyCash Portfolio Management System." *OOPSLA '92 Experience Report*.
- Kruchten, P., Nord, R.L., & Ozkaya, I. (2012). "Technical Debt: From Metaphor to Theory and Practice." *IEEE Software*, 29(6), 18-21.
- Tornhill, A. (2015). *Your Code as a Crime Scene*. Pragmatic Bookshelf.
- Li, Z., Avgeriou, P., & Liang, P. (2015). "A Systematic Mapping Study on Technical Debt and its Management." *Journal of Systems and Software*, 101, 193-220.
