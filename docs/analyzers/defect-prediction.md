---
sidebar_position: 6
---

# Defect Prediction

Omen's defect prediction analyzer estimates the probability that a file contains latent defects, assigning each file a risk score from 0 to 100%. The model combines process metrics from Git history with static code metrics to identify files most likely to produce bugs in the near future.

## The PMAT Model

Omen uses a weighted composite model with four factor groups. The name is a mnemonic for the inputs:

| Factor | Weight | What It Measures |
|---|---|---|
| **P**rocess | 30% | Churn frequency, ownership diffusion, developer count |
| **M**etrics | 25% | Cyclomatic complexity, cognitive complexity |
| **A**ge | 20% | Code age, time since last modification, stability |
| **T**otal size | 25% | Lines of code (LOC) |

Each factor group produces a normalized sub-score. The final risk score is the weighted sum, scaled to a 0--100 range.

### Process (30%)

Process metrics capture how a file has been changed over time, not what it currently contains. Two files with identical complexity can have very different defect rates if one is modified by a single author on a steady cadence and the other is touched by a dozen developers in bursts.

- **Churn frequency**: how often the file has been modified in the analysis window.
- **Ownership diffusion**: the number of distinct contributors, weighted by recency. A file with many recent contributors has higher ownership diffusion.

Process metrics are the heaviest-weighted factor because empirical research consistently shows that how code changes is a stronger defect predictor than what the code looks like at any point in time.

### Metrics (25%)

Static analysis metrics from tree-sitter parsing:

- **Cyclomatic complexity**: the number of linearly independent paths through a function. Higher values mean more branches and more opportunities for defects in edge cases.
- **Cognitive complexity**: a measure of how difficult code is for a human to understand, penalizing deeply nested structures and non-linear control flow more heavily than cyclomatic complexity does.

These are aggregated at the file level (sum or max across functions, depending on configuration).

### Age (20%)

Code age is a double-edged signal. Very old code that hasn't been touched is often stable and well-tested. Recently written or recently modified code is more likely to contain defects because it hasn't been exercised in production as long.

- **Code age**: time since the file was first added to the repository.
- **Last modification**: time since the most recent commit touching the file.
- **Stability**: the ratio of the file's age to its churn count. High age with low churn indicates stability.

### Total Size (25%)

Lines of code is the simplest and, in some studies, the single most effective defect predictor. Larger files contain more code, and more code means more places for bugs to hide. This factor uses logical lines of code (excluding blanks and comments).

## Risk Levels

The continuous 0--100 score is bucketed into three levels for reporting:

| Level | Score Range | Interpretation |
|---|---|---|
| **Low** | 0--33% | File has low defect probability based on current signals. |
| **Medium** | 34--66% | Elevated risk. Worth reviewing if the file is being modified. |
| **High** | 67--100% | Strong signal of defect-prone code. Prioritize review, testing, or refactoring. |

## Usage

```bash
# Run defect prediction on the current directory
omen defect

# JSON output for scripting
omen -f json defect

# Analyze a specific path
omen -p ./src defect

# Analyze a remote repository
omen -p facebook/react defect
```

### Example Output

```
Defect Prediction Analysis
==========================

Risk: HIGH (78%)
  src/parser/transform.rs
  Process: 0.85  Metrics: 0.72  Age: 0.61  Size: 0.88

Risk: HIGH (71%)
  src/engine/resolver.rs
  Process: 0.79  Metrics: 0.68  Age: 0.55  Size: 0.82

Risk: MEDIUM (52%)
  src/cli/commands.rs
  Process: 0.45  Metrics: 0.61  Age: 0.40  Size: 0.65

Risk: LOW (18%)
  src/config/defaults.rs
  Process: 0.12  Metrics: 0.22  Age: 0.15  Size: 0.20

Files analyzed: 142
High risk: 8 (5.6%)
Medium risk: 31 (21.8%)
Low risk: 103 (72.5%)
```

## Configuration

In `omen.toml`:

```toml
[defect]
# Adjust factor weights (must sum to 1.0)
process_weight = 0.30
metrics_weight = 0.25
age_weight = 0.20
size_weight = 0.25

# Time window for process metrics
since = "6m"
```

## Interpreting Results

A high defect prediction score does not mean a file has bugs right now. It means the file exhibits patterns that are statistically associated with higher defect rates in empirical studies. Use the results to:

- **Prioritize code review**: focus reviewer attention on high-risk files in a pull request.
- **Guide testing**: write more thorough tests for files flagged as high risk.
- **Inform refactoring**: if a file consistently scores high, consider breaking it into smaller, more focused modules.
- **Track trends**: run `omen defect` periodically and watch for files whose scores are rising. Increasing risk over time is a stronger signal than a single snapshot.

## Limitations

- **Git history required**: the Process and Age factors depend on Git history. Repositories with shallow clones or very short histories will produce less reliable scores.
- **Language-agnostic LOC**: the size factor counts logical lines without weighting for language verbosity. A 500-line Java file and a 500-line Python file receive the same size score, even though the Java file likely does less.
- **No runtime data**: the model is purely static and historical. It does not incorporate test results, production error rates, or coverage information.

## Research Background

Omen's defect prediction approach draws on two key findings from the empirical software engineering literature:

**Menzies et al. (2007), "Data Mining Static Code Attributes to Learn Defect Predictors" (IEEE Transactions on Software Engineering).** This study demonstrated that relatively simple statistical models using static code metrics (LOC, complexity measures) can effectively predict defect-prone modules. The authors showed that a Naive Bayes classifier using a small set of static attributes achieved recall rates of 71% or higher across multiple NASA datasets, and that more complex learners did not consistently outperform simpler ones.

**Rahman et al. (2014), "Comparing Static Bug Finders and Statistical Prediction" (ACM/IEEE International Conference on Software Engineering).** Rahman et al. compared static analysis tools (like FindBugs) with statistical defect prediction models and found that even simple prediction models based on code metrics outperform random file selection by a wide margin. Their key finding was that statistical models and static analysis tools find largely non-overlapping sets of defects, suggesting they are complementary. For prioritization purposes -- deciding which files to review or test -- statistical prediction is highly effective even without running the code.

The practical takeaway from this research is that you do not need a sophisticated machine learning model to get value from defect prediction. A weighted combination of well-chosen features, applied consistently, is enough to meaningfully reduce the search space when looking for problematic code.
