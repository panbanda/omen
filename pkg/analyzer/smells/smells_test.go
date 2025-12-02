package smells

import (
	"testing"

	"github.com/panbanda/omen/pkg/analyzer/graph"
)

func TestSmellAnalyzer_DetectCyclicDependencies(t *testing.T) {
	g := graph.NewDependencyGraph()

	// Create a cycle: A -> B -> C -> A
	g.AddNode(graph.Node{ID: "A", Name: "A"})
	g.AddNode(graph.Node{ID: "B", Name: "B"})
	g.AddNode(graph.Node{ID: "C", Name: "C"})

	g.AddEdge(graph.Edge{From: "A", To: "B"})
	g.AddEdge(graph.Edge{From: "B", To: "C"})
	g.AddEdge(graph.Edge{From: "C", To: "A"})

	analyzer := New()
	analysis := analyzer.AnalyzeGraph(g)

	cyclicSmells := 0
	for _, smell := range analysis.Smells {
		if smell.Type == TypeCyclicDependency {
			cyclicSmells++
			if len(smell.Components) != 3 {
				t.Errorf("Expected 3 components in cycle, got %d", len(smell.Components))
			}
			if smell.Severity != SeverityCritical {
				t.Errorf("Expected critical severity for cycle, got %s", smell.Severity)
			}
		}
	}

	if cyclicSmells != 1 {
		t.Errorf("Expected 1 cyclic dependency smell, got %d", cyclicSmells)
	}
}

func TestSmellAnalyzer_NoCycles(t *testing.T) {
	g := graph.NewDependencyGraph()

	// Create acyclic graph: A -> B -> C
	g.AddNode(graph.Node{ID: "A", Name: "A"})
	g.AddNode(graph.Node{ID: "B", Name: "B"})
	g.AddNode(graph.Node{ID: "C", Name: "C"})

	g.AddEdge(graph.Edge{From: "A", To: "B"})
	g.AddEdge(graph.Edge{From: "B", To: "C"})

	analyzer := New()
	analysis := analyzer.AnalyzeGraph(g)

	for _, smell := range analysis.Smells {
		if smell.Type == TypeCyclicDependency {
			t.Error("Should not detect cyclic dependency in acyclic graph")
		}
	}
}

func TestSmellAnalyzer_DetectHubDependencies(t *testing.T) {
	g := graph.NewDependencyGraph()

	// Create hub: H has many connections but NOT both high fan-in AND high fan-out
	// (otherwise it would be a god component)
	// Hub threshold = 20, God thresholds = 10/10
	// We want fan-in + fan-out > 20, but neither > 10
	g.AddNode(graph.Node{ID: "H", Name: "Hub"})

	// Add 12 incoming edges (fan-in = 12, but we need to avoid god detection)
	// Actually, let's make fan-in=18, fan-out=5 (total=23>20, fan-out<10 so not god)
	for i := 0; i < 18; i++ {
		id := string(rune('A' + i))
		g.AddNode(graph.Node{ID: id, Name: id})
		g.AddEdge(graph.Edge{From: id, To: "H"})
	}

	// Add 5 outgoing edges (fan-out = 5, below god threshold of 10)
	for i := 0; i < 5; i++ {
		id := string(rune('a' + i))
		g.AddNode(graph.Node{ID: id, Name: id})
		g.AddEdge(graph.Edge{From: "H", To: id})
	}

	// Use threshold of 20, total connections = 23
	// God thresholds = 10/10, fan-out=5 < 10 so not god
	analyzer := New(WithHubThreshold(20), WithGodThresholds(10, 10))
	analysis := analyzer.AnalyzeGraph(g)

	found := false
	for _, smell := range analysis.Smells {
		if smell.Type == TypeHubLikeDependency {
			found = true
			if smell.Components[0] != "H" {
				t.Errorf("Expected hub to be H, got %s", smell.Components[0])
			}
		}
	}

	if !found {
		t.Error("Should detect hub-like dependency")
	}
}

