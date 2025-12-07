package score

import "testing"

func TestDefaultWeights_SumToOne(t *testing.T) {
	w := DefaultWeights()
	sum := w.Complexity + w.Duplication + w.SATD + w.TDG + w.Coupling + w.Smells + w.Cohesion
	if sum < 0.99 || sum > 1.01 {
		t.Errorf("weights sum to %f, want 1.0", sum)
	}
}

func TestResult_ComputeComposite(t *testing.T) {
	r := &Result{
		Components: ComponentScores{
			Complexity:  80,
			Duplication: 90,
			SATD:        60,
			TDG:         75,
			Coupling:    85,
			Smells:      95,
			Cohesion:    80,
		},
		Weights: DefaultWeights(),
	}
	r.ComputeComposite()

	// Expected with default weights:
	// 80*0.25 + 90*0.20 + 60*0.10 + 75*0.15 + 85*0.10 + 95*0.05 + 80*0.15
	// = 20 + 18 + 6 + 11.25 + 8.5 + 4.75 + 12 = 80.5 -> 80
	if r.Score < 78 || r.Score > 82 {
		t.Errorf("ComputeComposite() = %d, want ~80", r.Score)
	}
}

func TestResult_CheckThresholds(t *testing.T) {
	r := &Result{
		Score: 75,
		Components: ComponentScores{
			Complexity:  80,
			Duplication: 60,
			SATD:        65,
			TDG:         70,
			Coupling:    85,
			Smells:      90,
			Cohesion:    80,
		},
	}

	th := Thresholds{
		Score:       70,
		Complexity:  75,
		Duplication: 70, // This should fail (60 < 70)
	}

	r.CheckThresholds(th)

	if r.Passed {
		t.Error("expected Passed=false due to duplication threshold violation")
	}
	if r.Thresholds["duplication"].Passed {
		t.Error("expected duplication threshold to fail")
	}
	if !r.Thresholds["score"].Passed {
		t.Error("expected score threshold to pass")
	}
}
