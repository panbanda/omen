---
sidebar_position: 9
---

# Hotspot Analysis

A hotspot is a file that is both complex and frequently modified. Either property alone is manageable: complex code that never changes is stable; simple code that changes often is easy to work with. The combination is where defects concentrate.

Omen's hotspot analyzer identifies these files by combining code complexity (from tree-sitter analysis) with churn data (from Git history).

## Calculation

The hotspot score is the geometric mean of the file's normalized churn percentile and complexity percentile:

```
hotspot = sqrt(churn_percentile * complexity_percentile)
```

The geometric mean is used instead of the arithmetic mean because it requires both factors to be elevated for the score to be high. A file at the 95th percentile for churn but the 10th percentile for complexity produces a hotspot score of `sqrt(0.95 * 0.10) = 0.31` (Moderate), not `(0.95 + 0.10) / 2 = 0.53` (which would be High under an arithmetic mean). This prevents false positives from files that are extreme in only one dimension.

### Normalization

Both churn and complexity are normalized against industry benchmarks using empirical cumulative distribution functions (CDFs). This means:

- A file's churn percentile reflects where it falls relative to expected churn rates across a broad range of projects, not just the current repository.
- A file's complexity percentile reflects where its cyclomatic complexity sits relative to empirical complexity distributions.

This cross-project normalization prevents the problem where a uniformly complex codebase appears to have no hotspots because every file is equally complex. Even if every file in a repository has high complexity, the files that also have high churn are the ones that need attention.

## Severity Levels

| Severity | Score | Interpretation |
|---|---|---|
| **Critical** | >= 0.60 | Both high complexity and high churn. Top priority for refactoring. |
| **High** | >= 0.40 | Significant risk. Should be addressed in upcoming work. |
| **Moderate** | >= 0.25 | Worth monitoring. May become a problem as the codebase evolves. |
| **Low** | < 0.25 | Not a concern. Either stable, simple, or both. |

## Usage

```bash
# Run hotspot analysis on the current directory
omen hotspot

# JSON output
omen -f json hotspot

# Analyze a specific path
omen -p ./src hotspot

# Analyze a remote repository
omen -p torvalds/linux hotspot
```

### Example Output

```
Hotspot Analysis
================

CRITICAL (0.82)
  src/engine/query_planner.rs
  Complexity: P94  Churn: P71  Contributors: 9

CRITICAL (0.74)
  src/parser/expression.rs
  Complexity: P89  Churn: P62  Contributors: 6

HIGH (0.51)
  src/api/handlers.rs
  Complexity: P72  Churn: P36  Contributors: 4

MODERATE (0.33)
  src/config/loader.rs
  Complexity: P55  Churn: P20  Contributors: 3

Files analyzed: 214
Critical: 4 (1.9%)
High: 11 (5.1%)
Moderate: 28 (13.1%)
Low: 171 (79.9%)
```

## What to Do with Hotspots

Identifying hotspots is the diagnostic step. The treatment depends on why the file is a hotspot.

### Complex and Frequently Extended

The file does too much, and every new feature touches it. This is the most common pattern. The fix is usually to decompose the file:

- Extract cohesive groups of functions into separate modules.
- Introduce interfaces or traits to decouple consumers from the implementation.
- Apply the single responsibility principle to split the file along logical boundaries.

### Complex and Frequently Fixed

The file has bugs because it's hard to understand, and fixes introduce new bugs. This is the most dangerous pattern and the strongest signal that refactoring should be prioritized:

- Add comprehensive tests before changing anything.
- Simplify control flow by reducing nesting depth.
- Replace conditional logic with polymorphism or lookup tables where appropriate.
- Consider a rewrite if the module's behavior is well-specified but the implementation has accumulated too many patches.

### Many Contributors

High contributor count amplifies both patterns above. When many developers modify the same complex file, implicit assumptions diverge and the probability of conflicting changes increases. Consider establishing clear ownership or splitting the file so that different teams own different parts.

## Configuration

In `omen.toml`:

```toml
[hotspot]
# Time window for churn data
since = "6m"

# Number of top hotspots to display
top = 20

# Minimum severity to report
min_severity = "moderate"
```

## Relationship to Other Analyzers

Hotspot analysis combines two of Omen's other analyzers:

- **`omen churn`** provides the churn data (commit frequency, lines changed, contributors).
- **`omen complexity`** provides the complexity data (cyclomatic and cognitive complexity).

Running `omen hotspot` is equivalent to running both and computing their intersection. If you want the raw data for either dimension independently, use the individual commands.

Hotspot data also feeds into the [defect prediction](./defect-prediction.md) model, where it contributes to the Process and Metrics factors.

## Research Background

**Adam Tornhill, "Your Code as a Crime Scene" (2015).** Tornhill popularized the idea of applying geographic profiling techniques (used in criminal investigations) to codebases. The core insight is that code analysis should be behavioral, not just structural. A file's history of changes is as important as its current state. Tornhill's work demonstrated that plotting complexity against churn on a scatter plot reliably identifies the files where development effort is disproportionately spent and where bugs are most likely to be found.

**Graves et al. (2000), "Predicting Fault Incidence Using Software Change History" (IEEE Transactions on Software Engineering).** This study showed that the number of changes to a module is a better predictor of faults than the module's size. Graves et al. analyzed a large Nortel Networks system and found that change history -- particularly recent change activity -- was the strongest predictor available. The finding supports the churn dimension of hotspot analysis: files that change frequently are files where defects appear.

**Nagappan, Murphy, and Basili (2005), "The Influence of Organizational Structure on Software Quality" (Microsoft Research Technical Report).** While primarily about organizational effects on software quality, this study established that code churn metrics (lines added, deleted, and modified over time) are among the best available predictors of post-release defects. The combination of code churn with static analysis metrics -- which is exactly what hotspot analysis does -- provides stronger predictive power than either metric family alone.

## Practical Guidance

Hotspot analysis is most useful when run periodically and tracked over time. A single run gives you a snapshot, but the trend matters more:

- **Rising hotspot scores** indicate a file is becoming harder to maintain. Intervene before it becomes a chronic problem.
- **Falling hotspot scores** after a refactoring effort confirm that the intervention worked.
- **Persistent critical hotspots** that resist improvement may indicate an architectural problem that can't be solved at the file level. Consider whether the module's responsibilities need to be redistributed across the system.
