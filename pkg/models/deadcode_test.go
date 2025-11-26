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
