package models

import "testing"

func TestNewDeadCodeSummary(t *testing.T) {
	s := NewDeadCodeSummary()

	if s.ByFile == nil {
		t.Error("ByFile should be initialized")
	}

	if len(s.ByFile) != 0 {
		t.Errorf("ByFile should be empty, got %d items", len(s.ByFile))
	}

	if s.TotalDeadFunctions != 0 {
		t.Error("TotalDeadFunctions should be 0")
	}
}

func TestDeadCodeSummary_AddDeadFunction(t *testing.T) {
	tests := []struct {
		name              string
		functions         []DeadFunction
		expectedTotal     int
		expectedFileCount int
	}{
		{
			name: "single function",
			functions: []DeadFunction{
				{File: "file1.go", Name: "func1"},
			},
			expectedTotal:     1,
			expectedFileCount: 1,
		},
		{
			name: "multiple functions same file",
			functions: []DeadFunction{
				{File: "file1.go", Name: "func1"},
				{File: "file1.go", Name: "func2"},
			},
			expectedTotal:     2,
			expectedFileCount: 1,
		},
		{
			name: "multiple functions different files",
			functions: []DeadFunction{
				{File: "file1.go", Name: "func1"},
				{File: "file2.go", Name: "func2"},
				{File: "file3.go", Name: "func3"},
			},
			expectedTotal:     3,
			expectedFileCount: 3,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := NewDeadCodeSummary()

			for _, fn := range tt.functions {
				s.AddDeadFunction(fn)
			}

			if s.TotalDeadFunctions != tt.expectedTotal {
				t.Errorf("TotalDeadFunctions = %d, expected %d", s.TotalDeadFunctions, tt.expectedTotal)
			}

			if len(s.ByFile) != tt.expectedFileCount {
				t.Errorf("ByFile count = %d, expected %d", len(s.ByFile), tt.expectedFileCount)
			}
		})
	}
}

func TestDeadCodeSummary_AddDeadVariable(t *testing.T) {
	tests := []struct {
		name              string
		variables         []DeadVariable
		expectedTotal     int
		expectedFileCount int
	}{
		{
			name: "single variable",
			variables: []DeadVariable{
				{File: "file1.go", Name: "var1"},
			},
			expectedTotal:     1,
			expectedFileCount: 1,
		},
		{
			name: "multiple variables",
			variables: []DeadVariable{
				{File: "file1.go", Name: "var1"},
				{File: "file2.go", Name: "var2"},
			},
			expectedTotal:     2,
			expectedFileCount: 2,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := NewDeadCodeSummary()

			for _, v := range tt.variables {
				s.AddDeadVariable(v)
			}

			if s.TotalDeadVariables != tt.expectedTotal {
				t.Errorf("TotalDeadVariables = %d, expected %d", s.TotalDeadVariables, tt.expectedTotal)
			}

			if len(s.ByFile) != tt.expectedFileCount {
				t.Errorf("ByFile count = %d, expected %d", len(s.ByFile), tt.expectedFileCount)
			}
		})
	}
}

func TestDeadCodeSummary_AddUnreachableBlock(t *testing.T) {
	tests := []struct {
		name           string
		blocks         []UnreachableBlock
		expectedLines  int
		expectedByFile map[string]int
	}{
		{
			name: "single block",
			blocks: []UnreachableBlock{
				{File: "file1.go", StartLine: 10, EndLine: 15},
			},
			expectedLines: 6,
			expectedByFile: map[string]int{
				"file1.go": 6,
			},
		},
		{
			name: "multiple blocks same file",
			blocks: []UnreachableBlock{
				{File: "file1.go", StartLine: 10, EndLine: 15},
				{File: "file1.go", StartLine: 20, EndLine: 22},
			},
			expectedLines: 9,
			expectedByFile: map[string]int{
				"file1.go": 9,
			},
		},
		{
			name: "single line block",
			blocks: []UnreachableBlock{
				{File: "file1.go", StartLine: 10, EndLine: 10},
			},
			expectedLines: 1,
			expectedByFile: map[string]int{
				"file1.go": 1,
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := NewDeadCodeSummary()

			for _, block := range tt.blocks {
				s.AddUnreachableBlock(block)
			}

			if s.TotalUnreachableLines != tt.expectedLines {
				t.Errorf("TotalUnreachableLines = %d, expected %d", s.TotalUnreachableLines, tt.expectedLines)
			}

			for file, expectedCount := range tt.expectedByFile {
				if s.ByFile[file] != expectedCount {
					t.Errorf("ByFile[%s] = %d, expected %d", file, s.ByFile[file], expectedCount)
				}
			}
		})
	}
}