func TestSmellAnalyzer_DetectGodComponent(t *testing.T) {
	g := graph.NewDependencyGraph()

	// Create god component: G has both high fan-in and high fan-out
	g.AddNode(graph.Node{ID: "G", Name: "God"})

	// Add 12 incoming edges (fan-in > 10)
	for i := 0; i < 12; i++ {
		id := string(rune('A' + i))
		g.AddNode(graph.Node{ID: id, Name: id})
		g.AddEdge(graph.Edge{From: id, To: "G"})
	}

	// Add 12 outgoing edges (fan-out > 10)
	for i := 0; i < 12; i++ {
		id := string(rune('a' + i))
		g.AddNode(graph.Node{ID: id, Name: id})
		g.AddEdge(graph.Edge{From: "G", To: id})
	}

	analyzer := New(WithGodThresholds(10, 10))
	analysis := analyzer.AnalyzeGraph(g)

	godFound := false
	for _, smell := range analysis.Smells {
		if smell.Type == TypeGodComponent {
			godFound = true
			if smell.Severity != SeverityCritical {
				t.Errorf("Expected critical severity for god component, got %s", smell.Severity)
			}
		}
	}

	if !godFound {
		t.Error("Should detect god component")
	}
}

func TestSmellAnalyzer_DetectUnstableDependency(t *testing.T) {
	g := graph.NewDependencyGraph()

	// Stable component: mostly incoming, I < 0.3
	// fan-in = 10, fan-out = 1, I = 1/11 = 0.09 (stable)
	g.AddNode(graph.Node{ID: "Stable", Name: "Stable"})
	for i := 0; i < 10; i++ {
		id := string(rune('A' + i))
		g.AddNode(graph.Node{ID: id, Name: id})
		g.AddEdge(graph.Edge{From: id, To: "Stable"})
	}

	// Unstable component: all outgoing, I > 0.7
	// fan-in = 1, fan-out = 10, I = 10/11 = 0.91 (unstable)
	g.AddNode(graph.Node{ID: "Unstable", Name: "Unstable"})
	for i := 0; i < 10; i++ {
		id := string(rune('a' + i))
		g.AddNode(graph.Node{ID: id, Name: id})
		g.AddEdge(graph.Edge{From: "Unstable", To: id})
	}

	// Stable depends on Unstable - this is bad!
	// This adds 1 to Stable's fan-out and 1 to Unstable's fan-in
	// Stable: fan-in=10, fan-out=1, I = 1/11 = 0.09 < 0.3 (stable)
	// Unstable: fan-in=1, fan-out=10, I = 10/11 = 0.91 > 0.7 (unstable)
	g.AddEdge(graph.Edge{From: "Stable", To: "Unstable"})

	analyzer := New()
	analysis := analyzer.AnalyzeGraph(g)

	found := false
	for _, smell := range analysis.Smells {
		if smell.Type == TypeUnstableDependency {
			found = true
			if len(smell.Components) != 2 {
				t.Errorf("Expected 2 components, got %d", len(smell.Components))
			}
		}
	}

	if !found {
		t.Error("Should detect unstable dependency")
	}
}

