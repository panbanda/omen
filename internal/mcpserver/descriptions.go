package mcpserver

// Tool descriptions with interpretation guidance for LLMs.
// Each description explains what the tool does, when to use it,
// how to interpret results, and key thresholds.

func describeComplexity() string {
	return `Measures cyclomatic and cognitive complexity of functions across a codebase.

USE WHEN:
- Identifying functions that are hard to test or maintain
- Finding refactoring candidates before code reviews
- Assessing overall code quality trends
- Prioritizing technical debt remediation

INTERPRETING RESULTS:
- Cyclomatic complexity > 10: function has many code paths, consider splitting
- Cyclomatic complexity > 20: high risk, strong refactoring candidate
- Cognitive complexity > 15: function is hard to understand, simplify logic
- Cognitive complexity > 30: very difficult to maintain
- MaxNesting > 4: deeply nested code, consider early returns or extraction
- P90 values show the 90th percentile across all functions (codebase trend)

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
- Tracking debt trends over time

INTERPRETING RESULTS:
- Severity levels: critical > high > medium > low
- Critical: Security-related debt (SECURITY, VULN, UNSAFE)
- High: Known defects (FIXME, BUG, BROKEN)
- Medium: Design compromises (HACK, KLUDGE, REFACTOR)
- Low: Future work (TODO, NOTE, OPTIMIZE)
- Categories: design, defect, requirement, test, performance, security

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
- Improving code maintainability

INTERPRETING RESULTS:
- Confidence score 0.0-1.0: higher means more likely truly unused
- Confidence > 0.8: high confidence, safe to investigate removal
- Confidence 0.5-0.8: medium confidence, verify usage manually
- Confidence < 0.5: lower confidence, may have dynamic usage
- Exported symbols may be used by external packages

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
- Discovering hidden complexity not visible in static analysis
- Planning refactoring priorities based on change patterns

INTERPRETING RESULTS:
- ChurnScore: normalized 0-100, higher means more volatile
- High churn + high complexity = prime refactoring target
- Many unique authors on one file may indicate unclear ownership
- Frequent small changes may indicate poor abstraction
- Files with high additions AND deletions are being reworked

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
- Preparing for DRY (Don't Repeat Yourself) improvements

INTERPRETING RESULTS:
- Similarity threshold: 0.0-1.0, higher means more similar
- Exact clones (1.0): identical code blocks
- Near clones (0.8-0.99): minor variations, likely copy-paste
- Similar blocks (0.6-0.8): related logic, consider abstraction
- Clone groups show all instances of the same duplicated code

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
- Making data-driven refactoring decisions

INTERPRETING RESULTS:
- Defect probability: 0.0-1.0, higher means higher risk
- > 0.7: High risk, prioritize review and testing
- 0.4-0.7: Medium risk, worth attention
- < 0.4: Lower risk, but not immune to bugs
- Combines: complexity, churn, ownership diffusion, code age

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
- Understanding which types of changes introduce defects

INTERPRETING RESULTS:
- Risk score: 0-1, higher means more likely to introduce bugs
- High risk (>0.7): commit should be carefully reviewed
- Medium risk (0.4-0.7): worth extra attention
- Low risk (<0.4): typical commit, standard review
- Bug fix commits indicate prior defects in touched files

JIT FACTORS (from Kamei et al. research):
- LA (lines added): more lines = more risk
- LD (lines deleted): fewer deletions is safer
- LT (lines in touched files): larger files = more risk
- FIX: bug fix commits indicate problematic areas
- NDEV (developers on files): more developers = more risk
- AGE (average file age): older files may be stable or legacy
- NUC (unique changes): high entropy = higher risk
- EXP (developer experience): less experience = more risk

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
- Tracking debt trends across releases
- Identifying files that consistently accumulate debt

INTERPRETING RESULTS:
- TDG score: composite metric, higher means more accumulated debt
- Combines: complexity, SATD markers, code smells, coupling
- Hotspot files: high TDG + high churn = urgent attention
- Stable files with high TDG: legacy debt, plan remediation
- Rising TDG trend: debt is accumulating, investigate causes

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
- Visualizing import/export relationships

INTERPRETING RESULTS:
- Edges show dependency direction (A depends on B)
- High in-degree: many dependents, changes have wide impact
- High out-degree: many dependencies, potentially fragile
- Cycles indicate circular dependencies (problematic)
- Isolated nodes may be dead code or entry points

METRICS RETURNED:
- Nodes: files/modules/functions depending on scope
- Edges: dependency relationships with counts
- Metrics (when enabled): in-degree, out-degree per node
- Output can be used with graph visualization tools

Scope options: file (default), function, module.`
}