func TestDeadCodeSummary_CalculatePercentage(t *testing.T) {
	tests := []struct {
		name                  string
		totalLinesAnalyzed    int
		deadFunctions         int
		deadVariables         int
		unreachableLines      int
		expectedPercentageMin float64
		expectedPercentageMax float64
	}{
		{
			name:                  "no dead code",
			totalLinesAnalyzed:    1000,
			deadFunctions:         0,
			deadVariables:         0,
			unreachableLines:      0,
			expectedPercentageMin: 0.0,
			expectedPercentageMax: 0.0,
		},
		{
			name:                  "10% dead code",
			totalLinesAnalyzed:    1000,
			deadFunctions:         5,
			deadVariables:         20,
			unreachableLines:      30,
			expectedPercentageMin: 9.0,
			expectedPercentageMax: 11.0,
		},
		{
			name:                  "zero lines analyzed",
			totalLinesAnalyzed:    0,
			deadFunctions:         5,
			deadVariables:         10,
			unreachableLines:      20,
			expectedPercentageMin: 0.0,
			expectedPercentageMax: 0.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := DeadCodeSummary{
				TotalLinesAnalyzed:    tt.totalLinesAnalyzed,
				TotalDeadFunctions:    tt.deadFunctions,
				TotalDeadVariables:    tt.deadVariables,
				TotalUnreachableLines: tt.unreachableLines,
			}

			s.CalculatePercentage()

			if s.DeadCodePercentage < tt.expectedPercentageMin || s.DeadCodePercentage > tt.expectedPercentageMax {
				t.Errorf("DeadCodePercentage = %v, expected between %v and %v",
					s.DeadCodePercentage, tt.expectedPercentageMin, tt.expectedPercentageMax)
			}
		})
	}
}

func TestDeadCodeSummary_CombinedOperations(t *testing.T) {
	s := NewDeadCodeSummary()
	s.TotalLinesAnalyzed = 1000

	s.AddDeadFunction(DeadFunction{File: "file1.go", Name: "func1"})
	s.AddDeadFunction(DeadFunction{File: "file1.go", Name: "func2"})
	s.AddDeadVariable(DeadVariable{File: "file2.go", Name: "var1"})
	s.AddUnreachableBlock(UnreachableBlock{File: "file3.go", StartLine: 10, EndLine: 20})

	s.CalculatePercentage()

	if s.TotalDeadFunctions != 2 {
		t.Errorf("TotalDeadFunctions = %d, expected 2", s.TotalDeadFunctions)
	}
	if s.TotalDeadVariables != 1 {
		t.Errorf("TotalDeadVariables = %d, expected 1", s.TotalDeadVariables)
	}
	if s.TotalUnreachableLines != 11 {
		t.Errorf("TotalUnreachableLines = %d, expected 11", s.TotalUnreachableLines)
	}
	if s.DeadCodePercentage <= 0 {
		t.Error("DeadCodePercentage should be > 0")
	}
	if len(s.ByFile) != 3 {
		t.Errorf("ByFile count = %d, expected 3", len(s.ByFile))
	}
}

