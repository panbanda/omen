package score

import (
	"testing"
)

func TestNormalizeComplexity(t *testing.T) {
	tests := []struct {
		name               string
		totalFunctions     int
		violatingFunctions int
		want               int
	}{
		{"no violations", 100, 0, 100},
		{"10% violations", 100, 10, 90},
		{"50% violations", 100, 50, 50},
		{"all violations", 100, 100, 0},
		{"no functions", 0, 0, 100},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := NormalizeComplexity(tt.totalFunctions, tt.violatingFunctions)
			if got != tt.want {
				t.Errorf("NormalizeComplexity(%d, %d) = %d, want %d",
					tt.totalFunctions, tt.violatingFunctions, got, tt.want)
			}
		})
	}
}

func TestNormalizeDuplication_NonLinear(t *testing.T) {
	tests := []struct {
		name  string
		ratio float64
		min   int // Expected score should be >= min
		max   int // Expected score should be <= max
	}{
		// 0-3%: excellent (100-95)
		{"0% duplication", 0.00, 100, 100},
		{"1% duplication", 0.01, 98, 100},
		{"3% duplication", 0.03, 94, 96},

		// 3-5%: good (95-90)
		{"4% duplication", 0.04, 91, 94},
		{"5% duplication", 0.05, 89, 91},

		// 5-10%: fair (90-80)
		{"7% duplication", 0.07, 84, 88},
		{"10% duplication", 0.10, 79, 81},

		// 10-20%: poor (80-60)
		{"14% duplication", 0.14, 70, 75},
		{"20% duplication", 0.20, 59, 61},

		// >20%: critical (60-0)
		{"30% duplication", 0.30, 40, 50},
		{"50% duplication", 0.50, 10, 20},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := NormalizeDuplication(tt.ratio)
			if got < tt.min || got > tt.max {
				t.Errorf("NormalizeDuplication(%f) = %d, want %d-%d",
					tt.ratio, got, tt.min, tt.max)
			}
		})
	}
}

func TestNormalizeDuplication_Monotonic(t *testing.T) {
	// Scores should decrease as duplication increases
	prev := 101
	for ratio := 0.0; ratio <= 1.0; ratio += 0.05 {
		score := NormalizeDuplication(ratio)
		if score > prev {
			t.Errorf("score increased from %d to %d at ratio %f", prev, score, ratio)
		}
		prev = score
	}
}

func TestNormalizeDefect_PowerCurve(t *testing.T) {
	tests := []struct {
		name           string
		avgProbability float32
		min            int
		max            int
	}{
		{"0% probability", 0.0, 100, 100},
		{"1% probability", 0.01, 89, 91},  // sqrt(0.01) = 0.1, score ~90
		{"10% probability", 0.10, 67, 69}, // sqrt(0.1) = 0.316, score ~68
		{"25% probability", 0.25, 49, 51}, // sqrt(0.25) = 0.5, score ~50
		{"50% probability", 0.50, 28, 30}, // sqrt(0.5) = 0.707, score ~29
		{"100% probability", 1.0, 0, 0},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := NormalizeDefect(tt.avgProbability)
			if got < tt.min || got > tt.max {
				t.Errorf("NormalizeDefect(%f) = %d, want %d-%d",
					tt.avgProbability, got, tt.min, tt.max)
			}
		})
	}
}

func TestNormalizeDebt_SeverityWeighted(t *testing.T) {
	tests := []struct {
		name   string
		counts DebtSeverityCounts
		loc    int
		min    int
		max    int
	}{
		{
			name:   "no debt",
			counts: DebtSeverityCounts{0, 0, 0, 0},
			loc:    1000,
			min:    100,
			max:    100,
		},
		{
			name:   "only low severity",
			counts: DebtSeverityCounts{0, 0, 0, 40}, // 40 low * 0.25 = 10 weighted
			loc:    1000,
			min:    78, // 100 - 10*2 = 80, allow margin
			max:    82,
		},
		{
			name:   "only medium severity",
			counts: DebtSeverityCounts{0, 0, 10, 0}, // 10 medium * 1.0 = 10 weighted
			loc:    1000,
			min:    78,
			max:    82,
		},
		{
			name:   "only high severity",
			counts: DebtSeverityCounts{0, 5, 0, 0}, // 5 high * 2.0 = 10 weighted
			loc:    1000,
			min:    78,
			max:    82,
		},
		{
			name:   "critical items",
			counts: DebtSeverityCounts{3, 0, 0, 0}, // 3 critical * 4.0 = 12 weighted
			loc:    1000,
			min:    74,
			max:    78,
		},
		{
			name:   "mixed severity",
			counts: DebtSeverityCounts{2, 3, 5, 10}, // 2*4 + 3*2 + 5*1 + 10*0.25 = 8+6+5+2.5 = 21.5
			loc:    1000,
			min:    55, // 100 - 21.5*2 = ~57
			max:    59,
		},
		{
			name:   "no LOC returns 100",
			counts: DebtSeverityCounts{10, 10, 10, 10},
			loc:    0,
			min:    100,
			max:    100,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := NormalizeDebt(tt.counts, tt.loc)
			if got < tt.min || got > tt.max {
				t.Errorf("NormalizeDebt(%+v, %d) = %d, want %d-%d",
					tt.counts, tt.loc, got, tt.min, tt.max)
			}
		})
	}
}