func TestSmellAnalyzer_ComponentMetrics(t *testing.T) {
	g := graph.NewDependencyGraph()

	// Create simple graph
	g.AddNode(graph.Node{ID: "A", Name: "A"})
	g.AddNode(graph.Node{ID: "B", Name: "B"})
	g.AddNode(graph.Node{ID: "C", Name: "C"})

	// A -> B, A -> C (A has fan-out=2, fan-in=0, I=1)
	// B has fan-in=1, fan-out=0, I=0
	// C has fan-in=1, fan-out=0, I=0
	g.AddEdge(graph.Edge{From: "A", To: "B"})
	g.AddEdge(graph.Edge{From: "A", To: "C"})

	analyzer := New()
	analysis := analyzer.AnalyzeGraph(g)

	if len(analysis.Components) != 3 {
		t.Fatalf("Expected 3 components, got %d", len(analysis.Components))
	}

	componentMap := make(map[string]ComponentMetrics)
	for _, c := range analysis.Components {
		componentMap[c.ID] = c
	}

	// Check A metrics
	if componentMap["A"].FanIn != 0 {
		t.Errorf("A fan-in: expected 0, got %d", componentMap["A"].FanIn)
	}
	if componentMap["A"].FanOut != 2 {
		t.Errorf("A fan-out: expected 2, got %d", componentMap["A"].FanOut)
	}
	if componentMap["A"].Instability != 1.0 {
		t.Errorf("A instability: expected 1.0, got %f", componentMap["A"].Instability)
	}

	// Check B metrics
	if componentMap["B"].FanIn != 1 {
		t.Errorf("B fan-in: expected 1, got %d", componentMap["B"].FanIn)
	}
	if componentMap["B"].FanOut != 0 {
		t.Errorf("B fan-out: expected 0, got %d", componentMap["B"].FanOut)
	}
	if componentMap["B"].Instability != 0.0 {
		t.Errorf("B instability: expected 0.0, got %f", componentMap["B"].Instability)
	}
}

func TestSmellAnalyzer_EmptyGraph(t *testing.T) {
	g := graph.NewDependencyGraph()

	analyzer := New()
	analysis := analyzer.AnalyzeGraph(g)

	if len(analysis.Smells) != 0 {
		t.Errorf("Expected 0 smells for empty graph, got %d", len(analysis.Smells))
	}
	if len(analysis.Components) != 0 {
		t.Errorf("Expected 0 components for empty graph, got %d", len(analysis.Components))
	}
}

func TestSmellAnalyzer_Summary(t *testing.T) {
	g := graph.NewDependencyGraph()

	// Create cycle
	g.AddNode(graph.Node{ID: "A", Name: "A"})
	g.AddNode(graph.Node{ID: "B", Name: "B"})
	g.AddEdge(graph.Edge{From: "A", To: "B"})
	g.AddEdge(graph.Edge{From: "B", To: "A"})

	analyzer := New()
	analysis := analyzer.AnalyzeGraph(g)

	if analysis.Summary.TotalSmells != len(analysis.Smells) {
		t.Errorf("Summary total smells mismatch: %d vs %d",
			analysis.Summary.TotalSmells, len(analysis.Smells))
	}

	if analysis.Summary.CyclicCount < 1 {
		t.Error("Expected at least 1 cyclic smell in summary")
	}

	if analysis.Summary.TotalComponents != 2 {
		t.Errorf("Expected 2 components in summary, got %d", analysis.Summary.TotalComponents)
	}
}

func TestSmellAnalyzer_CustomThresholds(t *testing.T) {
	thresholds := Thresholds{
		HubThreshold:          5,
		GodFanInThreshold:     3,
		GodFanOutThreshold:    3,
		InstabilityDifference: 0.5,
		StableThreshold:       0.2,
		UnstableThreshold:     0.8,
	}

	analyzer := New(WithThresholds(thresholds))

	if analyzer.thresholds.HubThreshold != 5 {
		t.Errorf("Expected hub threshold 5, got %d", analyzer.thresholds.HubThreshold)
	}
	if analyzer.thresholds.GodFanInThreshold != 3 {
		t.Errorf("Expected god fan-in threshold 3, got %d", analyzer.thresholds.GodFanInThreshold)
	}
}

func TestComponentMetrics_CalculateInstability(t *testing.T) {
	tests := []struct {
		name     string
		fanIn    int
		fanOut   int
		expected float64
	}{
		{"all incoming (stable)", 10, 0, 0.0},
		{"all outgoing (unstable)", 0, 10, 1.0},
		{"balanced", 5, 5, 0.5},
		{"no connections", 0, 0, 0.0},
		{"mostly stable", 8, 2, 0.2},
		{"mostly unstable", 2, 8, 0.8},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cm := ComponentMetrics{
				FanIn:  tt.fanIn,
				FanOut: tt.fanOut,
			}
			result := cm.CalculateInstability()

			if result != tt.expected {
				t.Errorf("Expected instability %f, got %f", tt.expected, result)
			}
			if cm.Instability != tt.expected {
				t.Errorf("Expected stored instability %f, got %f", tt.expected, cm.Instability)
			}
		})
	}
}

