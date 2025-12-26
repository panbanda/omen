package score

import (
	"testing"
)

func TestAnalyzer_New(t *testing.T) {
	a := New()
	if a == nil || a.weights.Complexity != 0.25 {
		t.Fatalf("New() returned nil or has wrong defaults (want complexity weight 0.25)")
	}
}

func TestAnalyzer_WithWeights(t *testing.T) {
	custom := Weights{
		Complexity:  0.30,
		Duplication: 0.25,
		SATD:        0.15,
		TDG:         0.10,
		Coupling:    0.10,
		Smells:      0.10,
	}
	a := New(WithWeights(custom))
	if a.weights.Complexity != 0.30 {
		t.Errorf("custom complexity weight = %f, want 0.30", a.weights.Complexity)
	}
}

func TestAnalyzer_WithThresholds(t *testing.T) {
	th := Thresholds{Score: 70, Complexity: 80}
	a := New(WithThresholds(th))
	if a.thresholds.Score != 70 {
		t.Errorf("threshold score = %d, want 70", a.thresholds.Score)
	}
}

func TestAnalyzer_WithChurnDays(t *testing.T) {
	a := New(WithChurnDays(60))
	if a.churnDays != 60 {
		t.Errorf("churnDays = %d, want 60", a.churnDays)
	}
}