func TestNormalizeCoupling_MultiSignal(t *testing.T) {
	tests := []struct {
		name    string
		metrics CouplingMetrics
		min     int
		max     int
	}{
		{
			name:    "no components returns neutral",
			metrics: CouplingMetrics{0, 0, 0.0, 0},
			min:     75,
			max:     75,
		},
		{
			name:    "perfect coupling",
			metrics: CouplingMetrics{0, 0, 0.0, 10},
			min:     100,
			max:     100,
		},
		{
			name:    "moderate instability",
			metrics: CouplingMetrics{0, 0, 0.5, 10},
			min:     74, // 100 - 0.5*50 = 75
			max:     76,
		},
		{
			name:    "one cycle in 100 components",
			metrics: CouplingMetrics{1, 0, 0.0, 100},
			// cycleDensity = 1/100*100 = 1%, deduction = 1*10 = 10
			min: 89,
			max: 91,
		},
		{
			name:    "one cycle in 10 components (higher density)",
			metrics: CouplingMetrics{1, 0, 0.0, 10},
			// cycleDensity = 1/10*100 = 10%, deduction = 10*10 = 100 capped at 50
			min: 49,
			max: 51,
		},
		{
			name:    "SDP violations",
			metrics: CouplingMetrics{0, 10, 0.0, 100},
			// sdpDensity = 10/100*100 = 10%, deduction = 10*2 = 20
			min: 79,
			max: 81,
		},
		{
			name:    "multiple issues large codebase",
			metrics: CouplingMetrics{5, 20, 0.3, 500},
			// baseScore = 100 - 0.3*50 = 85
			// cycleDensity = 5/500*100 = 1%, deduction = 1*10 = 10
			// sdpDensity = 20/500*100 = 4%, deduction = 4*2 = 8
			// score = 85 - 10 - 8 = 67
			min: 66,
			max: 68,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := NormalizeCoupling(tt.metrics)
			if got < tt.min || got > tt.max {
				t.Errorf("NormalizeCoupling(%+v) = %d, want %d-%d",
					tt.metrics, got, tt.min, tt.max)
			}
		})
	}
}

func TestNormalizeSmells_ScaledBySize(t *testing.T) {
	tests := []struct {
		name            string
		counts          SmellCounts
		totalComponents int
		min             int
		max             int
	}{
		{
			name:            "no smells",
			counts:          SmellCounts{0, 0, 0},
			totalComponents: 10,
			min:             100,
			max:             100,
		},
		{
			name:            "no components returns 100",
			counts:          SmellCounts{5, 5, 5},
			totalComponents: 0,
			min:             100,
			max:             100,
		},
		{
			name:            "one critical in 10 components",
			counts:          SmellCounts{1, 0, 0},
			totalComponents: 10,
			// weighted = 1*3 = 3, density per 10 = 3/10*10 = 3, score = 100 - 3*10 = 70
			min: 69,
			max: 71,
		},
		{
			name:            "one critical in 100 components",
			counts:          SmellCounts{1, 0, 0},
			totalComponents: 100,
			min:             96, // density = 3/100*10 = 0.3, score = 100-3 = 97
			max:             100,
		},
		{
			name:            "multiple smells relative to size",
			counts:          SmellCounts{2, 5, 10},
			totalComponents: 100,
			// weighted = 2*3 + 5*2 + 10*1 = 6+10+10 = 26
			// density = 26/100*10 = 2.6
			// score = 100 - 26 = 74
			min: 72,
			max: 78,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := NormalizeSmells(tt.counts, tt.totalComponents)
			if got < tt.min || got > tt.max {
				t.Errorf("NormalizeSmells(%+v, %d) = %d, want %d-%d",
					tt.counts, tt.totalComponents, got, tt.min, tt.max)
			}
		})
	}
}

func TestNormalizeCohesion(t *testing.T) {
	// LCOM4 = 1 means perfect cohesion, higher values indicate class could be split
	tests := []struct {
		name    string
		avgLCOM float64
		min     int
		max     int
	}{
		{"below 1 (edge case)", 0.5, 100, 100}, // treated as perfect
		{"perfect cohesion LCOM4=1", 1.0, 100, 100},
		{"slightly fragmented LCOM4=2", 2.0, 88, 90},  // 100 - (2-1)*11.1 = 88.9
		{"fragmented LCOM4=5", 5.0, 55, 57},           // 100 - (5-1)*11.1 = 55.6
		{"very fragmented LCOM4=10", 10.0, 0, 2},      // 100 - (10-1)*11.1 = 0.1
		{"extremely fragmented LCOM4=20", 20.0, 0, 0}, // clamped to 0
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := NormalizeCohesion(tt.avgLCOM)
			if got < tt.min || got > tt.max {
				t.Errorf("NormalizeCohesion(%f) = %d, want %d-%d",
					tt.avgLCOM, got, tt.min, tt.max)
			}
		})
	}
}

func TestClamp(t *testing.T) {
	tests := []struct {
		value, min, max int
		want            int
	}{
		{50, 0, 100, 50},
		{-10, 0, 100, 0},
		{150, 0, 100, 100},
		{0, 0, 100, 0},
		{100, 0, 100, 100},
	}
	for _, tt := range tests {
		got := clamp(tt.value, tt.min, tt.max)
		if got != tt.want {
			t.Errorf("clamp(%d, %d, %d) = %d, want %d",
				tt.value, tt.min, tt.max, got, tt.want)
		}
	}
}
