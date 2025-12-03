package score

import "testing"

func TestGradeFromScore(t *testing.T) {
	tests := []struct {
		score int
		want  Grade
	}{
		{100, GradeAPlus},
		{95, GradeAPlus},
		{94, GradeA},
		{90, GradeA},
		{85, GradeAMinus},
		{80, GradeBPlus},
		{75, GradeB},
		{70, GradeBMinus},
		{65, GradeCPlus},
		{60, GradeC},
		{55, GradeCMinus},
		{50, GradeD},
		{49, GradeF},
		{0, GradeF},
	}
	for _, tt := range tests {
		got := GradeFromScore(tt.score)
		if got != tt.want {
			t.Errorf("GradeFromScore(%d) = %s, want %s", tt.score, got, tt.want)
		}
	}
}

func TestDefaultWeights_SumToOne(t *testing.T) {
	w := DefaultWeights()
	sum := w.Complexity + w.Duplication + w.Defect + w.Debt + w.Coupling + w.Smells
	if sum < 0.99 || sum > 1.01 {
		t.Errorf("weights sum to %f, want 1.0", sum)
	}
}

func TestResult_ComputeComposite(t *testing.T) {
	r := &Result{
		Components: ComponentScores{
			Complexity:  80,
			Duplication: 90,
			Defect:      70,
			Debt:        60,
			Coupling:    85,
			Smells:      95,
		},
		Weights: DefaultWeights(),
	}
	r.ComputeComposite()

	// Expected: 80*0.25 + 90*0.20 + 70*0.25 + 60*0.15 + 85*0.10 + 95*0.05
	// = 20 + 18 + 17.5 + 9 + 8.5 + 4.75 = 77.75 -> 78
	if r.Score < 77 || r.Score > 79 {
		t.Errorf("ComputeComposite() = %d, want ~78", r.Score)
	}
	if r.Grade != string(GradeB) {
		t.Errorf("Grade = %s, want B", r.Grade)
	}
}

func TestResult_CheckThresholds(t *testing.T) {
	r := &Result{
		Score: 75,
		Components: ComponentScores{
			Complexity:  80,
			Duplication: 60,
			Defect:      70,
			Debt:        65,
			Coupling:    85,
			Smells:      90,
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
