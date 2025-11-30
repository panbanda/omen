package analyzer

import (
	"testing"

	"github.com/panbanda/omen/pkg/models"
)

func TestSmellAnalyzer_DetectCyclicDependencies(t *testing.T) {
	graph := models.NewDependencyGraph()

	// Create a cycle: A -> B -> C -> A
	graph.AddNode(models.GraphNode{ID: "A", Name: "A"})
	graph.AddNode(models.GraphNode{ID: "B", Name: "B"})
	graph.AddNode(models.GraphNode{ID: "C", Name: "C"})

	graph.AddEdge(models.GraphEdge{From: "A", To: "B"})
	graph.AddEdge(models.GraphEdge{From: "B", To: "C"})
	graph.AddEdge(models.GraphEdge{From: "C", To: "A"})

	analyzer := NewSmellAnalyzer()
	analysis := analyzer.AnalyzeGraph(graph)

	cyclicSmells := 0
	for _, smell := range analysis.Smells {
		if smell.Type == models.SmellCyclicDependency {
			cyclicSmells++
			if len(smell.Components) != 3 {
				t.Errorf("Expected 3 components in cycle, got %d", len(smell.Components))
			}
			if smell.Severity != models.SmellSeverityCritical {
				t.Errorf("Expected critical severity for cycle, got %s", smell.Severity)
			}
		}
	}

	if cyclicSmells != 1 {
		t.Errorf("Expected 1 cyclic dependency smell, got %d", cyclicSmells)
	}
}

func TestSmellAnalyzer_NoCycles(t *testing.T) {
	graph := models.NewDependencyGraph()

	// Create acyclic graph: A -> B -> C
	graph.AddNode(models.GraphNode{ID: "A", Name: "A"})
	graph.AddNode(models.GraphNode{ID: "B", Name: "B"})
	graph.AddNode(models.GraphNode{ID: "C", Name: "C"})

	graph.AddEdge(models.GraphEdge{From: "A", To: "B"})
	graph.AddEdge(models.GraphEdge{From: "B", To: "C"})

	analyzer := NewSmellAnalyzer()
	analysis := analyzer.AnalyzeGraph(graph)

	for _, smell := range analysis.Smells {
		if smell.Type == models.SmellCyclicDependency {
			t.Error("Should not detect cyclic dependency in acyclic graph")
		}
	}
}

