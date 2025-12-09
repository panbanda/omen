package score

import "math"

// =============================================================================
// SCORE NORMALIZATION
// =============================================================================
//
// Each component metric is normalized to a 0-100 score where higher is better.
// The normalization functions are designed to be:
//
// 1. FAIR: Different metrics with similar severity produce similar scores
// 2. CALIBRATED: Based on industry benchmarks where available
// 3. NON-LINEAR: Gentle penalties for minor issues, steep for severe ones
// 4. SEVERITY-AWARE: Weight items by impact, not just count
//
// References:
// - SonarQube quality gates: https://docs.sonarqube.org/latest/user-guide/metric-definitions/
// - CodeClimate maintainability: https://docs.codeclimate.com/docs/maintainability
// - CISQ quality measures: https://www.it-cisq.org/standards/
// =============================================================================

// -----------------------------------------------------------------------------
// Complexity Normalization
// -----------------------------------------------------------------------------
//
// Measures the percentage of functions exceeding complexity thresholds.
//
// Rationale:
// - Cyclomatic complexity >10 indicates hard-to-test code (McCabe, 1976) omen:ignore
// - Cognitive complexity >15 indicates hard-to-understand code (SonarSource)
// - Linear scaling is appropriate here since % violations is already normalized
//
// Benchmarks:
// - 0% violations: 100 (excellent)
// - 5% violations: 95 (good)
// - 10% violations: 90 (acceptable)
// - 20% violations: 80 (concerning)
// - 50% violations: 50 (poor)
// -----------------------------------------------------------------------------

// NormalizeComplexity converts complexity metrics to 0-100 score.
// Uses linear scaling since input is already a normalized percentage.
func NormalizeComplexity(totalFunctions, violatingFunctions int) int {
	if totalFunctions == 0 {
		return 100
	}
	ratio := float64(violatingFunctions) / float64(totalFunctions)
	score := int(math.Round(100 * (1 - ratio)))
	return clamp(score, 0, 100)
}

// -----------------------------------------------------------------------------
// Duplication Normalization
// -----------------------------------------------------------------------------
//
// Uses a NON-LINEAR scale calibrated against industry benchmarks.
//
// Rationale:
// - Small amounts of duplication (<3%) are often acceptable (boilerplate, tests)
// - 3-10% indicates emerging patterns that should be abstracted
// - 10-20% indicates systematic copy-paste that increases maintenance burden
// - >20% indicates severe technical debt requiring immediate attention omen:ignore
//
// Benchmarks (aligned with SonarQube/CodeClimate):
// - 0-3%: A (100-95) - Excellent, minimal duplication
// - 3-5%: B (95-90) - Good, acceptable for most projects
// - 5-10%: C (90-80) - Fair, consider refactoring
// - 10-20%: D (80-60) - Poor, significant maintenance risk
// - >20%: F (60-0) - Critical, requires immediate action
//
// The curve is piecewise linear with increasing slope at higher ratios,
// reflecting that each additional % of duplication is progressively worse.
// -----------------------------------------------------------------------------

// NormalizeDuplication converts duplication ratio to 0-100 score.
// Uses non-linear scaling: gentle at low ratios, steep at high ratios.
func NormalizeDuplication(ratio float64) int {
	var score float64
	switch {
	case ratio <= 0.03:
		// 0-3%: 100-95 (slope: -166.7 per 1.0 ratio)
		score = 100 - (ratio * 166.7)
	case ratio <= 0.05:
		// 3-5%: 95-90 (slope: -250 per 1.0 ratio)
		score = 95 - ((ratio - 0.03) * 250)
	case ratio <= 0.10:
		// 5-10%: 90-80 (slope: -200 per 1.0 ratio)
		score = 90 - ((ratio - 0.05) * 200)
	case ratio <= 0.20:
		// 10-20%: 80-60 (slope: -200 per 1.0 ratio)
		score = 80 - ((ratio - 0.10) * 200)
	default:
		// >20%: 60-0 (slope: -150 per 1.0 ratio, floors at 0)
		score = 60 - ((ratio - 0.20) * 150)
	}
	return clamp(int(math.Round(score)), 0, 100)
}

