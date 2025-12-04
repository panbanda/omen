package score

import (
	"testing"
)

func TestAnalyzer_New(t *testing.T) {
	a := New()
	if a == nil {
		t.Fatal("New() returned nil")
	}
	// Check defaults
	w := a.weights
	if w.Complexity != 0.25 {
		t.Errorf("default complexity weight = %f, want 0.25", w.Complexity)
	}
}

func TestAnalyzer_WithWeights(t *testing.T) {
	custom := Weights{
		Complexity:  0.30,
		Duplication: 0.20,
		Defect:      0.20,
		Debt:        0.15,
		Coupling:    0.10,
		Smells:      0.05,
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