func TestDeadCodeSummary_AddDeadClass(t *testing.T) {
	s := NewDeadCodeSummary()

	s.AddDeadClass(DeadClass{File: "file1.ts", Name: "MyClass"})
	s.AddDeadClass(DeadClass{File: "file1.ts", Name: "OtherClass"})
	s.AddDeadClass(DeadClass{File: "file2.ts", Name: "AnotherClass"})

	if s.TotalDeadClasses != 3 {
		t.Errorf("TotalDeadClasses = %d, expected 3", s.TotalDeadClasses)
	}
	if len(s.ByFile) != 2 {
		t.Errorf("ByFile count = %d, expected 2", len(s.ByFile))
	}
}

func TestNewDeadCodeResult(t *testing.T) {
	result := NewDeadCodeResult()

	if result == nil {
		t.Fatal("NewDeadCodeResult returned nil")
	}
	if result.Files == nil {
		t.Error("Files should be initialized")
	}
}

func TestFileDeadCodeMetrics_CalculateScore(t *testing.T) {
	tests := []struct {
		name     string
		metrics  FileDeadCodeMetrics
		minScore float32
		maxScore float32
	}{
		{
			name: "high confidence high percentage",
			metrics: FileDeadCodeMetrics{
				DeadPercentage: 50.0,
				DeadLines:      100,
				DeadFunctions:  10,
				Confidence:     ConfidenceHigh,
			},
			minScore: 30.0,
			maxScore: 100.0,
		},
		{
			name: "low confidence",
			metrics: FileDeadCodeMetrics{
				DeadPercentage: 50.0,
				DeadLines:      100,
				DeadFunctions:  10,
				Confidence:     ConfidenceLow,
			},
			minScore: 20.0,
			maxScore: 90.0,
		},
		{
			name: "medium confidence default",
			metrics: FileDeadCodeMetrics{
				DeadPercentage: 25.0,
				DeadLines:      50,
				DeadFunctions:  5,
			},
			minScore: 10.0,
			maxScore: 50.0,
		},
		{
			name: "capped dead lines",
			metrics: FileDeadCodeMetrics{
				DeadPercentage: 10.0,
				DeadLines:      2000, // Over cap of 1000
				DeadFunctions:  100,  // Over cap of 50
				Confidence:     ConfidenceHigh,
			},
			minScore: 30.0,
			maxScore: 100.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tt.metrics.CalculateScore()
			if tt.metrics.DeadScore < tt.minScore || tt.metrics.DeadScore > tt.maxScore {
				t.Errorf("DeadScore = %v, expected between %v and %v", tt.metrics.DeadScore, tt.minScore, tt.maxScore)
			}
		})
	}
}

func TestFileDeadCodeMetrics_UpdatePercentage(t *testing.T) {
	tests := []struct {
		name     string
		metrics  FileDeadCodeMetrics
		expected float32
	}{
		{
			name:     "zero total lines",
			metrics:  FileDeadCodeMetrics{DeadLines: 10, TotalLines: 0},
			expected: 0.0,
		},
		{
			name:     "50% dead",
			metrics:  FileDeadCodeMetrics{DeadLines: 50, TotalLines: 100},
			expected: 50.0,
		},
		{
			name:     "10% dead",
			metrics:  FileDeadCodeMetrics{DeadLines: 10, TotalLines: 100},
			expected: 10.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tt.metrics.UpdatePercentage()
			if tt.metrics.DeadPercentage != tt.expected {
				t.Errorf("DeadPercentage = %v, expected %v", tt.metrics.DeadPercentage, tt.expected)
			}
		})
	}
}