// omen:ignore - documentation describes SATD markers, not actual debt
// -----------------------------------------------------------------------------
// Self-Admitted Technical Debt (SATD) Normalization
// -----------------------------------------------------------------------------
//
// Uses SEVERITY-WEIGHTED scoring rather than raw item counts.
//
// Rationale:
// - Not all debt is equal: a SECURITY marker is far worse than a NOTE
// - Raw counts penalize well-documented codebases that track their debt
// - Density (per KLOC) scales appropriately with codebase size
//
// Severity weights (based on remediation urgency):
// - Critical (SECURITY, VULN): 4.0 - Immediate security risk
// - High (FIXME, BUG): 2.0 - Known defects requiring fix
// - Medium (HACK, REFACTOR): 1.0 - Design compromises
// - Low (TODO, NOTE): 0.25 - Future work, minimal impact
//
// The weighted density is then mapped to score:
// - 0 weighted items/KLOC: 100
// - 5 weighted items/KLOC: 90
// - 10 weighted items/KLOC: 80
// - 20 weighted items/KLOC: 60
// - 50+ weighted items/KLOC: 0
// -----------------------------------------------------------------------------

// SATDSeverityCounts holds SATD item counts by severity level.
// omen:ignore - field comments describe SATD markers, not actual debt
type SATDSeverityCounts struct {
	Critical int
	High     int
	Medium   int
	Low      int
}

// NormalizeSATD converts SATD severity counts to 0-100 score.
// Uses severity-weighted density per 1K LOC.
func NormalizeSATD(counts SATDSeverityCounts, loc int) int {
	if loc == 0 {
		return 100
	}

	// Severity weights reflect remediation urgency
	const (
		criticalWeight = 4.0
		highWeight     = 2.0
		mediumWeight   = 1.0
		lowWeight      = 0.25
	)

	weighted := float64(counts.Critical)*criticalWeight +
		float64(counts.High)*highWeight +
		float64(counts.Medium)*mediumWeight +
		float64(counts.Low)*lowWeight

	densityPer1K := weighted / float64(loc) * 1000

	// Map density to score: 0->100, 10->80, 50->0
	// Using a curve that's gentle at low density, steep at high
	score := 100 - (densityPer1K * 2)
	return clamp(int(math.Round(score)), 0, 100)
}

// -----------------------------------------------------------------------------
// Technical Debt Gradient (TDG) Normalization
// -----------------------------------------------------------------------------
//
// The TDG analyzer produces a comprehensive 0-100 score (higher is better)
// based on structural complexity, semantic complexity, duplication, coupling,
// documentation, and consistency.
//
// Since TDG already outputs a 0-100 score, we pass it through with minor
// adjustments to align with our scoring philosophy.
// -----------------------------------------------------------------------------

// NormalizeTDG converts TDG average score to 0-100 component score.
// TDG scores are already 0-100 (higher = better quality).
func NormalizeTDG(avgScore float32) int {
	return clamp(int(math.Round(float64(avgScore))), 0, 100)
}

// -----------------------------------------------------------------------------
// Coupling Normalization
// -----------------------------------------------------------------------------
//
// Measures architectural coupling using MULTIPLE signals, not just instability.
//
// Rationale:
// - Average instability alone is insufficient (can be 0 with no dependencies)
// - Cyclic dependencies are critical architectural violations
// - The Stable Dependencies Principle (SDP) violations indicate fragility
// - Hub components create single points of failure
//
// Signals and their weights:
// - Cyclic dependencies: -15 points each (critical, breaks builds)
// - SDP violations: -5 points each (unstable deps from stable modules)
// - Average instability: maps 0-1 to 100-50 (baseline coupling measure)
//
// A codebase with no detected issues but also no analyzed components
// should NOT score 100 - this indicates insufficient data.
// -----------------------------------------------------------------------------

// CouplingMetrics holds coupling analysis results.
type CouplingMetrics struct {
	CyclicCount        int     // Number of cyclic dependency groups
	SDPViolations      int     // Stable Dependencies Principle violations
	AverageInstability float64 // Mean instability across modules (0-1)
	TotalComponents    int     // Number of analyzed components
}

