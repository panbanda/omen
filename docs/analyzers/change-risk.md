---
sidebar_position: 7
---

# Change Risk Analysis (JIT)

Omen's change risk analyzer performs Just-In-Time (JIT) defect prediction at the commit level. Rather than asking "is this file likely to have bugs?" (which is what file-level defect prediction does), JIT prediction asks "is this commit likely to introduce a bug?"

This distinction matters because it produces actionable results at the moment code is being written, not after the fact.

## JIT Risk Factors

The analyzer evaluates each commit using factors derived from the Kamei et al. (2013) framework. These factors capture the size, scope, and context of a change:

| Factor | Abbreviation | Description |
|---|---|---|
| Lines Added | LA | Total lines added across all files in the commit |
| Lines Deleted | LD | Total lines deleted across all files in the commit |
| Lines in Touched Files | LT | Total size (in lines) of all files modified by the commit |
| Bug Fix | FIX | Whether the commit message indicates a bug fix (e.g., contains "fix", "bug", "patch") |
| Number of Developers | NDEV | Number of distinct developers who have previously modified the touched files |
| Average File Age | AGE | Mean age (in days) of the files modified by the commit |
| Unique Changes | NUC | Number of unique files changed by the commit |
| Developer Experience | EXP | Number of prior commits by this developer to the repository |

Each factor is computed from Git history and the commit diff. No source code parsing is required, which makes JIT analysis fast and language-agnostic.

### How Factors Interact

No single factor determines risk. A commit that adds 500 lines to a single new file (high LA, low NUC, low NDEV) is a different risk profile from a commit that adds 500 lines spread across 20 files with 15 prior contributors (high LA, high NUC, high NDEV). The latter is substantially more likely to introduce a defect because the change is scattered across code that many people have touched.

The FIX indicator is worth noting: bug-fix commits are themselves more likely to introduce new bugs. This is a well-documented phenomenon in the literature. Fixes are often written under time pressure, applied to complex code that was already problematic, and tend to receive less review than new feature code.

## Risk Classification

Omen uses percentile-based thresholds to classify commit risk:

| Risk Level | Percentile | Meaning |
|---|---|---|
| **High** | Top 5% (P95+) | Commit's risk score is in the 95th percentile or above |
| **Medium** | Top 20% (P80--P95) | Commit's risk score falls between the 80th and 95th percentiles |
| **Low** | Bottom 80% (&lt;P80) | Commit's risk score is below the 80th percentile |

These thresholds are calibrated to the repository's own history. A "high risk" commit in one repository is high relative to that repository's baseline, not an absolute scale. This aligns with the 80/20 rule observed in defect prediction research: roughly 20% of changes introduce roughly 80% of defects.

The percentile approach avoids the problem of fixed thresholds that don't transfer across projects. A mature, stable repository with small, careful commits will have different absolute risk values than a fast-moving startup codebase, but the relative ranking still identifies the most dangerous changes.

## Usage

```bash
# Analyze recent commits for change risk
omen changes

# JSON output
omen -f json changes

# Analyze a specific path
omen -p ./src changes

# Analyze a remote repository
omen -p expressjs/express changes
```

### Example Output

```
Change Risk Analysis (JIT)
==========================

Risk: HIGH (P97)
  abc1234 "Refactor auth middleware and update 12 route handlers"
  LA: 847  LD: 312  NUC: 14  NDEV: 8  EXP: 23  AGE: 340d  FIX: no

Risk: HIGH (P96)
  def5678 "Fix race condition in session cleanup"
  LA: 156  LD: 89  NUC: 6  NDEV: 11  EXP: 5  AGE: 890d  FIX: yes

Risk: MEDIUM (P88)
  fed9876 "Add rate limiting to public API endpoints"
  LA: 234  LD: 12  NUC: 4  NDEV: 3  EXP: 67  AGE: 120d  FIX: no

Risk: LOW (P42)
  321dcba "Update error messages in validation module"
  LA: 18  LD: 15  NUC: 1  NDEV: 2  EXP: 145  AGE: 60d  FIX: no

Commits analyzed: 87
High risk: 4 (4.6%)
Medium risk: 14 (16.1%)
Low risk: 69 (79.3%)
```

## Reading the Results

Patterns to watch for in high-risk commits:

- **High LA + High NUC**: large changes spread across many files. These are hard to review and easy to get wrong.
- **FIX = yes + High NDEV**: a bug fix in code that many developers have touched. The "too many cooks" effect increases the chance that the fix breaks an assumption another contributor relied on.
- **Low EXP + High LT**: an inexperienced developer (relative to this repository) modifying large, established files. Not a judgment of the developer -- it's a signal that the change may lack context about implicit constraints in the code.
- **High AGE + High LD**: deleting lines from old, stable code. Old code that hasn't been touched may have survived because it works. Removing parts of it can break invariants that aren't documented or tested.

## Configuration

In `omen.toml`:

```toml
[changes]
# Number of recent commits to analyze
limit = 100

# Percentile thresholds for risk classification
high_percentile = 95
medium_percentile = 80
```

## Comparison with File-Level Defect Prediction

JIT and file-level defect prediction answer different questions and are complementary:

| Dimension | File-Level (`omen defect`) | JIT (`omen changes`) |
|---|---|---|
| Unit of analysis | File | Commit |
| When it's useful | Planning, refactoring prioritization | Code review, CI gates |
| Data required | Git history + source code parsing | Git history only |
| Latency | Needs tree-sitter parsing | Fast (diff-only) |
| Granularity | Which files are risky | Which changes are risky |

Use `omen defect` when you want to understand the overall risk landscape of a codebase. Use `omen changes` when you want to evaluate recent activity and flag dangerous commits for review.

## Research Background

**Kamei et al. (2013), "A Large-Scale Empirical Study of Just-In-Time Quality Assurance" (IEEE Transactions on Software Engineering).** This paper introduced the JIT defect prediction framework that Omen's change risk analyzer is based on. Kamei et al. studied six open source and five commercial projects, demonstrating that commit-level prediction using the factors listed above (LA, LD, LT, FIX, NDEV, AGE, NUC, EXP) can identify 20% of changes that contain roughly 75% of bugs. The key insight is that effort-aware prediction at the commit level is more actionable than file-level prediction because developers can immediately decide whether to invest more review time in a specific change.

**Zeng et al. (2021), "Deep Just-In-Time Defect Prediction: How Far Are We?" (ACM SIGSOFT International Symposium on Software Testing and Analysis).** This study compared deep learning approaches to JIT defect prediction against simpler logistic regression and random forest models. The finding that matters for Omen's design: simple models achieve approximately 65% accuracy on JIT prediction tasks, and deep learning models do not significantly outperform them. What simple models lose in marginal accuracy, they gain in interpretability -- you can look at the factor values and understand why a commit was flagged. Omen prioritizes this interpretability, showing the raw factor values alongside the risk classification so that developers can make informed decisions rather than trusting an opaque score.
