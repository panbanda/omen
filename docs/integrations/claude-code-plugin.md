---
sidebar_position: 3
---

# Claude Code Plugin

Omen provides a Claude Code plugin that packages common analysis workflows into skills -- predefined sequences of tool calls that combine multiple analyzers to answer high-level questions about a codebase.

## Installation

```bash
/plugin install panbanda/omen
```

After installation, verify the skills are available:

```bash
/skills
```

You should see the Omen skills listed among your available skills.

## Prerequisites

The plugin requires Omen's MCP server to be configured. If you haven't already set it up:

```bash
claude mcp add omen -- omen mcp
```

The plugin invokes Omen tools through the MCP server. Without it, skills will fail with a connection error.

## Available Skills

The plugin provides 11 skills that cover the most common analysis workflows.

### setup-config

Generate an `omen.toml` configuration file tailored to the current project. Analyzes the codebase to detect languages, project structure, and test frameworks, then produces a configuration with appropriate defaults.

```
/setup-config
```

Creates `omen.toml` in the project root with analyzer thresholds, excluded paths, and score weights calibrated for the detected project type.

### onboard-codebase

Analyze a codebase you are unfamiliar with. Runs repository score, complexity, dependency graph, ownership, and repository map to produce a structured overview of the project's architecture, health, and key areas of concern.

```
/onboard-codebase
```

Useful when you are starting work on a new project or reviewing a repository for the first time. The output covers: overall health score, most complex modules, dependency structure, primary contributors per area, and areas of concern.

### check-quality

Run quality gates and report pass/fail status. Checks the repository score against the configured threshold and breaks down component scores so you can see which dimensions are dragging the overall score down.

```
/check-quality
```

Returns the overall score, per-component breakdown, and whether the configured minimum threshold is met. Equivalent to `omen score` but with interpreted results and recommended actions.

### find-bugs

Identify areas of the codebase most likely to contain defects. Combines defect prediction, hotspot analysis, and complexity data to produce a ranked list of high-risk files and functions.

```
/find-bugs
```

The output prioritizes files that score high on multiple risk signals simultaneously: high complexity, frequent changes, low test coverage (via mutation testing), and prior defect history. Files that appear as hotspots across multiple dimensions are the highest priority for review and testing.

### focus-review

Prioritize code review effort. Given the current set of changes (uncommitted or a branch diff), identifies which changed files carry the most risk and recommends review order.

```
/focus-review
```

Uses diff analysis, churn history, ownership data, and complexity to rank changed files by risk. Files modified by a single contributor with no prior ownership are flagged as higher risk (the "bus factor" concern).

### audit-flags

Check feature flag hygiene. Reports all detected feature flags, their staleness status, and recommends flags for cleanup.

```
/audit-flags
```

Returns a list of all flags grouped by provider, with stale flags highlighted and sorted by age. Includes the total number of references for each flag, making it easier to estimate cleanup effort.

### audit-architecture

Detect architectural issues including dependency cycles, god classes, high coupling, low cohesion, and dead code. Combines dependency graph analysis, CK metrics, code smells, and dead code detection.

```
/audit-architecture
```

The output identifies specific architectural problems with locations and recommended actions. Dependency cycles are reported with the full cycle path. God classes are identified by WMC and method count. High-coupling modules are listed with their afferent and efferent coupling counts.

### plan-refactoring

Prioritize refactoring work by combining technical debt gradient, complexity, code smells, and churn data. Produces a ranked list of refactoring targets ordered by impact.

```
/plan-refactoring
```

Each recommendation includes the target (file, class, or function), the specific issues detected, an estimated effort level, and the expected impact on the repository score. This helps teams make data-driven decisions about which refactoring work to prioritize.

### target-tests

Focus testing effort where it matters most. Uses mutation testing results, coverage data, defect prediction, and hotspot analysis to identify areas where additional tests would have the highest return on investment.

```
/target-tests
```

Returns a ranked list of files and functions that would benefit most from additional testing, along with the specific type of testing recommended (unit tests for uncovered branches, integration tests for high-coupling areas, etc.).

### track-trends

Monitor codebase health over time. Runs score trend analysis and compares current metrics against historical baselines to identify whether quality is improving, stable, or degrading.

```
/track-trends
```

Shows the trajectory of the repository score and its components over recent history. Highlights significant changes (positive or negative) and correlates them with the commits that caused them.

### report-debt

Generate a comprehensive technical debt report. Combines SATD detection, technical debt gradient, code smells, complexity analysis, and dependency issues into a structured debt inventory.

```
/report-debt
```

Produces a categorized inventory of all detected technical debt with severity ratings, locations, and estimated effort for remediation. Suitable for sprint planning, architecture reviews, or stakeholder reporting.

## Usage Patterns

### New Project Onboarding

When starting work on an unfamiliar codebase:

1. `/setup-config` -- generate configuration
2. `/onboard-codebase` -- get the structural overview
3. `/audit-architecture` -- understand dependency structure and problem areas

### Pre-merge Review

Before merging a PR:

1. `/focus-review` -- prioritize which files to review
2. `/check-quality` -- verify the quality gate passes
3. `/find-bugs` -- check if changes touch high-risk areas

### Sprint Planning

When deciding what technical work to prioritize:

1. `/report-debt` -- inventory current debt
2. `/plan-refactoring` -- rank refactoring targets by impact
3. `/target-tests` -- identify testing gaps

### Periodic Health Checks

On a regular cadence (weekly, per-sprint, per-release):

1. `/track-trends` -- check health trajectory
2. `/audit-flags` -- clean up stale flags
3. `/check-quality` -- verify score is above threshold