func TestFileDeadCodeMetrics_AddItem(t *testing.T) {
	metrics := &FileDeadCodeMetrics{Items: make([]DeadCodeItem, 0)}

	metrics.AddItem(DeadCodeItem{ItemType: DeadCodeTypeFunction, Name: "func1"})
	metrics.AddItem(DeadCodeItem{ItemType: DeadCodeTypeClass, Name: "Class1"})
	metrics.AddItem(DeadCodeItem{ItemType: DeadCodeTypeVariable, Name: "var1"})
	metrics.AddItem(DeadCodeItem{ItemType: DeadCodeTypeUnreachable, Name: "block"})

	if metrics.DeadFunctions != 1 {
		t.Errorf("DeadFunctions = %d, expected 1", metrics.DeadFunctions)
	}
	if metrics.DeadClasses != 1 {
		t.Errorf("DeadClasses = %d, expected 1", metrics.DeadClasses)
	}
	if metrics.DeadModules != 1 { // Variables tracked as modules
		t.Errorf("DeadModules = %d, expected 1", metrics.DeadModules)
	}
	if metrics.UnreachableBlocks != 1 {
		t.Errorf("UnreachableBlocks = %d, expected 1", metrics.UnreachableBlocks)
	}
	if len(metrics.Items) != 4 {
		t.Errorf("Items count = %d, expected 4", len(metrics.Items))
	}
	// 10 (func) + 10 (class) + 1 (var) + 3 (unreachable) = 24
	if metrics.DeadLines != 24 {
		t.Errorf("DeadLines = %d, expected 24", metrics.DeadLines)
	}
}

func TestNewCallGraph(t *testing.T) {
	cg := NewCallGraph()

	if cg == nil {
		t.Fatal("NewCallGraph returned nil")
	}
	if cg.Nodes == nil {
		t.Error("Nodes should be initialized")
	}
	if cg.Edges == nil {
		t.Error("Edges should be initialized")
	}
	if cg.EdgeIndex == nil {
		t.Error("EdgeIndex should be initialized")
	}
}

func TestCallGraph_AddNode(t *testing.T) {
	cg := NewCallGraph()

	node1 := &ReferenceNode{ID: 1, Name: "func1"}
	node2 := &ReferenceNode{ID: 2, Name: "func2"}
	nodeEntry := &ReferenceNode{ID: 3, Name: "main", IsEntry: true}

	cg.AddNode(node1)
	cg.AddNode(node2)
	cg.AddNode(nodeEntry)

	if len(cg.Nodes) != 3 {
		t.Errorf("Nodes count = %d, expected 3", len(cg.Nodes))
	}
	if len(cg.EntryPoints) != 1 {
		t.Errorf("EntryPoints count = %d, expected 1", len(cg.EntryPoints))
	}
}

func TestCallGraph_AddEdge(t *testing.T) {
	cg := NewCallGraph()

	cg.AddEdge(ReferenceEdge{From: 1, To: 2, Type: RefDirectCall})
	cg.AddEdge(ReferenceEdge{From: 1, To: 3, Type: RefDirectCall})

	edges := cg.GetOutgoingEdges(1)
	if len(edges) != 2 {
		t.Errorf("Edges count = %d, expected 2", len(edges))
	}

	// Test getting edges for non-existent node
	emptyEdges := cg.GetOutgoingEdges(999)
	if len(emptyEdges) != 0 {
		t.Errorf("Expected 0 edges for non-existent node, got %d", len(emptyEdges))
	}
}

func TestDeadCodeSummary_AddDeadFunction_ByFileTracking(t *testing.T) {
	s := NewDeadCodeSummary()

	s.AddDeadFunction(DeadFunction{File: "test.go", Name: "func1"})
	s.AddDeadFunction(DeadFunction{File: "test.go", Name: "func2"})

	if s.ByFile["test.go"] != 2 {
		t.Errorf("ByFile[test.go] = %d, expected 2", s.ByFile["test.go"])
	}
}

func TestDeadCodeSummary_AddDeadVariable_ByFileTracking(t *testing.T) {
	s := NewDeadCodeSummary()

	s.AddDeadVariable(DeadVariable{File: "test.go", Name: "var1"})
	s.AddDeadVariable(DeadVariable{File: "test.go", Name: "var2"})

	if s.ByFile["test.go"] != 2 {
		t.Errorf("ByFile[test.go] = %d, expected 2", s.ByFile["test.go"])
	}
}

func TestDeadCodeSummary_AddDeadFunction_WithKind(t *testing.T) {
	s := NewDeadCodeSummary()

	s.AddDeadFunction(DeadFunction{File: "test.go", Name: "func1", Kind: DeadKindFunction})
	s.AddDeadFunction(DeadFunction{File: "test.go", Name: "func2"}) // No kind, should default

	if s.ByKind[DeadKindFunction] != 2 {
		t.Errorf("ByKind[DeadKindFunction] = %d, expected 2", s.ByKind[DeadKindFunction])
	}
}

