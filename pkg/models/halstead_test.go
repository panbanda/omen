package models

import (
	"math"
	"testing"
)

func TestNewHalsteadMetrics(t *testing.T) {
	tests := []struct {
		name      string
		n1        uint32 // distinct operators
		n2        uint32 // distinct operands
		N1        uint32 // total operators
		N2        uint32 // total operands
		wantVoc   uint32
		wantLen   uint32
		checkVol  bool
		checkDiff bool
	}{
		{
			name:      "Basic metrics",
			n1:        5,
			n2:        10,
			N1:        20,
			N2:        40,
			wantVoc:   15,
			wantLen:   60,
			checkVol:  true,
			checkDiff: true,
		},
		{
			name:      "Zero operators",
			n1:        0,
			n2:        10,
			N1:        0,
			N2:        20,
			wantVoc:   0,
			wantLen:   0,
			checkVol:  false,
			checkDiff: false,
		},
		{
			name:      "Zero operands",
			n1:        5,
			n2:        0,
			N1:        10,
			N2:        0,
			wantVoc:   0,
			wantLen:   0,
			checkVol:  false,
			checkDiff: false,
		},
		{
			name:      "Equal counts",
			n1:        10,
			n2:        10,
			N1:        50,
			N2:        50,
			wantVoc:   20,
			wantLen:   100,
			checkVol:  true,
			checkDiff: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			h := NewHalsteadMetrics(tt.n1, tt.n2, tt.N1, tt.N2)

			if h == nil {
				t.Fatal("NewHalsteadMetrics() returned nil")
			}

			// Check base values
			if h.OperatorsUnique != tt.n1 {
				t.Errorf("OperatorsUnique = %d, want %d", h.OperatorsUnique, tt.n1)
			}
			if h.OperandsUnique != tt.n2 {
				t.Errorf("OperandsUnique = %d, want %d", h.OperandsUnique, tt.n2)
			}
			if h.OperatorsTotal != tt.N1 {
				t.Errorf("OperatorsTotal = %d, want %d", h.OperatorsTotal, tt.N1)
			}
			if h.OperandsTotal != tt.N2 {
				t.Errorf("OperandsTotal = %d, want %d", h.OperandsTotal, tt.N2)
			}

			// Check derived values
			if tt.checkVol {
				if h.Vocabulary != tt.wantVoc {
					t.Errorf("Vocabulary = %d, want %d", h.Vocabulary, tt.wantVoc)
				}
				if h.Length != tt.wantLen {
					t.Errorf("Length = %d, want %d", h.Length, tt.wantLen)
				}
				if h.Volume <= 0 {
					t.Error("Volume should be > 0")
				}
			}

			if tt.checkDiff {
				if h.Difficulty <= 0 {
					t.Error("Difficulty should be > 0")
				}
			}
		})
	}
}