func TestNew_DefaultThresholds(t *testing.T) {
	analyzer := New()
	defaults := DefaultThresholds()

	if analyzer.thresholds.HubThreshold != defaults.HubThreshold {
		t.Errorf("HubThreshold = %d, want %d", analyzer.thresholds.HubThreshold, defaults.HubThreshold)
	}
	if analyzer.thresholds.GodFanInThreshold != defaults.GodFanInThreshold {
		t.Errorf("GodFanInThreshold = %d, want %d", analyzer.thresholds.GodFanInThreshold, defaults.GodFanInThreshold)
	}
	if analyzer.thresholds.GodFanOutThreshold != defaults.GodFanOutThreshold {
		t.Errorf("GodFanOutThreshold = %d, want %d", analyzer.thresholds.GodFanOutThreshold, defaults.GodFanOutThreshold)
	}
}

func TestWithInstabilityDifference(t *testing.T) {
	analyzer := New(WithInstabilityDifference(0.6))

	if analyzer.thresholds.InstabilityDifference != 0.6 {
		t.Errorf("InstabilityDifference = %f, want 0.6", analyzer.thresholds.InstabilityDifference)
	}
}

func TestAnalyzer_Close(t *testing.T) {
	analyzer := New()
	analyzer.Close() // Should not panic
}

func TestDefaultThresholds(t *testing.T) {
	defaults := DefaultThresholds()

	if defaults.HubThreshold != 20 {
		t.Errorf("HubThreshold = %d, want 20", defaults.HubThreshold)
	}
	if defaults.GodFanInThreshold != 10 {
		t.Errorf("GodFanInThreshold = %d, want 10", defaults.GodFanInThreshold)
	}
	if defaults.GodFanOutThreshold != 10 {
		t.Errorf("GodFanOutThreshold = %d, want 10", defaults.GodFanOutThreshold)
	}
	if defaults.InstabilityDifference != 0.4 {
		t.Errorf("InstabilityDifference = %f, want 0.4", defaults.InstabilityDifference)
	}
	if defaults.StableThreshold != 0.3 {
		t.Errorf("StableThreshold = %f, want 0.3", defaults.StableThreshold)
	}
	if defaults.UnstableThreshold != 0.7 {
		t.Errorf("UnstableThreshold = %f, want 0.7", defaults.UnstableThreshold)
	}
}

