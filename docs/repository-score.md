---
sidebar_position: 14
---

# Repository Score

The repository score is a composite health metric ranging from 0 to 100. It aggregates results from multiple analyzers into a single number that represents the overall structural quality of a codebase.

## Usage

```bash
omen score
```

JSON output for scripting:

```bash
omen -f json score
```

## Score Components

The score is a weighted combination of seven analyzer dimensions:

| Component | Weight | What It Measures |
|---|---|---|
| Complexity | 25% | Cyclomatic and cognitive complexity of functions and methods |
| Duplication | 20% | Percentage of code that exists as clones (Type 1, 2, and 3) |
| SATD | 10% | Self-admitted technical debt: TODO, FIXME, HACK, and similar markers |
| TDG (Tech Debt Gradient) | 15% | Rate of technical debt accumulation over recent history |
| Coupling | 10% | Afferent and efferent coupling between modules |
| Smells | 5% | Structural code smells (long methods, large classes, deep nesting, etc.) |
| Cohesion | 15% | How focused modules are on a single responsibility |

Each component produces a sub-score from 0 to 100, which is then multiplied by its weight and summed to produce the final score.

## Normalization Philosophy

Raw analyzer outputs vary widely in scale and distribution. A codebase with average cyclomatic complexity of 8 is qualitatively different from one averaging 25, but the relationship isn't linear -- the jump from 8 to 15 matters more than the jump from 40 to 47.

Omen's normalization follows several principles:

### Calibrated Against Industry Tools

Score thresholds are calibrated against benchmarks from SonarQube, CodeClimate, and CISQ standards. A score of 80+ in Omen should correspond roughly to an "A" rating in comparable tools. This doesn't mean the scores are identical -- the methodologies differ -- but the relative quality tiers are aligned.

### Non-linear Penalty Curves

Most components use non-linear penalty curves rather than straight linear scaling. This means:

- Low levels of complexity/duplication/debt are penalized lightly. Some complexity is normal.
- Moderate levels are penalized proportionally.
- High levels are penalized aggressively. A codebase where 40% of the code is duplicated is not twice as bad as 20% -- it's significantly worse.

This produces scores that feel intuitively right: small issues don't tank the score, but serious problems are clearly reflected.

### Severity-aware SATD Weighting

Not all technical debt markers carry equal weight. A `// SECURITY: potential SQL injection` comment indicates a fundamentally different risk than `// TODO: add logging`.

SATD detection uses severity multipliers:

| Marker Type | Severity Multiplier | Rationale |
|---|---|---|
| Security annotations | 4x | Unresolved security issues are high-impact |
| FIXME, BUG | 2x | Known defects that haven't been addressed |
| HACK, REFACTOR | 1x | Acknowledged shortcuts and structural issues |
| TODO, NOTE | 0.25x | Low-priority reminders and documentation |

This means a codebase with 10 security-related SATD markers scores worse than one with 40 TODOs, which matches the actual risk profile.

## Configuration

Score weights and thresholds are configurable in `omen.toml`:

```toml
[score]
# Override component weights (must sum to 1.0)
complexity_weight = 0.25
duplication_weight = 0.20
satd_weight = 0.10
tdg_weight = 0.15
coupling_weight = 0.10
smells_weight = 0.05
cohesion_weight = 0.15

# Threshold for pass/fail in CI
minimum_score = 60
```

If weights are overridden, they must sum to 1.0. Omen will error on startup if they don't.

## Score Trend Tracking

Track how the score changes over time:

```bash
omen score trend --period monthly --since 6m
```

This walks the Git history, checks out historical commits (or reads cached snapshots), computes the score at each point, and displays the trend. Useful for:

- Seeing whether code quality is improving or degrading
- Correlating score changes with specific releases or refactoring efforts
- Providing evidence for technical debt reduction initiatives

Available periods: `daily`, `weekly`, `monthly`. The `--since` flag accepts duration strings like `3m` (3 months), `1y` (1 year), `90d` (90 days).

## CI/CD Integration

### Exit Codes

`omen score` exits with code 0 if the score meets the configured minimum threshold, and code 1 if it doesn't. This makes it directly usable as a quality gate:

```bash
omen score || exit 1
```

### Lefthook Integration

Omen integrates with [Lefthook](https://github.com/evilmartians/lefthook) for Git hook-based quality gates. Add to your `lefthook.yml`:

```yaml
pre-push:
  commands:
    omen-score:
      run: omen score
      fail_text: "Repository score is below the minimum threshold."
```

This prevents pushes when the score drops below the configured minimum, catching quality regressions before they reach the remote.

### JSON Output for Custom Gates

For more complex CI logic, parse the JSON output:

```bash
RESULT=$(omen -f json score)
SCORE=$(echo "$RESULT" | jq '.score')
COMPLEXITY=$(echo "$RESULT" | jq '.components.complexity')

echo "Overall: $SCORE, Complexity: $COMPLEXITY"

if [ "$(echo "$SCORE < 70" | bc)" -eq 1 ]; then
  echo "Score $SCORE is below 70, failing build."
  exit 1
fi
```

## Interpreting the Score

| Range | Interpretation |
|---|---|
| 80-100 | Good structural health. Maintainable, low risk. |
| 60-79 | Moderate. Some areas need attention but overall manageable. |
| 40-59 | Concerning. Accumulated debt or complexity is creating risk. |
| 0-39 | Critical. Significant structural problems that likely affect velocity and reliability. |

These ranges are guidelines, not absolutes. A score of 55 in a 15-year-old codebase with active maintenance may be perfectly acceptable. A score of 55 in a 6-month-old greenfield project suggests problems.