func TestHalsteadMetrics_Formulas(t *testing.T) {
	// Test with known values to verify formula correctness
	n1 := uint32(4)  // distinct operators
	n2 := uint32(8)  // distinct operands
	N1 := uint32(16) // total operators
	N2 := uint32(32) // total operands

	h := NewHalsteadMetrics(n1, n2, N1, N2)

	// Vocabulary: n = n1 + n2 = 4 + 8 = 12
	expectedVoc := n1 + n2
	if h.Vocabulary != expectedVoc {
		t.Errorf("Vocabulary = %d, want %d", h.Vocabulary, expectedVoc)
	}

	// Length: N = N1 + N2 = 16 + 32 = 48
	expectedLen := N1 + N2
	if h.Length != expectedLen {
		t.Errorf("Length = %d, want %d", h.Length, expectedLen)
	}

	// Volume: V = N * log2(n) = 48 * log2(12)
	expectedVol := float64(expectedLen) * math.Log2(float64(expectedVoc))
	if diff := math.Abs(h.Volume - expectedVol); diff > 0.001 {
		t.Errorf("Volume = %f, want %f", h.Volume, expectedVol)
	}

	// Difficulty: D = (n1/2) * (N2/n2) = (4/2) * (32/8) = 2 * 4 = 8
	expectedDiff := (float64(n1) / 2.0) * (float64(N2) / float64(n2))
	if diff := math.Abs(h.Difficulty - expectedDiff); diff > 0.001 {
		t.Errorf("Difficulty = %f, want %f", h.Difficulty, expectedDiff)
	}

	// Effort: E = D * V
	expectedEffort := expectedDiff * expectedVol
	if diff := math.Abs(h.Effort - expectedEffort); diff > 0.001 {
		t.Errorf("Effort = %f, want %f", h.Effort, expectedEffort)
	}

	// Time: T = E / 18
	expectedTime := expectedEffort / 18.0
	if diff := math.Abs(h.Time - expectedTime); diff > 0.001 {
		t.Errorf("Time = %f, want %f", h.Time, expectedTime)
	}

	// Bugs: B = E^(2/3) / 3000
	expectedBugs := math.Pow(expectedEffort, 2.0/3.0) / 3000.0
	if diff := math.Abs(h.Bugs - expectedBugs); diff > 0.001 {
		t.Errorf("Bugs = %f, want %f", h.Bugs, expectedBugs)
	}
}

func TestLog2(t *testing.T) {
	tests := []struct {
		input float64
		want  float64
	}{
		{1, 0},
		{2, 1},
		{4, 2},
		{8, 3},
		{16, 4},
		{0, 0},  // Edge case
		{-1, 0}, // Edge case
	}

	for _, tt := range tests {
		t.Run("", func(t *testing.T) {
			got := log2(tt.input)
			if diff := math.Abs(got - tt.want); diff > 0.001 {
				t.Errorf("log2(%f) = %f, want %f", tt.input, got, tt.want)
			}
		})
	}
}

func TestPow(t *testing.T) {
	tests := []struct {
		x    float64
		y    float64
		want float64
	}{
		{2, 3, 8},
		{10, 2, 100},
		{4, 0.5, 2},
		{8, 1.0 / 3.0, 2},
		{0, 2, 0},  // Edge case
		{-1, 2, 0}, // Edge case (our impl returns 0)
	}

	for _, tt := range tests {
		t.Run("", func(t *testing.T) {
			got := pow(tt.x, tt.y)
			if diff := math.Abs(got - tt.want); diff > 0.01 {
				t.Errorf("pow(%f, %f) = %f, want %f", tt.x, tt.y, got, tt.want)
			}
		})
	}
}

func TestHalsteadMetrics_ZeroInputs(t *testing.T) {
	// Test that zero inputs don't cause panics or invalid values
	testCases := []struct {
		name string
		n1   uint32
		n2   uint32
		N1   uint32
		N2   uint32
	}{
		{"All zeros", 0, 0, 0, 0},
		{"Only n1", 5, 0, 10, 0},
		{"Only n2", 0, 5, 0, 10},
		{"n1 n2 only", 5, 5, 0, 0},
	}

	for _, tc := range testCases {
		t.Run(tc.name, func(t *testing.T) {
			h := NewHalsteadMetrics(tc.n1, tc.n2, tc.N1, tc.N2)
			if h == nil {
				t.Fatal("NewHalsteadMetrics() returned nil")
			}
			// Just verify no panic and no NaN/Inf
			if math.IsNaN(h.Volume) || math.IsInf(h.Volume, 0) {
				t.Error("Volume should not be NaN or Inf")
			}
			if math.IsNaN(h.Difficulty) || math.IsInf(h.Difficulty, 0) {
				t.Error("Difficulty should not be NaN or Inf")
			}
			if math.IsNaN(h.Effort) || math.IsInf(h.Effort, 0) {
				t.Error("Effort should not be NaN or Inf")
			}
		})
	}
}

func BenchmarkNewHalsteadMetrics(b *testing.B) {
	for i := 0; i < b.N; i++ {
		_ = NewHalsteadMetrics(20, 50, 100, 250)
	}
}
