package score

import "gonum.org/v1/gonum/stat"

// TrendStats holds regression statistics computed from trend data points.
type TrendStats struct {
	Slope       float64 // Score change per period
	Intercept   float64 // Y-intercept
	RSquared    float64 // Goodness of fit (0-1)
	Correlation float64 // Pearson correlation (-1 to 1)
}

// ComputeTrendStats calculates regression statistics from data points.
// Returns zero values if fewer than 2 points are provided.
func ComputeTrendStats(points []TrendPoint) TrendStats {
	n := len(points)
	if n < 2 {
		return TrendStats{}
	}

	xs := make([]float64, n) // time index (0, 1, 2, ...)
	ys := make([]float64, n) // scores

	for i, p := range points {
		xs[i] = float64(i)
		ys[i] = float64(p.Score)
	}

	return computeStats(xs, ys)
}

// ComputeComponentTrends calculates regression statistics for each component.
func ComputeComponentTrends(points []TrendPoint) ComponentTrends {
	n := len(points)
	if n < 2 {
		return ComponentTrends{}
	}

	xs := make([]float64, n)
	complexity := make([]float64, n)
	duplication := make([]float64, n)
	defect := make([]float64, n)
	debt := make([]float64, n)
	coupling := make([]float64, n)
	smells := make([]float64, n)
	cohesion := make([]float64, n)

	for i, p := range points {
		xs[i] = float64(i)
		complexity[i] = float64(p.Components.Complexity)
		duplication[i] = float64(p.Components.Duplication)
		defect[i] = float64(p.Components.Defect)
		debt[i] = float64(p.Components.Debt)
		coupling[i] = float64(p.Components.Coupling)
		smells[i] = float64(p.Components.Smells)
		cohesion[i] = float64(p.Components.Cohesion)
	}

	return ComponentTrends{
		Complexity:  computeStats(xs, complexity),
		Duplication: computeStats(xs, duplication),
		Defect:      computeStats(xs, defect),
		Debt:        computeStats(xs, debt),
		Coupling:    computeStats(xs, coupling),
		Smells:      computeStats(xs, smells),
		Cohesion:    computeStats(xs, cohesion),
	}
}

// computeStats calculates regression statistics from x and y values.
func computeStats(xs, ys []float64) TrendStats {
	intercept, slope := stat.LinearRegression(xs, ys, nil, false)
	rSquared := stat.RSquared(xs, ys, nil, intercept, slope)
	correlation := stat.Correlation(xs, ys, nil)

	return TrendStats{
		Slope:       slope,
		Intercept:   intercept,
		RSquared:    rSquared,
		Correlation: correlation,
	}
}
