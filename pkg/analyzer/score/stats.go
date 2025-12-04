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
