---
sidebar_position: 10
---

# Git Churn Analysis

Churn analysis measures how frequently and extensively files have been modified over a configurable time period. Omen extracts churn data directly from Git history using the `gix` library (a pure-Rust Git implementation), so no shell-out to `git` is required.

Churn is one of the most reliable predictors of software defects. Files that change often are files where bugs appear, regardless of language, team size, or development methodology.

## Metrics

Omen computes three churn metrics per file:

| Metric | Description |
|---|---|
| **Commit count** | Number of commits that modified the file within the analysis window |
| **Lines added** | Total lines added to the file across all commits in the window |
| **Lines deleted** | Total lines deleted from the file across all commits in the window |
| **Contributors** | Number of distinct authors who modified the file in the window |

### Commit Count

The most straightforward churn metric. A file with 50 commits in six months is being changed roughly twice a week. This signals one of several things:

- The file is central to the system and gets modified as part of many features.
- The file is poorly designed and requires constant adjustment.
- The file is missing the right abstractions, so higher-level changes propagate down to it.

Commit count alone does not distinguish between these causes, but it reliably identifies the files that deserve further investigation.

### Lines Added and Deleted

Volume metrics complement commit count. A file with 5 commits that each change 200 lines is a different risk profile from a file with 50 commits that each change 2 lines. The former suggests major rewrites; the latter suggests frequent minor adjustments.

The ratio of additions to deletions also reveals patterns:

| Pattern | Interpretation |
|---|---|
| Additions >> Deletions | File is growing. May be accumulating responsibilities. |
| Additions ~= Deletions | File is being rewritten or refactored in place. |
| Deletions >> Additions | File is shrinking. Code is being removed or extracted elsewhere. |

### Contributors

The number of distinct contributors is both a churn metric and an ownership signal. Files modified by many developers tend to have higher defect rates because:

- No single person has full context on the file's behavior and invariants.
- Different developers may have conflicting assumptions about how the code should evolve.
- Review quality may suffer when reviewers are unfamiliar with the file's history.

## Time Period

Churn analysis uses a configurable time window. The default is 6 months.

| Period | Flag Value | Use Case |
|---|---|---|
| 1 month | `1m` | Recent activity. Useful for sprint-level analysis. |
| 3 months | `3m` | Short-term trends. Good for quarterly reviews. |
| 6 months | `6m` | Default. Balances recency with sufficient sample size. |
| 1 year | `1y` | Medium-term view. Captures seasonal patterns. |
| 2 years | `2y` | Long-term trends. Shows chronic hotspots. |
| All history | `all` | Full repository lifetime. May be slow on large repos. |

Shorter windows emphasize recent activity and are more responsive to changes in development patterns. Longer windows smooth out noise and reveal chronic high-churn files that may not stand out in any single quarter.

## Usage

```bash
# Run churn analysis with default settings (6 months)
omen churn

# Specify a time period
omen churn --since 3m
omen churn --since 1y
omen churn --since all

# JSON output
omen -f json churn

# Analyze a specific path
omen -p ./src churn

# Analyze a remote repository
omen -p django/django churn
```

### Example Output

```
Git Churn Analysis (last 6 months)
===================================

  File                              Commits   Added   Deleted   Contributors
  src/engine/query_planner.rs            47    2,841     1,203            9
  src/parser/expression.rs               38    1,567       892            6
  src/api/handlers.rs                    31      945       412            4
  src/cli/main.rs                        28      623       301            5
  src/config/loader.rs                   22      334       187            3
  ...

  Top 20 of 214 files shown.
  Total commits in window: 342
  Total files modified: 214
```

## Configuration

In `omen.toml`:

```toml
[churn]
# Time window for analysis
since = "6m"

# Number of top files to display
top = 20
```

## What High Churn Indicates

High churn is a symptom, not a diagnosis. It can indicate several underlying conditions:

**Central to the system.** Some files are legitimately high-churn because they are the integration point for the rest of the codebase. A router configuration file, a dependency injection container, or a schema definition will be touched by many features. High churn in these files is expected and not necessarily a problem, but it does mean these files should be well-tested and well-reviewed.

**Poorly designed.** When a file changes every time any feature is added, it may be violating the open-closed principle. The file should be designed so that new behavior can be added by extension rather than modification. High churn combined with high complexity is a strong signal of this problem.

**Missing abstractions.** If multiple files always change together, they may be tightly coupled through shared assumptions rather than explicit interfaces. The churn data reveals this coupling even when static analysis does not, because the coupling exists in the development process rather than in the code structure.

**Unstable requirements.** Sometimes high churn reflects external factors: requirements that keep changing, stakeholders who change their minds, or an evolving product direction. In these cases, the code itself may be fine -- the problem is upstream.

## Relationship to Other Analyzers

Churn data feeds into several other Omen analyzers:

- **[Hotspot analysis](./hotspots.md)**: combines churn with complexity to identify files that are both frequently modified and structurally complex.
- **[Defect prediction](./defect-prediction.md)**: uses churn frequency as the primary component of the Process factor (30% of the PMAT model).
- **[Change risk](./change-risk.md)**: uses per-file churn history to contextualize individual commits.

Running `omen churn` independently is useful for understanding raw change patterns. The derived analyzers add interpretive layers on top of this data.

## Research Background

**Nagappan and Ball (2005), "Use of Relative Code Churn Measures to Predict System Defect Density" (International Conference on Software Engineering, IEEE/ACM).** This Microsoft Research study analyzed Windows Server 2003 and found that relative code churn measures -- the ratio of churned lines to total lines, the number of files churned, and churn frequency -- were statistically significant predictors of system defect density. The key finding for practical purposes: files with high relative churn had defect densities 2--8 times higher than files with low churn. Code churn outperformed other commonly used metrics like code coverage and code complexity as a standalone predictor, though the best results came from combining churn with other metrics.

Nagappan and Ball also found that relative churn measures (normalized by file size) were better predictors than absolute churn measures. A 100-line file with 50 commits is a stronger signal than a 10,000-line file with 50 commits. Omen reports both the raw metrics and the percentile rankings, which provide implicit normalization against the rest of the repository.
