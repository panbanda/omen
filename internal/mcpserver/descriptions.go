package mcpserver

// Tool descriptions optimized for LLM context efficiency.
// Keep descriptions concise - focus on what the tool does and when to use it.

func describeComplexity() string {
	return `Measures cyclomatic and cognitive complexity of functions across a codebase.

USE WHEN:
- Identifying functions that are hard to test or maintain
- Finding refactoring candidates before code reviews
- Prioritizing technical debt remediation

METRICS RETURNED:
- Per-function: cyclomatic, cognitive, max_nesting, lines
- Per-file: function list, averages and totals
- Summary: P50, P90, P95 percentiles, max values, total functions`
}

func describeSATD() string {
	return `Detects self-admitted technical debt markers in code comments (TODO, FIXME, HACK, etc.).

USE WHEN:
- Auditing technical debt before a release
- Finding forgotten workarounds and temporary fixes
- Identifying security-related debt markers

METRICS RETURNED:
- Items: list of debt markers with file, line, severity, category
- Summary: counts by severity, by category, files with debt
- Context hash for tracking individual items over time`
}

func describeDeadcode() string {
	return `Identifies potentially unused functions, methods, and variables in a codebase.

USE WHEN:
- Cleaning up code before major refactoring
- Reducing binary size and compile times
- Finding orphaned code after feature removal

METRICS RETURNED:
- Unused items: functions, methods, variables with confidence scores
- Per-file grouping with language detection
- Summary: total unused by type, distribution by confidence

Note: Dynamic dispatch, reflection, and external consumers can cause false positives.`
}

func describeChurn() string {
	return `Analyzes git history to identify files that change frequently (high churn).

USE WHEN:
- Finding unstable or problematic code areas
- Identifying files that may need architectural attention
- Planning refactoring priorities based on change patterns

METRICS RETURNED:
- Per-file: commits, lines added/deleted, unique authors, churn score
- Author contributions across the codebase
- Time range of analysis (configurable days)

Requires git repository. Analyzes commits within the specified time period.`
}

func describeDuplicates() string {
	return `Detects code clones and duplicated code blocks across the codebase.

USE WHEN:
- Finding copy-paste code that should be refactored
- Identifying candidates for shared utilities or abstractions
- Reducing maintenance burden from duplicated logic

METRICS RETURNED:
- Clone groups: sets of similar code blocks
- Per-clone: file, start/end lines, similarity score
- Summary: total clones, lines duplicated, duplication percentage

Larger min_lines reduces noise from trivial duplicates.`
}

func describeDefect() string {
	return `Predicts defect probability using PMAT-weighted metrics combining complexity, churn, and ownership.

USE WHEN:
- Prioritizing code review focus areas
- Identifying high-risk files before releases
- Planning testing effort allocation

METRICS RETURNED:
- Per-file: defect probability, contributing factors
- Risk factors breakdown: complexity score, churn score, ownership score
- Ranked list from highest to lowest risk

Requires git repository for churn and ownership data.`
}

func describeChanges() string {
	return `Analyzes recent changes for defect risk using Just-in-Time prediction (Kamei et al. 2013).

USE WHEN:
- Reviewing recent commits for bug risk before release
- Identifying risky changes that need extra review
- Prioritizing code review effort on high-risk commits

METRICS RETURNED:
- Per-commit: hash, author, message, risk score, risk level
- Risk factors breakdown for each commit
- Summary: total commits, risk distribution, bug fix count

Requires git repository. Analyzes commits within the specified time period.`
}

func describeTDG() string {
	return `Calculates Technical Debt Gradient scores to identify debt accumulation patterns.

USE WHEN:
- Finding areas where debt is increasing over time
- Prioritizing debt paydown by impact
- Identifying files that consistently accumulate debt

METRICS RETURNED:
- Per-file: TDG score, component scores
- Hotspots: files ranked by TDG score
- Summary: average TDG, distribution, trend indicators`
}

func describeGraph() string {
	return `Generates dependency graphs showing relationships between code modules.

USE WHEN:
- Understanding codebase architecture
- Finding circular dependencies
- Identifying tightly coupled modules
- Planning module extraction or refactoring

METRICS RETURNED:
- Nodes: files/modules/functions depending on scope
- Edges: dependency relationships with counts
- Metrics (when enabled): in-degree, out-degree per node

Scope options: file (default), function, module.`
}

func describeHotspot() string {
	return `Identifies hotspots: files with both high churn AND high complexity.

USE WHEN:
- Finding the most problematic files in a codebase
- Prioritizing refactoring for maximum impact
- Identifying code that changes often but is hard to change safely

METRICS RETURNED:
- Per-file: hotspot score, churn metrics, complexity metrics
- Ranked list from hottest to coldest
- Summary: hotspot distribution, thresholds used

Requires git repository. Combines churn analysis with complexity analysis.`
}

func describeTemporalCoupling() string {
	return `Finds files that frequently change together in git history, indicating hidden dependencies.

USE WHEN:
- Discovering implicit dependencies not visible in code
- Finding files that should be co-located or merged
- Identifying architectural issues (shotgun surgery)

METRICS RETURNED:
- File pairs with co-change frequency
- Coupling strength normalized by individual change frequency
- Filtered by minimum co-changes threshold

Requires git repository. Analyzes commits within the specified time period.`
}

