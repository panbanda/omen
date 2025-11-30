package analyzer

import (
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/panbanda/omen/pkg/models"
)

// SmellAnalyzer detects architectural smells in dependency graphs.
// Implements detection algorithms from Fontana et al. (2017) "Arcan".
// This analyzer is safe for concurrent use.
type SmellAnalyzer struct {
	thresholds models.SmellThresholds
}

// SmellOption is a functional option for configuring SmellAnalyzer.
type SmellOption func(*SmellAnalyzer)

// WithSmellThresholds sets custom detection thresholds.
func WithSmellThresholds(thresholds models.SmellThresholds) SmellOption {
	return func(a *SmellAnalyzer) {
		a.thresholds = thresholds
	}
}

// WithHubThreshold sets the hub detection threshold.
func WithHubThreshold(threshold int) SmellOption {
	return func(a *SmellAnalyzer) {
		a.thresholds.HubThreshold = threshold
	}
}

// WithGodThresholds sets the god component detection thresholds.
func WithGodThresholds(fanIn, fanOut int) SmellOption {
	return func(a *SmellAnalyzer) {
		a.thresholds.GodFanInThreshold = fanIn
		a.thresholds.GodFanOutThreshold = fanOut
	}
}

// WithInstabilityDifference sets the unstable dependency threshold.
func WithInstabilityDifference(diff float64) SmellOption {
	return func(a *SmellAnalyzer) {
		a.thresholds.InstabilityDifference = diff
	}
}

// NewSmellAnalyzer creates a new smell analyzer.
func NewSmellAnalyzer(opts ...SmellOption) *SmellAnalyzer {
	a := &SmellAnalyzer{
		thresholds: models.DefaultSmellThresholds(),
	}
	for _, opt := range opts {
		opt(a)
	}

	// Validate thresholds are positive
	if a.thresholds.HubThreshold <= 0 {
		a.thresholds.HubThreshold = 20 // Default
	}
	if a.thresholds.GodFanInThreshold <= 0 {
		a.thresholds.GodFanInThreshold = 10 // Default
	}
	if a.thresholds.GodFanOutThreshold <= 0 {
		a.thresholds.GodFanOutThreshold = 10 // Default
	}
	if a.thresholds.InstabilityDifference <= 0 {
		a.thresholds.InstabilityDifference = 0.4 // Default
	}

	return a
}

// AnalyzeGraph detects architectural smells in a dependency graph.
func (a *SmellAnalyzer) AnalyzeGraph(graph *models.DependencyGraph) *models.SmellAnalysis {
	analysis := &models.SmellAnalysis{
		GeneratedAt: time.Now().UTC(),
		Smells:      make([]models.ArchitecturalSmell, 0),
		Components:  make([]models.ComponentMetrics, 0),
		Thresholds:  a.thresholds,
	}

	if len(graph.Nodes) == 0 {
		return analysis
	}

	// Calculate component metrics (fan-in, fan-out, instability)
	analysis.Components = a.calculateComponentMetrics(graph)

	// Create lookup maps for smell detection
	componentMap := make(map[string]*models.ComponentMetrics)
	for i := range analysis.Components {
		componentMap[analysis.Components[i].ID] = &analysis.Components[i]
	}

	// Phase 1: Detect cyclic dependencies using Tarjan's SCC
	a.detectCyclicDependencies(graph, analysis)

	// Phase 2: Detect hub-like dependencies
	a.detectHubDependencies(analysis, componentMap)

	// Phase 3: Detect god components
	a.detectGodComponents(analysis, componentMap)

	// Phase 4: Detect unstable dependencies
	a.detectUnstableDependencies(graph, analysis, componentMap)

	// Calculate summary statistics
	analysis.CalculateSummary()

	return analysis
}