func describeHotspot() string {
	return `Identifies hotspots: files with both high churn AND high complexity.

USE WHEN:
- Finding the most problematic files in a codebase
- Prioritizing refactoring for maximum impact
- Identifying code that changes often but is hard to change safely
- Making the case for technical debt investment

INTERPRETING RESULTS:
- Hotspot score combines churn and complexity metrics
- High churn + high complexity = highest priority hotspot
- High churn + low complexity = frequently changing but manageable
- Low churn + high complexity = complex but stable, lower priority
- Top hotspots are the best refactoring candidates

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
- Understanding change propagation patterns

INTERPRETING RESULTS:
- Co-change count: number of commits where both files changed
- Coupling strength: higher means stronger implicit dependency
- Files always changing together may belong in same module
- Unexpected couplings may indicate hidden dependencies
- High coupling across module boundaries = architectural smell

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
- Assessing team distribution across the codebase
- Planning for team member transitions

INTERPRETING RESULTS:
- Bus factor: minimum contributors whose absence would halt work
- Bus factor = 1: critical risk, only one person knows this code
- Bus factor = 2-3: moderate risk, knowledge should be spread
- Ownership percentage: contribution share per author
- Dominant owner > 80%: potential knowledge silo

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
- Measuring inheritance hierarchy depth

INTERPRETING RESULTS:
- LCOM (Lack of Cohesion): higher = less cohesive, consider splitting
- LCOM > 0.8: class likely has multiple responsibilities
- WMC (Weighted Methods per Class): method complexity sum
- WMC > 20: class may be too complex
- CBO (Coupling Between Objects): number of dependencies
- CBO > 10: high coupling, harder to change in isolation
- DIT (Depth of Inheritance Tree): inheritance levels
- DIT > 4: deep hierarchy, may be over-engineered

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
- Identifying central components that many things depend on

INTERPRETING RESULTS:
- PageRank score: importance based on reference graph
- Higher rank = more referenced by other code
- Top symbols are likely core abstractions or entry points
- Types/interfaces with high rank are key domain concepts
- Functions with high rank are frequently called utilities

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
- Preparing architecture review documentation

INTERPRETING RESULTS:
Severity levels (critical > high > medium > low):

CYCLIC DEPENDENCIES (critical):
- Circular imports/dependencies between modules
- Makes changes risky, testing difficult
- Should be broken with dependency inversion

HUB COMPONENTS (high):
- High fan-in AND fan-out (connects everything)
- Changes here affect many parts of the system
- Consider splitting or using interfaces

GOD COMPONENTS (high):
- Extremely high coupling, does too much
- Fan-in > threshold AND fan-out > threshold
- Strong refactoring candidate

UNSTABLE DEPENDENCIES (medium):
- Stable module depends on unstable module
- Violates Stable Dependencies Principle
- Can cause cascading changes

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
- Identifying complex flag implementations
- Planning feature flag debt remediation

SUPPORTED PROVIDERS:
- LaunchDarkly (JS/TS, Python, Go, Java)
- Split.io (JS/TS, Python, Go)
- Unleash (JS/TS, Go)
- PostHog (JS/TS, Python, Go, Java, Ruby)
- Flipper (Ruby)

INTERPRETING RESULTS:
Priority levels (CRITICAL > HIGH > MEDIUM > LOW):
- CRITICAL: Very stale (>90 days) + high complexity
- HIGH: Stale flags (>30 days) or high file spread
- MEDIUM: Moderate staleness or complexity
- LOW: Recent flags with low complexity

COMPLEXITY METRICS:
- FileSpread: Number of files containing flag references
- MaxNestingDepth: Deepest conditional nesting level
- DecisionPoints: Total flag check locations
- CoupledFlags: Other flags used in same conditionals

STALENESS METRICS (requires git):
- DaysSinceIntro: Days since flag first appeared
- DaysSinceModified: Days since last flag-related change
- Authors: Contributors who touched the flag
- StalenessScore: 0-1 score, higher = more stale

METRICS RETURNED:
- Per-flag: key, provider, references, complexity, staleness, priority
- Summary: total flags, by priority, by provider, avg file spread
- References: file locations with line numbers

Use git history for accurate staleness analysis. Disable with include_git=false for faster results.`
}
