# Repository Score Design

## Overview

Add `omen score` command that outputs a composite repository health score (0-100) with category breakdown. Supports CI enforcement via threshold flags and JSON output for trend tracking.

## Command Interface

```
omen score [paths...] [flags]
```

### Output (default text)

```
Repository Score: 74/100 (C)

  Complexity:    82/100
  Duplication:   91/100
  Defect Risk:   65/100
  Technical Debt: 70/100
  Coupling:      78/100
  Smells:        85/100

  Cohesion:      68/100  (not in composite)

Files analyzed: 247
```

### Flags

| Flag | Description |
|------|-------------|
| `--min-score N` | Exit 1 if composite score < N |
| `--min-complexity N` | Exit 1 if complexity score < N |
| `--min-duplication N` | Exit 1 if duplication score < N |
| `--min-defect N` | Exit 1 if defect risk score < N |
| `--min-debt N` | Exit 1 if technical debt score < N |
| `--min-coupling N` | Exit 1 if coupling score < N |
| `--min-smells N` | Exit 1 if smells score < N |
| `--min-cohesion N` | Exit 1 if cohesion score < N |
| `--enable-cohesion` | Include cohesion in composite score |
| `--format` | Output format: text, json, markdown, toon |
| `--json` | Shorthand for `--format json` |

### Exit Codes

- `0` - Pass (all thresholds met)
- `1` - Threshold violation
- `2` - Error

## Score Calculation

### Component Scores

Each component normalized to 0-100, higher is better.

| Component | Source Analyzer | Normalization |
|-----------|-----------------|---------------|
| Complexity | `complexity` | 100 - (% functions exceeding threshold) |
| Duplication | `duplicates` | 100 - (duplication ratio * 100) |
| Defect Risk | `defect` | 100 - (avg probability * 100) |
| Technical Debt | `satd` | 100 - (debt density per 1K LOC, capped) |
| Coupling | `graph` | 100 - (normalized coupling score) |
| Smells | `smells` | 100 - (smell count penalty) |
| Cohesion | `cohesion` | 100 - (avg LCOM * 100) |

Cohesion is reported separately by default (penalizes non-OO codebases). Use `enable_cohesion = true` or `--enable-cohesion` to include it in the composite score.

### Default Weights

| Component | Weight | Rationale |
|-----------|--------|-----------|
| Complexity | 25% | Core maintainability signal |
| Defect Risk | 25% | Predictive of bugs |
| Duplication | 20% | Maintenance cost |
| Technical Debt | 15% | Acknowledged issues |
| Coupling | 10% | Change risk |
| Smells | 5% | Structural issues |

Composite = weighted sum of component scores, rounded to integer.

### Grade Scale

Standard academic grading scale:

| Score | Grade |
|-------|-------|
| 97-100 | A+ |
| 93-96 | A |
| 90-92 | A- |
| 87-89 | B+ |
| 83-86 | B |
| 80-82 | B- |
| 77-79 | C+ |
| 73-76 | C |
| 70-72 | C- |
| 67-69 | D+ |
| 63-66 | D |
| 60-62 | D- |
| 0-59 | F |

## Configuration

In `omen.toml`:

```toml
[score]
# Include cohesion in composite score (for OO-heavy codebases)
# When true, weights are automatically scaled by 0.85 and cohesion added at 0.15
enable_cohesion = false

# Weights for composite (must sum to 1.0)
[score.weights]
complexity = 0.25
defect = 0.25
duplication = 0.20
debt = 0.15
coupling = 0.10
smells = 0.05
cohesion = 0.0  # Or set manually if you prefer custom weights

# Default thresholds (--min-* flags override)
[score.thresholds]
score = 0        # 0 = no enforcement
complexity = 0
duplication = 0
defect = 0
debt = 0
coupling = 0
smells = 0
cohesion = 0
```

Setting a threshold > 0 in config enforces it on every run. CLI flags override config values.

When `enable_cohesion = true` and `cohesion` weight is 0, weights are automatically redistributed:
- Existing weights scaled by 0.85
- Cohesion added at 0.15 weight

## JSON Output

```json
{
  "score": 74,
  "grade": "C",
  "components": {
    "complexity": 82,
    "duplication": 91,
    "defect": 65,
    "debt": 70,
    "coupling": 78,
    "smells": 85,
    "cohesion": 68
  },
  "cohesion_included": false,
  "weights": {
    "complexity": 0.25,
    "duplication": 0.20,
    "defect": 0.25,
    "debt": 0.15,
    "coupling": 0.10,
    "smells": 0.05,
    "cohesion": 0.0
  },
  "files_analyzed": 247,
  "thresholds": {
    "score": { "min": 70, "passed": true },
    "complexity": { "min": 0, "passed": true }
  },
  "passed": true,
  "timestamp": "2025-12-03T10:30:00Z",
  "commit": "a36f06e"
}
```

Includes commit SHA and timestamp for correlating scores to git history.

## Implementation

### Package Structure

```
pkg/analyzer/score/
  score.go      # Analyzer orchestrating component analyzers
  types.go      # Score, ComponentScores, Result, Config
  weights.go    # Weight config, normalization functions
```

### Flow

1. `score.New(opts...)` creates analyzer with weight/threshold config
2. `AnalyzeProject(path)` runs component analyzers in parallel
3. Each result normalized to 0-100
4. Weighted sum produces composite score
5. Thresholds checked, result includes pass/fail status

### Analyzer Dependencies

Reuses existing analyzers (no logic duplication):

- `complexity.AnalyzeProject()` - function complexity metrics
- `duplicates.AnalyzeProject()` - code clone detection
- `defect.AnalyzeProject()` - defect probability prediction
- `satd.AnalyzeProject()` - self-admitted technical debt
- `graph.AnalyzeProject()` - coupling metrics
- `smells.AnalyzeProject()` - architectural smell detection
- `cohesion.AnalyzeProject()` - LCOM metrics (reported separately)

### CLI Registration

New top-level command in `cmd/omen/main.go` alongside `analyze`, `context`, `mcp`.

## MCP Integration

### Tool

`score_repository` - returns JSON score output for LLM analysis.

```
score_repository:
  paths: [optional, defaults to "."]
```

### Prompt

`check-score` - guides interpretation of score results, identifies what's dragging the score down, suggests improvements.

## Usage Examples

### Basic score check

```bash
omen score
```

### CI enforcement

```bash
omen score --min-score 70 --json
```

### Per-category thresholds

```bash
omen score --min-complexity 80 --min-duplication 90
```

### Score at specific commit

```bash
git checkout v1.2.0
omen score --json >> scores.jsonl
git checkout main
```

### Compare branches

```bash
git checkout main && omen score --json > main.json
git checkout feature && omen score --json > feature.json
# diff externally
```
