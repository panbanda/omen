---
sidebar_position: 2
---

# CI/CD Integration

Omen is designed to run in CI pipelines as a quality gate, risk assessment tool, and health tracker. All commands support JSON output (`-f json`) for programmatic parsing, and `omen score` returns a non-zero exit code when the score falls below a configured `fail_under` threshold.

## GitHub Action

Omen provides a composite GitHub Action for automated PR analysis. It runs diff risk analysis (on pull request events) and health scoring on every run, and can write results to the job summary, post a sticky PR comment, apply a risk label, and fail the build on high risk.

### Basic Usage

```yaml
name: Omen Analysis
on: [pull_request]

permissions:
  contents: read

jobs:
  omen:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - uses: panbanda/omen@omen-v4.26.1
        id: omen

      - name: Print results
        run: |
          echo "Risk: ${{ steps.omen.outputs.risk-level }} (${{ steps.omen.outputs.risk-score }})"
          echo "Health: ${{ steps.omen.outputs.health-grade }} (${{ steps.omen.outputs.health-score }})"
```

`fetch-depth: 0` is required. Omen needs full git history for accurate analysis (churn, ownership, hotspot, and temporal coupling all depend on it).

### Complete Workflow

```yaml
name: Omen Analysis

on:
  pull_request:

permissions:
  contents: read
  pull-requests: write
  issues: write

jobs:
  omen:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - uses: panbanda/omen@omen-v4.26.1
        id: omen
        with:
          version: latest
          path: .
          comment: true
          label: true
          label-template: 'risk: {{level}}'
          label-color-low: '0e8a16'
          label-color-medium: 'fbca04'
          label-color-high: 'd93f0b'
          check: true
          check-threshold: high
          summary: true
          github-token: ${{ secrets.GITHUB_TOKEN }}

      - name: Print results
        run: |
          echo "Risk: ${{ steps.omen.outputs.risk-level }} (${{ steps.omen.outputs.risk-score }})"
          echo "Health: ${{ steps.omen.outputs.health-grade }} (${{ steps.omen.outputs.health-score }})"
```

`pull-requests: write` is required when `comment` is enabled, and `issues: write` is required when `label` is enabled. Workflows that disable both features can drop to `contents: read` only.

