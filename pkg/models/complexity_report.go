package models

import (
	"fmt"
	"sort"
)

// AggregateResults creates a ComplexityReport from file metrics using default thresholds.
func AggregateResults(files []FileComplexity) *ComplexityReport {
	return AggregateResultsWithThresholds(files, nil, nil)
}

// AggregateResultsWithThresholds creates a ComplexityReport with custom thresholds.
// If maxCyclomatic or maxCognitive is nil, defaults are used.
func AggregateResultsWithThresholds(files []FileComplexity, maxCyclomatic, maxCognitive *uint32) *ComplexityReport {
	thresholds := buildCustomThresholds(maxCyclomatic, maxCognitive)

	var allCyclomatic, allCognitive []uint32
	var violations []Violation
	var hotspots []ComplexityHotspot
	var totalFunctions int

	for _, file := range files {
		for _, fn := range file.Functions {
			totalFunctions++
			allCyclomatic = append(allCyclomatic, fn.Metrics.Cyclomatic)
			allCognitive = append(allCognitive, fn.Metrics.Cognitive)

			// Check for violations
			checkFunctionViolations(&fn, file.Path, thresholds, &violations)

			// Check for hotspots
			checkFunctionHotspots(&fn, file.Path, thresholds, &hotspots)
		}
	}

	// Sort metrics for percentile calculation
	sort.Slice(allCyclomatic, func(i, j int) bool { return allCyclomatic[i] < allCyclomatic[j] })
	sort.Slice(allCognitive, func(i, j int) bool { return allCognitive[i] < allCognitive[j] })

	// Calculate statistics
	summary := ExtendedComplexitySummary{
		TotalFiles:       len(files),
		TotalFunctions:   totalFunctions,
		MedianCyclomatic: calculateMedian(allCyclomatic),
		MedianCognitive:  calculateMedian(allCognitive),
		MaxCyclomatic:    maxValue(allCyclomatic),
		MaxCognitive:     maxValue(allCognitive),
		P90Cyclomatic:    percentileU32(allCyclomatic, 90),
		P90Cognitive:     percentileU32(allCognitive, 90),
	}

	// Calculate technical debt
	debtHours := calculateTechnicalDebt(violations)
	summary.TechnicalDebtHours = debtHours

	// Sort hotspots by complexity (descending) and limit to top 10
	sort.Slice(hotspots, func(i, j int) bool { return hotspots[i].Complexity > hotspots[j].Complexity })
	if len(hotspots) > 10 {
		hotspots = hotspots[:10]
	}

	return &ComplexityReport{
		Summary:            summary,
		Violations:         violations,
		Hotspots:           hotspots,
		Files:              files,
		TechnicalDebtHours: debtHours,
	}
}

// buildCustomThresholds creates thresholds from optional parameters.
func buildCustomThresholds(maxCyclomatic, maxCognitive *uint32) ExtendedComplexityThresholds {
	thresholds := DefaultExtendedThresholds()

	if maxCyclomatic != nil {
		thresholds.CyclomaticError = *maxCyclomatic
		if *maxCyclomatic > 5 {
			thresholds.CyclomaticWarn = *maxCyclomatic - 5
		} else {
			thresholds.CyclomaticWarn = 1
		}
	}

	if maxCognitive != nil {
		thresholds.CognitiveError = *maxCognitive
		if *maxCognitive > 5 {
			thresholds.CognitiveWarn = *maxCognitive - 5
		} else {
			thresholds.CognitiveWarn = 1
		}
	}

	return thresholds
}