func TestSmellAnalyzer_DetectHubDependencies(t *testing.T) {
	graph := models.NewDependencyGraph()

	// Create hub: H has many connections but NOT both high fan-in AND high fan-out
	// (otherwise it would be a god component)
	// Hub threshold = 20, God thresholds = 10/10
	// We want fan-in + fan-out > 20, but neither > 10
	graph.AddNode(models.GraphNode{ID: "H", Name: "Hub"})

	// Add 12 incoming edges (fan-in = 12, but we need to avoid god detection)
	// Actually, let's make fan-in=18, fan-out=5 (total=23>20, fan-out<10 so not god)
	for i := 0; i < 18; i++ {
		id := string(rune('A' + i))
		graph.AddNode(models.GraphNode{ID: id, Name: id})
		graph.AddEdge(models.GraphEdge{From: id, To: "H"})
	}

	// Add 5 outgoing edges (fan-out = 5, below god threshold of 10)
	for i := 0; i < 5; i++ {
		id := string(rune('a' + i))
		graph.AddNode(models.GraphNode{ID: id, Name: id})
		graph.AddEdge(models.GraphEdge{From: "H", To: id})
	}

	// Use threshold of 20, total connections = 23
	// God thresholds = 10/10, fan-out=5 < 10 so not god
	analyzer := NewSmellAnalyzer(WithHubThreshold(20), WithGodThresholds(10, 10))
	analysis := analyzer.AnalyzeGraph(graph)

	found := false
	for _, smell := range analysis.Smells {
		if smell.Type == models.SmellHubLikeDependency {
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
	graph := models.NewDependencyGraph()

	// Create god component: G has both high fan-in and high fan-out
	graph.AddNode(models.GraphNode{ID: "G", Name: "God"})

	// Add 12 incoming edges (fan-in > 10)
	for i := 0; i < 12; i++ {
		id := string(rune('A' + i))
		graph.AddNode(models.GraphNode{ID: id, Name: id})
		graph.AddEdge(models.GraphEdge{From: id, To: "G"})
	}

	// Add 12 outgoing edges (fan-out > 10)
	for i := 0; i < 12; i++ {
		id := string(rune('a' + i))
		graph.AddNode(models.GraphNode{ID: id, Name: id})
		graph.AddEdge(models.GraphEdge{From: "G", To: id})
	}

	analyzer := NewSmellAnalyzer(WithGodThresholds(10, 10))
	analysis := analyzer.AnalyzeGraph(graph)

	godFound := false
	for _, smell := range analysis.Smells {
		if smell.Type == models.SmellGodComponent {
			godFound = true
			if smell.Severity != models.SmellSeverityCritical {
				t.Errorf("Expected critical severity for god component, got %s", smell.Severity)
			}
		}
	}

	if !godFound {
		t.Error("Should detect god component")
	}
}

func TestSmellAnalyzer_DetectUnstableDependency(t *testing.T) {
	graph := models.NewDependencyGraph()

	// Stable component: mostly incoming, I < 0.3
	// fan-in = 10, fan-out = 1, I = 1/11 = 0.09 (stable)
	graph.AddNode(models.GraphNode{ID: "Stable", Name: "Stable"})
	for i := 0; i < 10; i++ {
		id := string(rune('A' + i))
		graph.AddNode(models.GraphNode{ID: id, Name: id})
		graph.AddEdge(models.GraphEdge{From: id, To: "Stable"})
	}

	// Unstable component: all outgoing, I > 0.7
	// fan-in = 1, fan-out = 10, I = 10/11 = 0.91 (unstable)
	graph.AddNode(models.GraphNode{ID: "Unstable", Name: "Unstable"})
	for i := 0; i < 10; i++ {
		id := string(rune('a' + i))
		graph.AddNode(models.GraphNode{ID: id, Name: id})
		graph.AddEdge(models.GraphEdge{From: "Unstable", To: id})
	}

	// Stable depends on Unstable - this is bad!
	// This adds 1 to Stable's fan-out and 1 to Unstable's fan-in
	// Stable: fan-in=10, fan-out=1, I = 1/11 = 0.09 < 0.3 (stable)
	// Unstable: fan-in=1, fan-out=10, I = 10/11 = 0.91 > 0.7 (unstable)
	graph.AddEdge(models.GraphEdge{From: "Stable", To: "Unstable"})

	analyzer := NewSmellAnalyzer()
	analysis := analyzer.AnalyzeGraph(graph)

	found := false
	for _, smell := range analysis.Smells {
		if smell.Type == models.SmellUnstableDependency {
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
	graph := models.NewDependencyGraph()

	// Create simple graph
	graph.AddNode(models.GraphNode{ID: "A", Name: "A"})
	graph.AddNode(models.GraphNode{ID: "B", Name: "B"})
	graph.AddNode(models.GraphNode{ID: "C", Name: "C"})

	// A -> B, A -> C (A has fan-out=2, fan-in=0, I=1)
	// B has fan-in=1, fan-out=0, I=0
	// C has fan-in=1, fan-out=0, I=0
	graph.AddEdge(models.GraphEdge{From: "A", To: "B"})
	graph.AddEdge(models.GraphEdge{From: "A", To: "C"})

	analyzer := NewSmellAnalyzer()
	analysis := analyzer.AnalyzeGraph(graph)

	if len(analysis.Components) != 3 {
		t.Fatalf("Expected 3 components, got %d", len(analysis.Components))
	}

	componentMap := make(map[string]models.ComponentMetrics)
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
	graph := models.NewDependencyGraph()

	analyzer := NewSmellAnalyzer()
	analysis := analyzer.AnalyzeGraph(graph)

	if len(analysis.Smells) != 0 {
		t.Errorf("Expected 0 smells for empty graph, got %d", len(analysis.Smells))
	}
	if len(analysis.Components) != 0 {
		t.Errorf("Expected 0 components for empty graph, got %d", len(analysis.Components))
	}
}

func TestSmellAnalyzer_Summary(t *testing.T) {
	graph := models.NewDependencyGraph()

	// Create cycle
	graph.AddNode(models.GraphNode{ID: "A", Name: "A"})
	graph.AddNode(models.GraphNode{ID: "B", Name: "B"})
	graph.AddEdge(models.GraphEdge{From: "A", To: "B"})
	graph.AddEdge(models.GraphEdge{From: "B", To: "A"})

	analyzer := NewSmellAnalyzer()
	analysis := analyzer.AnalyzeGraph(graph)

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
	thresholds := models.SmellThresholds{
		HubThreshold:          5,
		GodFanInThreshold:     3,
		GodFanOutThreshold:    3,
		InstabilityDifference: 0.5,
		StableThreshold:       0.2,
		UnstableThreshold:     0.8,
	}

	analyzer := NewSmellAnalyzer(WithSmellThresholds(thresholds))

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
			cm := models.ComponentMetrics{
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