// calculateComponentMetrics computes fan-in, fan-out, and instability for each node.
func (a *SmellAnalyzer) calculateComponentMetrics(graph *models.DependencyGraph) []models.ComponentMetrics {
	// Initialize counters
	fanIn := make(map[string]int)
	fanOut := make(map[string]int)

	for _, node := range graph.Nodes {
		fanIn[node.ID] = 0
		fanOut[node.ID] = 0
	}

	// Count edges
	for _, edge := range graph.Edges {
		fanOut[edge.From]++
		fanIn[edge.To]++
	}

	// Build component metrics
	components := make([]models.ComponentMetrics, 0, len(graph.Nodes))
	for _, node := range graph.Nodes {
		cm := models.ComponentMetrics{
			ID:     node.ID,
			Name:   node.Name,
			FanIn:  fanIn[node.ID],
			FanOut: fanOut[node.ID],
		}
		cm.CalculateInstability()

		// Check for hub-like and god components
		// A hub must have significant fan-in (things depend on it) to be considered a connector
		// Files with only fan-out are just consumers, not hubs
		// Require at least 3 dependents to be considered a hub
		totalDegree := cm.FanIn + cm.FanOut
		cm.IsHub = totalDegree > a.thresholds.HubThreshold && cm.FanIn >= 3
		cm.IsGod = cm.FanIn > a.thresholds.GodFanInThreshold &&
			cm.FanOut > a.thresholds.GodFanOutThreshold

		components = append(components, cm)
	}

	// Sort by instability descending for better reporting
	sort.Slice(components, func(i, j int) bool {
		return components[i].Instability > components[j].Instability
	})

	return components
}

// detectCyclicDependencies finds cycles using Tarjan's strongly connected components.
func (a *SmellAnalyzer) detectCyclicDependencies(graph *models.DependencyGraph, analysis *models.SmellAnalysis) {
	// Build adjacency list
	adj := make(map[string][]string)
	for _, node := range graph.Nodes {
		adj[node.ID] = []string{}
	}
	for _, edge := range graph.Edges {
		adj[edge.From] = append(adj[edge.From], edge.To)
	}

	// Tarjan's algorithm state (local for thread safety)
	index := 0
	stack := make([]string, 0, len(graph.Nodes))
	onStack := make(map[string]bool, len(graph.Nodes))
	indices := make(map[string]int, len(graph.Nodes))
	lowlinks := make(map[string]int, len(graph.Nodes))
	sccs := make([][]string, 0)

	var strongConnect func(v string)
	strongConnect = func(v string) {
		indices[v] = index
		lowlinks[v] = index
		index++
		stack = append(stack, v)
		onStack[v] = true

		for _, w := range adj[v] {
			if _, visited := indices[w]; !visited {
				strongConnect(w)
				if lowlinks[w] < lowlinks[v] {
					lowlinks[v] = lowlinks[w]
				}
			} else if onStack[w] {
				if indices[w] < lowlinks[v] {
					lowlinks[v] = indices[w]
				}
			}
		}

		if lowlinks[v] == indices[v] {
			scc := make([]string, 0)
			for {
				w := stack[len(stack)-1]
				stack = stack[:len(stack)-1]
				onStack[w] = false
				scc = append(scc, w)
				if w == v {
					break
				}
			}
			if len(scc) > 1 {
				sccs = append(sccs, scc)
			}
		}
	}

	for _, node := range graph.Nodes {
		if _, visited := indices[node.ID]; !visited {
			strongConnect(node.ID)
		}
	}

	// Create smells for each cycle
	for _, scc := range sccs {
		smell := models.ArchitecturalSmell{
			Type:       models.SmellCyclicDependency,
			Severity:   models.SmellSeverityCritical,
			Components: scc,
			Description: fmt.Sprintf("Cyclic dependency detected between %d components: %s",
				len(scc), formatComponentList(scc)),
			Suggestion: "Break the cycle by introducing an interface or restructuring the dependency direction",
			Metrics: models.SmellMetrics{
				CycleLength: len(scc),
			},
		}
		analysis.AddSmell(smell)
	}
}