func TestDeadCodeSummary_AddDeadVariable_WithKind(t *testing.T) {
	s := NewDeadCodeSummary()

	s.AddDeadVariable(DeadVariable{File: "test.go", Name: "var1", Kind: DeadKindVariable})
	s.AddDeadVariable(DeadVariable{File: "test.go", Name: "var2"}) // No kind

	if s.ByKind[DeadKindVariable] != 2 {
		t.Errorf("ByKind[DeadKindVariable] = %d, expected 2", s.ByKind[DeadKindVariable])
	}
}

func TestDeadCodeSummary_AddDeadClass_WithKind(t *testing.T) {
	s := NewDeadCodeSummary()

	s.AddDeadClass(DeadClass{File: "test.ts", Name: "MyClass", Kind: DeadKindClass})
	s.AddDeadClass(DeadClass{File: "test.ts", Name: "Other"}) // No kind

	if s.ByKind[DeadKindClass] != 2 {
		t.Errorf("ByKind[DeadKindClass] = %d, expected 2", s.ByKind[DeadKindClass])
	}
}

func TestDeadCodeSummary_CalculatePercentage_WithGraph(t *testing.T) {
	s := NewDeadCodeSummary()
	s.TotalNodesInGraph = 100
	s.ReachableNodes = 80

	s.CalculatePercentage()

	if s.UnreachableNodes != 20 {
		t.Errorf("UnreachableNodes = %d, expected 20", s.UnreachableNodes)
	}
	if s.DeadCodePercentage != 20.0 {
		t.Errorf("DeadCodePercentage = %v, expected 20.0", s.DeadCodePercentage)
	}
}

func TestDeadCodeResult_FromDeadCodeAnalysis(t *testing.T) {
	analysis := &DeadCodeAnalysis{
		DeadFunctions: []DeadFunction{
			{File: "main.go", Name: "unusedFunc", Line: 10, Confidence: 0.95},
			{File: "main.go", Name: "anotherUnused", Line: 20, Confidence: 0.7},
		},
		DeadClasses: []DeadClass{
			{File: "types.go", Name: "UnusedType", Line: 5},
		},
		DeadVariables: []DeadVariable{
			{File: "main.go", Name: "unusedVar", Line: 30},
		},
		UnreachableCode: []UnreachableBlock{
			{File: "main.go", StartLine: 40, EndLine: 45},
		},
		Summary: DeadCodeSummary{
			TotalFilesAnalyzed:    5,
			TotalDeadFunctions:    2,
			TotalDeadClasses:      1,
			TotalDeadVariables:    1,
			TotalUnreachableLines: 6,
		},
	}

	result := NewDeadCodeResult()
	result.FromDeadCodeAnalysis(analysis)

	if result.TotalFiles != 5 {
		t.Errorf("TotalFiles = %d, expected 5", result.TotalFiles)
	}
	if result.Summary.DeadFunctions != 2 {
		t.Errorf("Summary.DeadFunctions = %d, expected 2", result.Summary.DeadFunctions)
	}
	if len(result.Files) != 2 { // main.go and types.go
		t.Errorf("Files count = %d, expected 2", len(result.Files))
	}
}