// checkFunctionViolations checks a function for complexity violations.
func checkFunctionViolations(fn *FunctionComplexity, file string, t ExtendedComplexityThresholds, violations *[]Violation) {
	// Cyclomatic complexity check
	if fn.Metrics.Cyclomatic > t.CyclomaticError {
		*violations = append(*violations, Violation{
			Severity:  SeverityError,
			Rule:      "cyclomatic-complexity",
			Message:   fmt.Sprintf("Cyclomatic complexity of %d exceeds maximum allowed complexity of %d", fn.Metrics.Cyclomatic, t.CyclomaticError),
			Value:     fn.Metrics.Cyclomatic,
			Threshold: t.CyclomaticError,
			File:      file,
			Line:      fn.StartLine,
			Function:  fn.Name,
		})
	} else if fn.Metrics.Cyclomatic > t.CyclomaticWarn {
		*violations = append(*violations, Violation{
			Severity:  SeverityWarning,
			Rule:      "cyclomatic-complexity",
			Message:   fmt.Sprintf("Cyclomatic complexity of %d exceeds recommended complexity of %d", fn.Metrics.Cyclomatic, t.CyclomaticWarn),
			Value:     fn.Metrics.Cyclomatic,
			Threshold: t.CyclomaticWarn,
			File:      file,
			Line:      fn.StartLine,
			Function:  fn.Name,
		})
	}

	// Cognitive complexity check
	if fn.Metrics.Cognitive > t.CognitiveError {
		*violations = append(*violations, Violation{
			Severity:  SeverityError,
			Rule:      "cognitive-complexity",
			Message:   fmt.Sprintf("Cognitive complexity of %d exceeds maximum allowed complexity of %d", fn.Metrics.Cognitive, t.CognitiveError),
			Value:     fn.Metrics.Cognitive,
			Threshold: t.CognitiveError,
			File:      file,
			Line:      fn.StartLine,
			Function:  fn.Name,
		})
	} else if fn.Metrics.Cognitive > t.CognitiveWarn {
		*violations = append(*violations, Violation{
			Severity:  SeverityWarning,
			Rule:      "cognitive-complexity",
			Message:   fmt.Sprintf("Cognitive complexity of %d exceeds recommended complexity of %d", fn.Metrics.Cognitive, t.CognitiveWarn),
			Value:     fn.Metrics.Cognitive,
			Threshold: t.CognitiveWarn,
			File:      file,
			Line:      fn.StartLine,
			Function:  fn.Name,
		})
	}
}

// checkFunctionHotspots identifies complexity hotspots.
func checkFunctionHotspots(fn *FunctionComplexity, file string, t ExtendedComplexityThresholds, hotspots *[]ComplexityHotspot) {
	if fn.Metrics.Cyclomatic > t.CyclomaticWarn {
		*hotspots = append(*hotspots, ComplexityHotspot{
			File:           file,
			Function:       fn.Name,
			Line:           fn.StartLine,
			Complexity:     fn.Metrics.Cyclomatic,
			ComplexityType: "cyclomatic",
		})
	}
}

// calculateTechnicalDebt estimates hours to fix violations.
// Errors: 30 minutes per point over threshold
// Warnings: 15 minutes per point over threshold
func calculateTechnicalDebt(violations []Violation) float32 {
	var debtMinutes float32

	for _, v := range violations {
		if v.Value > v.Threshold {
			pointsOver := float32(v.Value - v.Threshold)
			if v.Severity == SeverityError {
				debtMinutes += pointsOver * 30.0
			} else {
				debtMinutes += pointsOver * 15.0
			}
		}
	}

	return debtMinutes / 60.0
}

// calculateMedian returns the median of a sorted slice.
func calculateMedian(values []uint32) float32 {
	if len(values) == 0 {
		return 0
	}

	mid := len(values) / 2
	if len(values)%2 == 0 {
		return float32(values[mid-1]+values[mid]) / 2.0
	}
	return float32(values[mid])
}

// maxValue returns the maximum value in a slice.
func maxValue(values []uint32) uint32 {
	if len(values) == 0 {
		return 0
	}

	max := values[0]
	for _, v := range values[1:] {
		if v > max {
			max = v
		}
	}
	return max
}

// percentileU32 calculates the p-th percentile of a sorted slice.
func percentileU32(sorted []uint32, p int) uint32 {
	if len(sorted) == 0 {
		return 0
	}

	idx := (p * len(sorted)) / 100
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}

// ErrorCount returns the number of error-severity violations.
func (r *ComplexityReport) ErrorCount() int {
	count := 0
	for _, v := range r.Violations {
		if v.Severity == SeverityError {
			count++
		}
	}
	return count
}

// WarningCount returns the number of warning-severity violations.
func (r *ComplexityReport) WarningCount() int {
	count := 0
	for _, v := range r.Violations {
		if v.Severity == SeverityWarning {
			count++
		}
	}
	return count
}