// detectHubDependencies finds components with excessive fan-in + fan-out.
func (a *SmellAnalyzer) detectHubDependencies(analysis *models.SmellAnalysis, componentMap map[string]*models.ComponentMetrics) {
	for _, cm := range analysis.Components {
		if cm.IsHub && !cm.IsGod { // God components are reported separately
			smell := models.ArchitecturalSmell{
				Type:       models.SmellHubLikeDependency,
				Severity:   models.SmellSeverityHigh,
				Components: []string{cm.ID},
				Description: fmt.Sprintf("Hub-like component %q has %d connections (fan-in=%d, fan-out=%d, threshold=%d)",
					cm.Name, cm.FanIn+cm.FanOut, cm.FanIn, cm.FanOut, a.thresholds.HubThreshold),
				Suggestion: "Consider splitting this component into smaller, more focused modules",
				Metrics: models.SmellMetrics{
					FanIn:       cm.FanIn,
					FanOut:      cm.FanOut,
					Instability: cm.Instability,
				},
			}
			analysis.AddSmell(smell)
		}
	}
}

// detectGodComponents finds components with both high fan-in and high fan-out.
func (a *SmellAnalyzer) detectGodComponents(analysis *models.SmellAnalysis, componentMap map[string]*models.ComponentMetrics) {
	for _, cm := range analysis.Components {
		if cm.IsGod {
			smell := models.ArchitecturalSmell{
				Type:       models.SmellGodComponent,
				Severity:   models.SmellSeverityCritical,
				Components: []string{cm.ID},
				Description: fmt.Sprintf("God component %q has excessive coupling (fan-in=%d, fan-out=%d)",
					cm.Name, cm.FanIn, cm.FanOut),
				Suggestion: "Decompose into smaller components with single responsibility; extract interfaces for consumers",
				Metrics: models.SmellMetrics{
					FanIn:       cm.FanIn,
					FanOut:      cm.FanOut,
					Instability: cm.Instability,
				},
			}
			analysis.AddSmell(smell)
		}
	}
}

// detectUnstableDependencies finds stable components depending on unstable ones.
// This violates the Stable Dependencies Principle (SDP).
func (a *SmellAnalyzer) detectUnstableDependencies(graph *models.DependencyGraph, analysis *models.SmellAnalysis, componentMap map[string]*models.ComponentMetrics) {
	for _, edge := range graph.Edges {
		fromCM := componentMap[edge.From]
		toCM := componentMap[edge.To]

		if fromCM == nil || toCM == nil {
			continue
		}

		// Check if stable component depends on unstable component
		// Stable: I < 0.3, Unstable: I > 0.7
		isFromStable := fromCM.Instability < a.thresholds.StableThreshold
		isToUnstable := toCM.Instability > a.thresholds.UnstableThreshold

		if isFromStable && isToUnstable {
			diff := toCM.Instability - fromCM.Instability
			if diff > a.thresholds.InstabilityDifference {
				smell := models.ArchitecturalSmell{
					Type:       models.SmellUnstableDependency,
					Severity:   models.SmellSeverityMedium,
					Components: []string{edge.From, edge.To},
					Description: fmt.Sprintf("Stable component %q (I=%.2f) depends on unstable component %q (I=%.2f)",
						fromCM.Name, fromCM.Instability, toCM.Name, toCM.Instability),
					Suggestion: "Introduce an interface in the stable component that the unstable component implements (Dependency Inversion)",
					Metrics: models.SmellMetrics{
						Instability: diff,
					},
				}
				analysis.AddSmell(smell)
			}
		}
	}
}

// formatComponentList formats a list of component IDs for display.
func formatComponentList(components []string) string {
	if len(components) == 0 {
		return ""
	}
	if len(components) <= 3 {
		return strings.Join(components, " -> ")
	}
	return fmt.Sprintf("%s -> ... -> %s", components[0], components[len(components)-1])
}

// Close releases analyzer resources.
func (a *SmellAnalyzer) Close() {
	// No resources to release
}