func TestDeadFunction_SetConfidenceLevel(t *testing.T) {
	tests := []struct {
		name                   string
		confidence             float64
		expectedLevel          ConfidenceLevel
		expectedReasonContains string
	}{
		{
			name:                   "high confidence - private unexported",
			confidence:             0.95,
			expectedLevel:          ConfidenceHigh,
			expectedReasonContains: "High confidence",
		},
		{
			name:                   "high confidence - boundary",
			confidence:             0.8,
			expectedLevel:          ConfidenceHigh,
			expectedReasonContains: "private/unexported",
		},
		{
			name:                   "medium confidence",
			confidence:             0.65,
			expectedLevel:          ConfidenceMedium,
			expectedReasonContains: "Medium confidence",
		},
		{
			name:                   "medium confidence - lower boundary",
			confidence:             0.5,
			expectedLevel:          ConfidenceMedium,
			expectedReasonContains: "exported but no internal",
		},
		{
			name:                   "low confidence - below medium threshold",
			confidence:             0.49,
			expectedLevel:          ConfidenceLow,
			expectedReasonContains: "Low confidence",
		},
		{
			name:                   "low confidence - very low",
			confidence:             0.1,
			expectedLevel:          ConfidenceLow,
			expectedReasonContains: "dynamic usage",
		},
		{
			name:                   "low confidence - zero",
			confidence:             0.0,
			expectedLevel:          ConfidenceLow,
			expectedReasonContains: "Low confidence",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			df := DeadFunction{
				Name:       "testFunc",
				Confidence: tt.confidence,
			}
			df.SetConfidenceLevel()

			if df.ConfidenceLevel != tt.expectedLevel {
				t.Errorf("ConfidenceLevel = %s, expected %s", df.ConfidenceLevel, tt.expectedLevel)
			}
			if df.ConfidenceReason == "" {
				t.Error("ConfidenceReason should not be empty")
			}
			if !contains(df.ConfidenceReason, tt.expectedReasonContains) {
				t.Errorf("ConfidenceReason = %q, expected to contain %q", df.ConfidenceReason, tt.expectedReasonContains)
			}
		})
	}
}

func TestDeadClass_SetConfidenceLevel(t *testing.T) {
	tests := []struct {
		name                   string
		confidence             float64
		expectedLevel          ConfidenceLevel
		expectedReasonContains string
	}{
		{
			name:                   "high confidence",
			confidence:             0.9,
			expectedLevel:          ConfidenceHigh,
			expectedReasonContains: "private/unexported type",
		},
		{
			name:                   "medium confidence",
			confidence:             0.6,
			expectedLevel:          ConfidenceMedium,
			expectedReasonContains: "exported type",
		},
		{
			name:                   "low confidence",
			confidence:             0.3,
			expectedLevel:          ConfidenceLow,
			expectedReasonContains: "reflection",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dc := DeadClass{
				Name:       "TestClass",
				Confidence: tt.confidence,
			}
			dc.SetConfidenceLevel()

			if dc.ConfidenceLevel != tt.expectedLevel {
				t.Errorf("ConfidenceLevel = %s, expected %s", dc.ConfidenceLevel, tt.expectedLevel)
			}
			if dc.ConfidenceReason == "" {
				t.Error("ConfidenceReason should not be empty")
			}
		})
	}
}

func TestDeadVariable_SetConfidenceLevel(t *testing.T) {
	tests := []struct {
		name                   string
		confidence             float64
		expectedLevel          ConfidenceLevel
		expectedReasonContains string
	}{
		{
			name:                   "high confidence",
			confidence:             0.85,
			expectedLevel:          ConfidenceHigh,
			expectedReasonContains: "private/unexported variable",
		},
		{
			name:                   "medium confidence",
			confidence:             0.55,
			expectedLevel:          ConfidenceMedium,
			expectedReasonContains: "exported variable",
		},
		{
			name:                   "low confidence",
			confidence:             0.2,
			expectedLevel:          ConfidenceLow,
			expectedReasonContains: "dynamically",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dv := DeadVariable{
				Name:       "testVar",
				Confidence: tt.confidence,
			}
			dv.SetConfidenceLevel()

			if dv.ConfidenceLevel != tt.expectedLevel {
				t.Errorf("ConfidenceLevel = %s, expected %s", dv.ConfidenceLevel, tt.expectedLevel)
			}
			if dv.ConfidenceReason == "" {
				t.Error("ConfidenceReason should not be empty")
			}
		})
	}
}

// contains checks if a string contains a substring (case-insensitive for flexibility)
func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(substr) == 0 ||
		(len(s) > 0 && containsSubstring(s, substr)))
}

func containsSubstring(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
