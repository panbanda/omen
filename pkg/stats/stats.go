// Package stats provides statistical utility functions for analyzers.
package stats

// Percentile calculates the p-th percentile of a sorted slice.
// The slice must already be sorted in ascending order.
// Returns 0 if the slice is empty.
func Percentile(sorted []float64, p int) float64 {
	if len(sorted) == 0 {
		return 0
	}
	idx := (p * len(sorted)) / 100
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}