func TestNewAnalysis(t *testing.T) {
	analysis := NewAnalysis()

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

func TestAnalysis_AddSmell(t *testing.T) {
	analysis := NewAnalysis()

	// Add cyclic smell
	analysis.AddSmell(Smell{
		Type:     TypeCyclicDependency,
		Severity: SeverityCritical,
	})

	if analysis.Summary.TotalSmells != 1 {
		t.Errorf("TotalSmells = %d, want 1", analysis.Summary.TotalSmells)
	}
	if analysis.Summary.CyclicCount != 1 {
		t.Errorf("CyclicCount = %d, want 1", analysis.Summary.CyclicCount)
	}
	if analysis.Summary.CriticalCount != 1 {
		t.Errorf("CriticalCount = %d, want 1", analysis.Summary.CriticalCount)
	}

	// Add hub smell
	analysis.AddSmell(Smell{
		Type:     TypeHubLikeDependency,
		Severity: SeverityHigh,
	})

	if analysis.Summary.TotalSmells != 2 {
		t.Errorf("TotalSmells = %d, want 2", analysis.Summary.TotalSmells)
	}
	if analysis.Summary.HubCount != 1 {
		t.Errorf("HubCount = %d, want 1", analysis.Summary.HubCount)
	}
	if analysis.Summary.HighCount != 1 {
		t.Errorf("HighCount = %d, want 1", analysis.Summary.HighCount)
	}

	// Add unstable smell
	analysis.AddSmell(Smell{
		Type:     TypeUnstableDependency,
		Severity: SeverityMedium,
	})

	if analysis.Summary.UnstableCount != 1 {
		t.Errorf("UnstableCount = %d, want 1", analysis.Summary.UnstableCount)
	}
	if analysis.Summary.MediumCount != 1 {
		t.Errorf("MediumCount = %d, want 1", analysis.Summary.MediumCount)
	}

	// Add god smell
	analysis.AddSmell(Smell{
		Type:     TypeGodComponent,
		Severity: SeverityCritical,
	})

	if analysis.Summary.GodCount != 1 {
		t.Errorf("GodCount = %d, want 1", analysis.Summary.GodCount)
	}
}

func TestAnalysis_CalculateSummary(t *testing.T) {
	t.Run("with components", func(t *testing.T) {
		analysis := NewAnalysis()
		analysis.Components = []ComponentMetrics{
			{ID: "A", Instability: 0.2},
			{ID: "B", Instability: 0.4},
			{ID: "C", Instability: 0.6},
		}

		analysis.CalculateSummary()

		if analysis.Summary.TotalComponents != 3 {
			t.Errorf("TotalComponents = %d, want 3", analysis.Summary.TotalComponents)
		}
		expected := 0.4 // (0.2 + 0.4 + 0.6) / 3
		diff := analysis.Summary.AverageInstability - expected
		if diff < -0.0001 || diff > 0.0001 {
			t.Errorf("AverageInstability = %f, want %f", analysis.Summary.AverageInstability, expected)
		}
	})

	t.Run("empty components", func(t *testing.T) {
		analysis := NewAnalysis()
		analysis.CalculateSummary()

		if analysis.Summary.TotalComponents != 0 {
			t.Errorf("TotalComponents = %d, want 0", analysis.Summary.TotalComponents)
		}
	})
}

func TestNew_InvalidThresholds(t *testing.T) {
	// Test that negative/zero thresholds are fixed to defaults
	analyzer := New(WithThresholds(Thresholds{
		HubThreshold:          0,
		GodFanInThreshold:     -1,
		GodFanOutThreshold:    0,
		InstabilityDifference: -0.5,
	}))

	if analyzer.thresholds.HubThreshold != 20 {
		t.Errorf("HubThreshold should be reset to default 20, got %d", analyzer.thresholds.HubThreshold)
	}
	if analyzer.thresholds.GodFanInThreshold != 10 {
		t.Errorf("GodFanInThreshold should be reset to default 10, got %d", analyzer.thresholds.GodFanInThreshold)
	}
	if analyzer.thresholds.GodFanOutThreshold != 10 {
		t.Errorf("GodFanOutThreshold should be reset to default 10, got %d", analyzer.thresholds.GodFanOutThreshold)
	}
	if analyzer.thresholds.InstabilityDifference != 0.4 {
		t.Errorf("InstabilityDifference should be reset to default 0.4, got %f", analyzer.thresholds.InstabilityDifference)
	}
}

func TestFormatComponentList(t *testing.T) {
	tests := []struct {
		name       string
		components []string
		expected   string
	}{
		{"empty", []string{}, ""},
		{"single", []string{"A"}, "A"},
		{"two", []string{"A", "B"}, "A -> B"},
		{"three", []string{"A", "B", "C"}, "A -> B -> C"},
		{"more than three", []string{"A", "B", "C", "D", "E"}, "A -> ... -> E"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := formatComponentList(tt.components)
			if result != tt.expected {
				t.Errorf("formatComponentList(%v) = %q, want %q", tt.components, result, tt.expected)
			}
		})
	}
}
