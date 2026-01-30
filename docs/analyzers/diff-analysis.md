---
sidebar_position: 8
---

# PR/Branch Diff Analysis

Omen's diff analyzer evaluates the cumulative changes on a branch relative to a target branch or commit. Where the [change risk analyzer](./change-risk.md) scores individual commits, diff analysis looks at the aggregate picture: everything that would land if this branch were merged right now.

This is the analyzer most directly useful in code review and CI/CD pipelines.

## Risk Factors

The diff analyzer computes five factors from the aggregate diff:

| Factor | Description |
|---|---|
| **Lines Added** | Total lines added across all commits on the branch |
| **Lines Deleted** | Total lines deleted across all commits on the branch |
| **Files Modified** | Number of distinct files changed |
| **Commits** | Number of commits on the branch |
| **Entropy** | How scattered the changes are across the codebase |

### Entropy

Entropy measures the distribution of changes across files. It is calculated using Shannon entropy over the proportion of changed lines per file. A branch that modifies one file heavily has low entropy. A branch that touches many files with small changes has high entropy.

High entropy is a risk signal because scattered changes are harder to review, harder to test, and more likely to have unintended interactions. A reviewer can reason about a focused change to a single module. A change that touches 30 files across 8 directories is difficult to hold in your head.

## Risk Score

The five factors are combined into a normalized risk score between 0.0 and 1.0:

| Score Range | Risk Level | Interpretation |
|---|---|---|
| < 0.2 | **LOW** | Small, focused change. Low review burden. |
| 0.2 -- 0.5 | **MEDIUM** | Moderate change. Standard review recommended. |
| > 0.5 | **HIGH** | Large or scattered change. Thorough review recommended. Consider splitting. |

## Usage

```bash
# Compare current branch against main
omen diff --target main

# Compare against a specific commit
omen diff --target abc1234

# Compare against a specific branch
omen diff --target release/v2.1

# Markdown output (useful for PR comments)
omen diff --target main -f markdown

# JSON output for CI scripting
omen -f json diff --target main
```

### Example Output

```
Branch Diff Analysis
====================
Current branch: feature/auth-overhaul
Target: main
Commits ahead: 12

Factor          Value    Normalized
Lines Added     1,247    0.72
Lines Deleted     389    0.45
Files Modified     18    0.61
Commits            12    0.38
Entropy          3.42    0.68

Risk Score: 0.57 (HIGH)

Recommendation: This branch has high entropy (changes spread across
18 files) and a large volume of additions. Consider splitting into
smaller, focused PRs if the changes are logically separable.
```

### Markdown Output

When using `-f markdown`, Omen produces output suitable for posting as a PR comment:

```markdown
## Branch Diff Analysis

| Factor | Value | Normalized |
|---|---|---|
| Lines Added | 1,247 | 0.72 |
| Lines Deleted | 389 | 0.45 |
| Files Modified | 18 | 0.61 |
| Commits | 12 | 0.38 |
| Entropy | 3.42 | 0.68 |

**Risk: HIGH (0.57)**
```

## Interpreting Change Patterns

The ratio between lines added and lines deleted reveals the nature of the work:

| Pattern | Added vs. Deleted | Likely Activity |
|---|---|---|
| High add, low delete | LA >> LD | New feature or new module |
| Balanced add/delete | LA ~= LD | Refactoring or rewriting |
| Net reduction | LD > LA | Cleanup, dead code removal, simplification |
| High add, high delete, high entropy | LA high, LD high, entropy high | Scattered refactoring or cross-cutting concern change |

None of these patterns are inherently good or bad. A high-add new feature is expected to have a high score. What matters is whether the risk level matches your expectations for the type of work being done. A "small bug fix" PR that comes back with a HIGH risk score and 18 modified files warrants investigation.

## CI/CD Integration

### GitHub Actions

```yaml
name: Omen Diff Analysis
on: [pull_request]

jobs:
  diff-analysis:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0  # Full history required for accurate diff

      - name: Install Omen
        run: cargo install omen-cli

      - name: Run diff analysis
        run: |
          RISK=$(omen -f json diff --target origin/main | jq -r '.risk_level')
          echo "Risk level: $RISK"

          if [ "$RISK" = "HIGH" ]; then
            echo "::warning::High risk diff detected. Review carefully."
          fi

      - name: Post PR comment
        if: always()
        run: |
          REPORT=$(omen diff --target origin/main -f markdown)
          gh pr comment ${{ github.event.pull_request.number }} --body "$REPORT"
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

### GitLab CI

```yaml
diff-analysis:
  stage: review
  script:
    - cargo install omen-cli
    - omen -f json diff --target origin/$CI_MERGE_REQUEST_TARGET_BRANCH_NAME
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
```

### Quality Gate

To fail a pipeline when the diff risk exceeds a threshold:

```bash
SCORE=$(omen -f json diff --target origin/main | jq '.risk_score')
if [ "$(echo "$SCORE > 0.5" | bc -l)" -eq 1 ]; then
  echo "Diff risk score $SCORE exceeds threshold (0.5)"
  exit 1
fi
```

## Configuration

In `omen.toml`:

```toml
[diff]
# Default target branch when --target is not specified
default_target = "main"
```

## Relationship to Other Analyzers

Diff analysis is a coarse-grained risk assessment of a branch as a whole. For finer-grained analysis, combine it with other analyzers:

- **`omen changes`**: scores individual commits within the branch, so you can find which specific commit is driving the risk.
- **`omen hotspot`**: identifies whether the branch touches known hotspot files.
- **`omen complexity`**: shows whether the branch is increasing or decreasing complexity in the modified files.
- **`omen clones`**: checks whether the branch introduces duplicated code.

Running `omen all` on a branch before merging gives the most complete picture, but `omen diff` is the fastest single check for CI integration.
