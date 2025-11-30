package models

import "testing"

func TestDefaultSmellThresholds(t *testing.T) {
	thresholds := DefaultSmellThresholds()

	if thresholds.HubThreshold != 20 {
		t.Errorf("Expected hub threshold 20, got %d", thresholds.HubThreshold)
	}
	if thresholds.GodFanInThreshold != 10 {
		t.Errorf("Expected god fan-in threshold 10, got %d", thresholds.GodFanInThreshold)
	}
	if thresholds.GodFanOutThreshold != 10 {
		t.Errorf("Expected god fan-out threshold 10, got %d", thresholds.GodFanOutThreshold)
	}
	if thresholds.InstabilityDifference != 0.4 {
		t.Errorf("Expected instability difference 0.4, got %f", thresholds.InstabilityDifference)
	}
}

func TestNewSmellAnalysis(t *testing.T) {
	analysis := NewSmellAnalysis()

	if analysis.Smells == nil {
		t.Error("Smells should be initialized")
	}
	if analysis.Components == nil {
		t.Error("Components should be initialized")
	}
	if analysis.GeneratedAt.IsZero() {
		t.Error("GeneratedAt should be set")
	}
}

func TestSmellAnalysis_AddSmell(t *testing.T) {
	analysis := NewSmellAnalysis()

	// Add cyclic smell
	analysis.AddSmell(ArchitecturalSmell{
		Type:     SmellCyclicDependency,
		Severity: SmellSeverityCritical,
	})

	if analysis.Summary.TotalSmells != 1 {
		t.Errorf("Expected 1 total smell, got %d", analysis.Summary.TotalSmells)
	}
	if analysis.Summary.CyclicCount != 1 {
		t.Errorf("Expected 1 cyclic count, got %d", analysis.Summary.CyclicCount)
	}
	if analysis.Summary.CriticalCount != 1 {
		t.Errorf("Expected 1 critical count, got %d", analysis.Summary.CriticalCount)
	}

	// Add hub smell
	analysis.AddSmell(ArchitecturalSmell{
		Type:     SmellHubLikeDependency,
		Severity: SmellSeverityHigh,
	})

	if analysis.Summary.HubCount != 1 {
		t.Errorf("Expected 1 hub count, got %d", analysis.Summary.HubCount)
	}
	if analysis.Summary.HighCount != 1 {
		t.Errorf("Expected 1 high count, got %d", analysis.Summary.HighCount)
	}

	// Add unstable smell
	analysis.AddSmell(ArchitecturalSmell{
		Type:     SmellUnstableDependency,
		Severity: SmellSeverityMedium,
	})

	if analysis.Summary.UnstableCount != 1 {
		t.Errorf("Expected 1 unstable count, got %d", analysis.Summary.UnstableCount)
	}
	if analysis.Summary.MediumCount != 1 {
		t.Errorf("Expected 1 medium count, got %d", analysis.Summary.MediumCount)
	}

	// Add god smell
	analysis.AddSmell(ArchitecturalSmell{
		Type:     SmellGodComponent,
		Severity: SmellSeverityCritical,
	})

	if analysis.Summary.GodCount != 1 {
		t.Errorf("Expected 1 god count, got %d", analysis.Summary.GodCount)
	}

	if analysis.Summary.TotalSmells != 4 {
		t.Errorf("Expected 4 total smells, got %d", analysis.Summary.TotalSmells)
	}
}

func TestSmellAnalysis_CalculateSummary(t *testing.T) {
	analysis := NewSmellAnalysis()

	analysis.Components = []ComponentMetrics{
		{ID: "A", Instability: 0.2},
		{ID: "B", Instability: 0.5},
		{ID: "C", Instability: 0.8},
	}

	analysis.CalculateSummary()

	if analysis.Summary.TotalComponents != 3 {
		t.Errorf("Expected 3 components, got %d", analysis.Summary.TotalComponents)
	}

	expectedAvg := 0.5 // (0.2 + 0.5 + 0.8) / 3
	if analysis.Summary.AverageInstability != expectedAvg {
		t.Errorf("Expected average instability %f, got %f", expectedAvg, analysis.Summary.AverageInstability)
	}
}

func TestSmellAnalysis_CalculateSummary_Empty(t *testing.T) {
	analysis := NewSmellAnalysis()
	analysis.CalculateSummary()

	if analysis.Summary.TotalComponents != 0 {
		t.Errorf("Expected 0 components, got %d", analysis.Summary.TotalComponents)
	}
}

func TestComponentMetrics_CalculateInstability(t *testing.T) {
	tests := []struct {
		name     string
		fanIn    int
		fanOut   int
		expected float64
	}{
		{"stable (all incoming)", 10, 0, 0.0},
		{"unstable (all outgoing)", 0, 10, 1.0},
		{"balanced", 5, 5, 0.5},
		{"no connections", 0, 0, 0.0},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cm := ComponentMetrics{
				FanIn:  tt.fanIn,
				FanOut: tt.fanOut,
			}
			result := cm.CalculateInstability()

			if result != tt.expected {
				t.Errorf("Expected %f, got %f", tt.expected, result)
			}
		})
	}
}

func TestSmellTypes(t *testing.T) {
	// Test that constants have expected values
	if SmellCyclicDependency != "cyclic_dependency" {
		t.Errorf("Unexpected cyclic dependency type: %s", SmellCyclicDependency)
	}
	if SmellHubLikeDependency != "hub_like_dependency" {
		t.Errorf("Unexpected hub type: %s", SmellHubLikeDependency)
	}
	if SmellUnstableDependency != "unstable_dependency" {
		t.Errorf("Unexpected unstable type: %s", SmellUnstableDependency)
	}
	if SmellGodComponent != "god_component" {
		t.Errorf("Unexpected god type: %s", SmellGodComponent)
	}
}

func TestSmellSeverities(t *testing.T) {
	if SmellSeverityCritical != "critical" {
		t.Errorf("Unexpected critical severity: %s", SmellSeverityCritical)
	}
	if SmellSeverityHigh != "high" {
		t.Errorf("Unexpected high severity: %s", SmellSeverityHigh)
	}
	if SmellSeverityMedium != "medium" {
		t.Errorf("Unexpected medium severity: %s", SmellSeverityMedium)
	}
}