Pin the action to a release tag (the latest at time of writing is `omen-v4.26.1`; check the [releases page](https://github.com/panbanda/omen/releases/latest) for the current one) and bump it as new releases ship. The `version: latest` input controls which omen **binary** the action downloads and is independent of the action tag itself.

### Action Inputs

| Input | Default | Description |
|-------|---------|-------------|
| `version` | `latest` | Omen version to install (for example `"4.20.3"`), or `latest` for the newest stable release |
| `path` | `.` | Repository path to analyze, relative to the workflow workspace |
| `comment` | `false` | Post or update a sticky analysis comment on pull requests |
| `label` | `false` | Create and apply a risk-level label to pull requests |
| `label-template` | `risk: &#123;&#123;level&#125;&#125;` | Label name template. Use `{{level}}` for risk level replacement |
| `label-color-low` | `0e8a16` | Hex color (without `#`) for the low-risk label |
| `label-color-medium` | `fbca04` | Hex color (without `#`) for the medium-risk label |
| `label-color-high` | `d93f0b` | Hex color (without `#`) for the high-risk label |
| `check` | `false` | Fail pull request analysis when the risk level meets or exceeds the configured threshold |
| `check-threshold` | `high` | Risk level threshold for check failure (`low`, `medium`, `high`) |
| `summary` | `true` | Write risk, change-size, repository health, and score-component results to the job step summary |
| `github-token` | `${{ github.token }}` | GitHub token used to resolve and download releases and, when enabled, manage pull request comments and labels |

### Action Outputs

| Output | Example | Description |
|--------|---------|-------------|
| `risk-score` | `0.42` | Pull request diff risk score (0.0 - 1.0); empty on non-pull-request events |
| `risk-level` | `medium` | Pull request risk level (`low`, `medium`, `high`); empty on non-pull-request events |
| `health-score` | `76.9` | Repository health score (0 - 100) |
| `health-grade` | `C` | Health grade (A, B, C, D, F) |
| `diff-json` | `{...}` | Full `omen diff` JSON output; empty on non-pull-request events |
| `score-json` | `{...}` | Full `omen score` JSON output |

If a JSON output would exceed the GitHub Actions output-size guard (900KB), the action writes it to a temporary file and returns the file path instead of inline JSON. Consumers should accept either form.

### Job Summary

When `summary: true` (the default), the action writes a job-summary table with health, PR risk, and change-size metrics, followed by a component-score breakdown table. Diff risk and change-size metrics are only available on pull request events; on other events the summary notes that they were skipped.

### Sticky PR Comment

When `comment: true` on a pull request event, the action posts (or updates, on subsequent pushes) a single comment marked with an `<!-- omen-analysis -->` marker. The comment leads with a **Needs attention** section that surfaces the lowest-scoring components below their attention thresholds, each with a one-line explanation and the CLI command to investigate it locally. Below that, collapsible sections cover the full component-score table, PR risk factors, recommendations, and a short "investigate locally" cheat sheet. The comment footer reports the omen version that generated it.

### Risk Label

When `label: true` on a pull request event, the action creates (if needed) and applies a label named from `label-template` with `{{level}}` replaced by the risk level, using the corresponding `label-color-*` input. Any stale risk label from a previous run (a different level) is removed first.

### Quality Gate via Action

```yaml
      - uses: panbanda/omen@omen-v4.26.1
        with:
          check: true
          check-threshold: high  # fail on high risk PRs
```

The check only runs for pull request events; on other events it is skipped with a warning, since diff risk analysis requires a PR base to compare against.

## GitHub Actions (Manual)

For more control, you can install Omen manually and run commands directly instead of using the composite action.

### Quality Gate with Repository Score

The simplest integration: fail the build if the repository score drops below a threshold configured in `omen.toml`.

```yaml
name: Code Quality
on: [push, pull_request]

jobs:
  quality-gate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0  # Full history needed for churn, ownership, hotspot analyzers

      - name: Install Omen
        run: brew install panbanda/brews/omen

      - name: Check repository score
        run: omen score
```

`omen score` exits non-zero if the score is below `[score] fail_under` in `omen.toml`. If `fail_under` is not set, the command always exits 0.

For a custom threshold without editing `omen.toml`:

```yaml
      - name: Check repository score
        run: |
          SCORE=$(omen -f json score | jq '.overall_score')
          echo "Repository score: $SCORE"
          if [ "$(echo "$SCORE < 70" | bc)" -eq 1 ]; then
            echo "::error::Repository score $SCORE is below threshold (70)"
            exit 1
          fi
```

### PR Risk Assessment with Diff Analysis

Analyze the structural impact of changes in a pull request:

```yaml
name: PR Risk Assessment
on:
  pull_request:
    types: [opened, synchronize]

jobs:
  risk-assessment:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install Omen
        run: brew install panbanda/brews/omen

      - name: Analyze PR changes
        run: |
          omen diff --target origin/${{ github.base_ref }}

      - name: Check change risk
        run: |
          RESULT=$(omen -f json diff --target origin/${{ github.base_ref }})
          RISK=$(echo "$RESULT" | jq '.score')
          echo "Change risk score: $RISK"

          if [ "$(echo "$RISK > 0.8" | bc)" -eq 1 ]; then
            echo "::warning::High-risk changes detected (risk score: $RISK). Extra review recommended."
          fi
```

## Docker

For CI environments where installing Rust tooling is impractical, use the Docker image:

```bash
docker run --rm -v "$(pwd):/repo" ghcr.io/panbanda/omen:latest -p /repo score
```

In a GitHub Actions workflow:

```yaml
      - name: Run Omen via Docker
        run: |
          docker run --rm \
            -v "${{ github.workspace }}:/repo" \
            ghcr.io/panbanda/omen:latest \
            -f json -p /repo score
```

The Docker image includes all tree-sitter grammars and requires no additional dependencies.

## Pre-push Hooks with Lefthook

[Lefthook](https://github.com/evilmartians/lefthook) provides fast, cross-platform Git hooks. Add a quality gate that runs before every push:

```yaml
# lefthook.yml
pre-push:
  commands:
    omen-score:
      run: omen score
      fail_text: "Repository score is below the minimum threshold. Run 'omen score' for details."
```

## JSON Output

All Omen commands support `-f json` for machine-readable output. This is the recommended format for CI integration because it provides structured data that can be parsed with `jq` or any JSON library. Add the global `--compact` flag to minify the output.

```bash
# Repository score with component breakdown
omen -f json score

# Complexity for all files
omen -f json complexity

# All analyzers
omen -f json all
```

JSON output goes to stdout. Human-readable messages (if any) go to stderr. This means piping and redirection work as expected:

```bash
# Save results to a file
omen -f json score > omen-results.json

# Pipe to jq for filtering
omen -f json complexity | jq '[.functions[] | select(.cyclomatic > 15)]'
```

## Score Thresholds

Configure the pass/fail threshold in `omen.toml`:

```toml
[score]
fail_under = 60.0
```

When `omen score` runs, it compares the computed score against this threshold. If the score is below the threshold, the command exits non-zero. If `fail_under` is not set, the command always exits 0 (no gate).

## Tips for CI Integration

**Use `fetch-depth: 0`.** Many analyzers (churn, ownership, hotspot, temporal coupling, defect prediction) require Git history. Shallow clones will produce incomplete or missing results for these analyzers. Always use `fetch-depth: 0` in your checkout step.

**Run analyzers selectively.** `omen all` runs every analyzer, which may be slow on large codebases. In CI, consider running only the analyzers that matter for your quality gate:

```bash
omen score                    # Composite score (runs necessary analyzers internally)
omen diff --target main       # PR-specific risk
omen satd                     # Debt check
omen stubs --gate error       # Fail the build if any unfinished stub is found
```

**Store results as artifacts.** Save JSON output for trend tracking:

```yaml
      - name: Save analysis results
        run: omen -f json all > omen-analysis.json

      - uses: actions/upload-artifact@v4
        with:
          name: omen-analysis
          path: omen-analysis.json
```
