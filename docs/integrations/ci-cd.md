---
sidebar_position: 2
---

# CI/CD Integration

Omen is designed to run in CI pipelines as a quality gate, risk assessment tool, and health tracker. All commands support JSON output (`-f json`) for programmatic parsing, and `omen score` returns non-zero exit codes when the score falls below a configured threshold.

## GitHub Actions

### Quality Gate with Repository Score

The simplest integration: fail the build if the repository score drops below a threshold.

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
        run: brew install panbanda/omen/omen

      - name: Check repository score
        run: omen score
```

`omen score` exits with code 1 if the score is below the configured minimum (default: 60). No additional scripting is needed for a basic pass/fail gate.

For a custom threshold:

```yaml
      - name: Check repository score
        run: |
          SCORE=$(omen -f json score | jq '.score')
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
        run: brew install panbanda/omen/omen

      - name: Analyze PR changes
        run: |
          omen diff --target ${{ github.base_ref }}

      - name: Check change risk
        run: |
          RESULT=$(omen -f json diff --target ${{ github.base_ref }})
          RISK=$(echo "$RESULT" | jq '.risk_score')
          echo "Change risk score: $RISK"

          if [ "$(echo "$RISK > 0.8" | bc)" -eq 1 ]; then
            echo "::warning::High-risk changes detected (risk score: $RISK). Extra review recommended."
          fi
```

### Complete Workflow

A full workflow that runs quality gates, risk assessment, and posts a summary comment on the PR:

```yaml
name: Omen Analysis
on:
  pull_request:
    types: [opened, synchronize]

jobs:
  analyze:
    runs-on: ubuntu-latest
    permissions:
      pull-requests: write

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install Omen
        run: brew install panbanda/omen/omen

      - name: Run analysis
        id: analysis
        run: |
          # Repository score
          SCORE=$(omen -f json score | jq '.score')
          echo "score=$SCORE" >> "$GITHUB_OUTPUT"

          # Diff analysis
          DIFF=$(omen -f json diff --target ${{ github.base_ref }})
          RISK=$(echo "$DIFF" | jq '.risk_score')
          CHANGED=$(echo "$DIFF" | jq '.files_changed')
          echo "risk=$RISK" >> "$GITHUB_OUTPUT"
          echo "changed=$CHANGED" >> "$GITHUB_OUTPUT"

          # SATD check
          SATD_SECURITY=$(omen -f json satd | jq '[.items[] | select(.category == "security")] | length')
          echo "security_debt=$SATD_SECURITY" >> "$GITHUB_OUTPUT"

          # Stale flags
          STALE_FLAGS=$(omen -f json flags | jq '[.flags[] | select(.stale)] | length')
          echo "stale_flags=$STALE_FLAGS" >> "$GITHUB_OUTPUT"

      - name: Post summary comment
        uses: actions/github-script@v7
        with:
          script: |
            const score = '${{ steps.analysis.outputs.score }}';
            const risk = '${{ steps.analysis.outputs.risk }}';
            const changed = '${{ steps.analysis.outputs.changed }}';
            const securityDebt = '${{ steps.analysis.outputs.security_debt }}';
            const staleFlags = '${{ steps.analysis.outputs.stale_flags }}';

            const body = `## Omen Analysis

            | Metric | Value |
            |--------|-------|
            | Repository Score | ${score}/100 |
            | Change Risk | ${risk} |
            | Files Changed | ${changed} |
            | Security Debt Items | ${securityDebt} |
            | Stale Feature Flags | ${staleFlags} |
            `;

            github.rest.issues.createComment({
              issue_number: context.issue.number,
              owner: context.repo.owner,
              repo: context.repo.repo,
              body: body
            });

      - name: Fail on quality gate
        run: |
          SCORE=${{ steps.analysis.outputs.score }}
          if [ "$(echo "$SCORE < 60" | bc)" -eq 1 ]; then
            echo "::error::Repository score $SCORE is below minimum threshold (60)"
            exit 1
          fi
```

## Docker

For CI environments where installing Rust tooling is impractical, use the Docker image:

```bash
docker run --rm -v "$(pwd):/repo" ghcr.io/panbanda/omen:latest score /repo
```

In a GitHub Actions workflow:

```yaml
      - name: Run Omen via Docker
        run: |
          docker run --rm \
            -v "${{ github.workspace }}:/repo" \
            ghcr.io/panbanda/omen:latest \
            -f json score /repo
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

    omen-satd-security:
      run: |
        SECURITY=$(omen -f json satd | jq '[.items[] | select(.category == "security")] | length')
        if [ "$SECURITY" -gt 0 ]; then
          echo "Found $SECURITY unresolved security debt items"
          exit 1
        fi
      fail_text: "Unresolved security debt detected. Run 'omen satd' for details."
```

## JSON Output

All Omen commands support `-f json` for machine-readable output. This is the recommended format for CI integration because it provides structured data that can be parsed with `jq` or any JSON library.

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
minimum_score = 60
```

When `omen score` runs, it compares the computed score against this threshold. If the score is below the minimum, the command exits with code 1. If the threshold is not set, the command always exits with code 0 (no gate).

The threshold can also be overridden on the command line:

```bash
omen score --minimum 75
```

## Tips for CI Integration

**Use `fetch-depth: 0`.** Many analyzers (churn, ownership, hotspot, temporal coupling, defect prediction) require Git history. Shallow clones will produce incomplete or missing results for these analyzers. Always use `fetch-depth: 0` in your checkout step.

**Cache the Omen binary.** If you install via `cargo install`, cache the Cargo binary directory to avoid recompilation on every run:

```yaml
      - uses: actions/cache@v4
        with:
          path: ~/.cargo/bin
          key: omen-${{ runner.os }}
```

**Run analyzers selectively.** `omen all` runs every analyzer, which may be slow on large codebases. In CI, consider running only the analyzers that matter for your quality gate:

```bash
omen score                    # Composite score (runs necessary analyzers internally)
omen diff --target main       # PR-specific risk
omen satd                     # Debt check
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