func describeOwnership() string {
	return `Analyzes code ownership patterns and calculates bus factor risk.

USE WHEN:
- Identifying knowledge silos and single points of failure
- Finding code that needs knowledge transfer
- Planning for team member transitions

METRICS RETURNED:
- Per-file: contributors, contribution percentages, bus factor
- Top contributors across the codebase
- Files with lowest bus factor (highest risk)
- Summary: average bus factor, ownership distribution

Requires git repository.`
}

func describeCohesion() string {
	return `Calculates Chidamber-Kemerer object-oriented metrics including LCOM, WMC, CBO, and DIT.

USE WHEN:
- Assessing class/struct design quality
- Finding classes that do too much (god classes)
- Identifying tightly coupled components

METRICS RETURNED:
- Per-class/struct: LCOM, WMC, CBO, DIT, RFC, NOC
- Summary: averages, distributions, outliers
- Sorted by chosen metric (lcom, wmc, cbo, or dit)`
}

func describeRepoMap() string {
	return `Generates a PageRank-weighted map of important symbols in the repository.

USE WHEN:
- Understanding the most important functions/types in a codebase
- Finding entry points and core abstractions
- Getting oriented in an unfamiliar codebase

METRICS RETURNED:
- Ranked list of symbols (functions, types, methods)
- Per-symbol: name, file, line, rank score
- Symbol type classification

Useful for LLM context: shows which symbols matter most for understanding the codebase.`
}

func describeSmells() string {
	return `Detects architectural smells: cycles, hubs, god components, and unstable dependencies.

USE WHEN:
- Auditing architecture before major refactoring
- Finding structural problems that cause maintenance pain
- Identifying components that violate design principles

METRICS RETURNED:
- Smells: type, severity, components involved, description
- Summary: counts by type and severity
- Components: specific files/modules with issues

Thresholds are configurable (hub-threshold, god-fan-in, god-fan-out, instability-diff).`
}

func describeFlags() string {
	return `Detects feature flags in source code and analyzes them for staleness and cleanup priority.

USE WHEN:
- Auditing feature flag usage before cleanup sprints
- Finding stale flags that should be removed
- Planning feature flag debt remediation

SUPPORTED PROVIDERS:
- LaunchDarkly (JS/TS, Python, Go, Java)
- Split.io (JS/TS, Python, Go)
- Unleash (JS/TS, Go)
- PostHog (JS/TS, Python, Go, Java, Ruby)
- Flipper (Ruby)

METRICS RETURNED:
- Per-flag: key, provider, references, complexity, staleness, priority
- Summary: total flags, by priority, by provider, avg file spread
- References: file locations with line numbers

Use git history for accurate staleness analysis. Disable with include_git=false for faster results.`
}

func describeScore() string {
	return `Compute repository health score (0-100) with component breakdown.

USE WHEN:
- Checking overall code health at a glance
- CI/CD quality gates before merge or release
- Tracking metrics trends over time
- Comparing code quality before/after refactoring

INTERPRETING RESULTS:
- Score 90-100: Excellent health, minimal issues
- Score 80-89: Good health, some improvement areas
- Score 70-79: Fair health, needs attention
- Score 50-69: Poor health, significant issues
- Score 0-49: Critical, requires immediate attention

COMPONENT WEIGHTS (default):
- Complexity: 25% - Function complexity issues
- Defect Risk: 25% - Predicted defect probability
- Duplication: 20% - Code clone ratio
- Technical Debt: 15% - SATD marker density
- Coupling: 10% - Module instability
- Smells: 5% - Architectural issues

METRICS RETURNED:
- score: Weighted composite (0-100)
- components: Individual scores per category
- cohesion: CK cohesion metrics (informational, not in composite by default)
- files_analyzed: Number of files included
- commit: Git commit SHA (if available)
- passed: Whether all thresholds met`
}

func describeTrend() string {
	return `Analyzes repository health score trends over time using git history.

USE WHEN:
- Tracking code quality improvements or degradation over time
- Preparing quarterly engineering reviews with historical data
- Identifying when code quality changes occurred

METRICS RETURNED:
- points: Array of {date, commit_sha, score, components}
- slope, intercept, r_squared, correlation: Overall regression stats
- component_trends: Per-component trend statistics
- start_score, end_score, total_change: Summary values

Requires git repository. Analysis time depends on history length and sampling period.`
}

func describeContext() string {
	return `Get deep context for a specific file or symbol before making changes.

USE WHEN:
- About to modify a file and need to understand its context
- Need to understand a function's complexity, callers, and dependencies
- Want to assess risk before touching unfamiliar code

INPUT RESOLUTION:
The focus parameter is resolved in this order:
1. Exact file path - if file exists at path
2. Glob pattern - if contains *, ?, or [ characters
3. Basename search - if looks like filename (has extension)
4. Symbol search - if repo map is available and name matches a symbol

METRICS RETURNED:
For files:
- Complexity: cyclomatic/cognitive totals, per-function breakdown
- Technical debt: SATD markers (TODO, FIXME, HACK) with line numbers

For symbols:
- Definition: file, line, kind (function/method/type)
- Complexity: metrics for the specific function`
}