// NormalizeCoupling converts coupling metrics to 0-100 score.
// Combines multiple signals: cycles, SDP violations, and instability.
// Uses size-relative deductions so large codebases aren't unfairly penalized.
func NormalizeCoupling(m CouplingMetrics) int {
	// If no components analyzed, we can't assess coupling - return neutral score
	if m.TotalComponents == 0 {
		return 75 // Neutral, not penalized but not perfect
	}

	// Start with instability-based score (0 instability = 100, 1.0 = 50)
	// This gives a baseline that doesn't penalize healthy instability
	baseScore := 100 - (m.AverageInstability * 50)

	// Calculate cycle density (cycles per 100 components)
	// A large codebase with a few cycles is less concerning than a small one
	cycleDensity := float64(m.CyclicCount) / float64(m.TotalComponents) * 100

	// Map cycle density to deduction: 0->0, 1%->15, 3%->35, 5%->50
	// Cycles are still critical, but scaled to codebase size
	cycleDeduction := math.Min(cycleDensity*10, 50)

	// SDP violation density (violations per 100 components)
	sdpDensity := float64(m.SDPViolations) / float64(m.TotalComponents) * 100

	// Map SDP density to deduction: 0->0, 1%->5, 5%->20, 10%->30 (capped)
	sdpDeduction := math.Min(sdpDensity*2, 30)

	score := baseScore - cycleDeduction - sdpDeduction
	return clamp(int(math.Round(score)), 0, 100)
}

// -----------------------------------------------------------------------------
// Architectural Smells Normalization
// -----------------------------------------------------------------------------
//
// Scales deductions relative to codebase size.
//
// Rationale:
// - Fixed point deductions unfairly penalize large codebases
// - A single god component in 100 files is worse than in 10 files
// - Smell density (smells per component) is a fairer measure omen:ignore
//
// Severity impacts (per component in codebase):
// - Critical smells (cycles): High impact on build/test omen:ignore
// - High smells (god components, hubs): High change risk omen:ignore
// - Medium smells (unstable deps): Moderate maintenance risk omen:ignore
//
// Scoring approach:
// - Calculate weighted smell density omen:ignore
// - Map density to score using sigmoid-like curve
// - Ensures 0 smells = 100, but doesn't require perfection for good score omen:ignore
// -----------------------------------------------------------------------------

// SmellCounts holds architectural smell counts by severity. omen:ignore
type SmellCounts struct {
	Critical int // Cyclic dependencies
	High     int // God components, hub components
	Medium   int // Unstable dependencies
}

// NormalizeSmells converts smell counts to 0-100 score. omen:ignore
// Scales deductions relative to codebase size (total components). omen:ignore
func NormalizeSmells(counts SmellCounts, totalComponents int) int {
	if totalComponents == 0 {
		return 100
	}

	// Weight smells by severity
	weighted := float64(counts.Critical)*3 + float64(counts.High)*2 + float64(counts.Medium)

	// Calculate smell density (smells per 10 components) omen:ignore
	densityPer10 := weighted / float64(totalComponents) * 10

	// Map density to score: 0->100, 1->90, 3->70, 5->50, 10->0
	// Using steeper curve since architectural smells are serious
	score := 100 - (densityPer10 * 10)
	return clamp(int(math.Round(score)), 0, 100)
}

// -----------------------------------------------------------------------------
// Cohesion Normalization
// -----------------------------------------------------------------------------
//
// Maps average LCOM (Lack of Cohesion of Methods) to score.
//
// Rationale:
// - LCOM measures how well methods in a class use shared fields
// - LCOM of 0 means perfect cohesion (all methods use all fields)
// - LCOM of 1 means no cohesion (methods share no fields)
// - Non-OO codebases (Go packages, C modules) may have naturally higher LCOM
//
// The linear mapping is appropriate since LCOM is already normalized 0-1.
// However, we're slightly more forgiving (0.5 LCOM = 60 score instead of 50)
// to account for legitimate architectural patterns.
// -----------------------------------------------------------------------------

// NormalizeCohesion converts average LCOM4 to 0-100 score.
// LCOM4 counts connected components in the method-field graph:
// - LCOM4 = 1: perfectly cohesive (all methods share fields)
// - LCOM4 > 1: class could potentially be split into N parts
//
// Scoring: LCOM4=1 -> 100, LCOM4=2 -> 70, LCOM4=5 -> 40, LCOM4=10+ -> 0
func NormalizeCohesion(avgLCOM float64) int {
	if avgLCOM <= 1.0 {
		return 100
	}
	// Linear decay from 100 at LCOM=1 to 0 at LCOM=10
	// Each unit above 1 subtracts ~11 points
	score := 100 - ((avgLCOM - 1) * 11.1)
	return clamp(int(math.Round(score)), 0, 100)
}

func clamp(value, min, max int) int {
	if value < min {
		return min
	}
	if value > max {
		return max
	}
	return value
}
