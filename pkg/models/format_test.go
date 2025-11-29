package models

import (
	"bytes"
	"encoding/json"
	"testing"
	"time"

	toon "github.com/toon-format/toon-go"
)

// TestAllTypesSerializeToJSON ensures all custom string types work with JSON encoding.
func TestAllTypesSerializeToJSON(t *testing.T) {
	now := time.Now()
	tests := []struct {
		name string
		data any
	}{
		{
			name: "TechnicalDebt",
			data: TechnicalDebt{
				Category:    DebtDesign,
				Severity:    SeverityHigh,
				File:        "test.go",
				Line:        42,
				Description: "Test debt",
				Marker:      "HACK",
				Text:        "some text",
				Author:      "test",
				Date:        &now,
			},
		},
		{
			name: "TechnicalDebt_nil_date",
			data: TechnicalDebt{
				Category:    DebtDefect,
				Severity:    SeverityMedium,
				File:        "test.go",
				Line:        10,
				Description: "Nil date test",
				Marker:      "BUG",
			},
		},
		{
			name: "SATDAnalysis",
			data: SATDAnalysis{
				Items: []TechnicalDebt{
					{Category: DebtRequirement, Severity: SeverityLow, File: "a.go", Line: 1, Marker: "TODO"},
				},
				Summary: SATDSummary{
					TotalItems: 1,
					BySeverity: map[string]int{"low": 1},
					ByCategory: map[string]int{"requirement": 1},
				},
				TotalFilesAnalyzed: 1,
				AnalyzedAt:         now,
			},
		},
		{
			name: "DeadCodeItem",
			data: DeadCodeItem{
				ItemType: DeadCodeTypeFunction,
				Name:     "unusedFunc",
				Line:     100,
				Reason:   "not called",
			},
		},
		{
			name: "DeadCodeAnalysis",
			data: DeadCodeAnalysis{
				DeadFunctions: []DeadFunction{
					{Name: "foo", File: "a.go", Line: 1, Confidence: 0.9, Reason: "unused"},
				},
				DeadVariables: []DeadVariable{
					{Name: "bar", File: "b.go", Line: 2, Confidence: 0.8, Reason: "unused"},
				},
			},
		},
		{
			name: "CodeClone",
			data: CodeClone{
				Type:       CloneType1,
				Similarity: 1.0,
				FileA:      "a.go",
				FileB:      "b.go",
				StartLineA: 1,
				EndLineA:   10,
				StartLineB: 20,
				EndLineB:   30,
			},
		},
		{
			name: "CloneAnalysis",
			data: CloneAnalysis{
				Clones: []CodeClone{
					{Type: CloneType2, Similarity: 0.95},
					{Type: CloneType3, Similarity: 0.8},
				},
				Summary: CloneSummary{
					TotalClones: 2,
					Type1Count:  0,
					Type2Count:  1,
					Type3Count:  1,
				},
			},
		},
		{
			name: "FunctionComplexity",
			data: FunctionComplexity{
				Name:      "testFunc",
				File:      "test.go",
				StartLine: 1,
				EndLine:   20,
				Metrics:   ComplexityMetrics{Cyclomatic: 5, Cognitive: 3, MaxNesting: 2, Lines: 20},
			},
		},
		{
			name: "FileComplexity",
			data: FileComplexity{
				Path:     "test.go",
				Language: "go",
				Functions: []FunctionComplexity{
					{Name: "foo", Metrics: ComplexityMetrics{Cyclomatic: 10}},
				},
			},
		},
		{
			name: "TdgScore",
			data: TdgScore{
				FilePath:   "test.go",
				Language:   LanguageGo,
				Total:      85.0,
				Grade:      GradeA,
				Confidence: 0.9,
			},
		},
		{
			name: "DefectScore",
			data: DefectScore{
				FilePath:    "test.go",
				Probability: 0.8,
				RiskLevel:   RiskHigh,
			},
		},
		{
			name: "GraphNode",
			data: GraphNode{
				ID:   "pkg/foo",
				Name: "foo",
				Type: NodePackage,
			},
		},
		{
			name: "GraphEdge",
			data: GraphEdge{
				From: "pkg/a",
				To:   "pkg/b",
				Type: EdgeImport,
			},
		},
		{
			name: "DependencyGraph",
			data: DependencyGraph{
				Nodes: []GraphNode{
					{ID: "a", Name: "a", Type: NodePackage},
				},
				Edges: []GraphEdge{
					{From: "a", To: "b", Type: EdgeImport},
				},
			},
		},
		{
			name: "Violation",
			data: Violation{
				Severity:  SeverityWarning,
				Rule:      "max_cyclomatic",
				Message:   "High complexity",
				Value:     15,
				Threshold: 10,
			},
		},
		{
			name: "DeadFunction_with_kind",
			data: DeadFunction{
				Name:       "test",
				File:       "test.go",
				Line:       1,
				Kind:       DeadKindFunction,
				Confidence: 0.9,
			},
		},
		{
			name: "ReferenceEdge",
			data: ReferenceEdge{
				From:       1,
				To:         2,
				Type:       RefDirectCall,
				Confidence: 0.95,
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name+"_json", func(t *testing.T) {
			data, err := json.Marshal(tt.data)
			if err != nil {
				t.Errorf("JSON marshal failed: %v", err)
				return
			}
			if len(data) == 0 {
				t.Error("JSON output should not be empty")
			}
		})
	}
}

// TestAllTypesSerializeToTOON ensures all custom string types work with TOON encoding.
func TestAllTypesSerializeToTOON(t *testing.T) {
	now := time.Now()
	tests := []struct {
		name string
		data any
	}{
		{
			name: "TechnicalDebt",
			data: TechnicalDebt{
				Category:    DebtDesign,
				Severity:    SeverityHigh,
				File:        "test.go",
				Line:        42,
				Description: "Test debt",
				Marker:      "HACK",
			},
		},
		{
			name: "TechnicalDebt_nil_date",
			data: TechnicalDebt{
				Category:    DebtDefect,
				Severity:    SeverityMedium,
				File:        "test.go",
				Line:        10,
				Description: "Nil date test",
				Marker:      "BUG",
			},
		},
		{
			name: "SATDAnalysis",
			data: SATDAnalysis{
				Items: []TechnicalDebt{
					{Category: DebtRequirement, Severity: SeverityLow, File: "a.go", Line: 1, Marker: "TODO"},
				},
				Summary: SATDSummary{
					TotalItems: 1,
					BySeverity: map[string]int{"low": 1},
					ByCategory: map[string]int{"requirement": 1},
				},
				TotalFilesAnalyzed: 1,
				AnalyzedAt:         now,
			},
		},
		{
			name: "DeadCodeItem",
			data: DeadCodeItem{
				ItemType: DeadCodeTypeFunction,
				Name:     "unusedFunc",
				Line:     100,
				Reason:   "unused",
			},
		},
		{
			name: "DeadCodeAnalysis",
			data: DeadCodeAnalysis{
				DeadFunctions: []DeadFunction{
					{Name: "foo", File: "a.go", Line: 1, Confidence: 0.9},
				},
				DeadVariables: []DeadVariable{
					{Name: "bar", File: "b.go", Line: 2, Confidence: 0.5},
				},
			},
		},
		{
			name: "CodeClone",
			data: CodeClone{
				Type:       CloneType1,
				Similarity: 1.0,
				FileA:      "a.go",
				FileB:      "b.go",
			},
		},
		{
			name: "FunctionComplexity",
			data: FunctionComplexity{
				Name:      "testFunc",
				File:      "test.go",
				StartLine: 1,
				EndLine:   20,
				Metrics:   ComplexityMetrics{Cyclomatic: 5, Cognitive: 3, MaxNesting: 2, Lines: 20},
			},
		},
		{
			name: "TdgScore",
			data: TdgScore{
				FilePath:   "test.go",
				Language:   LanguageGo,
				Total:      85.0,
				Grade:      GradeA,
				Confidence: 0.9,
			},
		},
		{
			name: "DefectScore",
			data: DefectScore{
				FilePath:    "test.go",
				Probability: 0.8,
				RiskLevel:   RiskHigh,
			},
		},
		{
			name: "DependencyGraph",
			data: DependencyGraph{
				Nodes: []GraphNode{
					{ID: "a", Name: "a", Type: NodePackage},
				},
				Edges: []GraphEdge{
					{From: "a", To: "b", Type: EdgeImport},
				},
			},
		},
		{
			name: "Violation",
			data: Violation{
				Severity: SeverityError,
				Rule:     "test",
				Message:  "test violation",
			},
		},
		{
			name: "DeadFunction_with_kind",
			data: DeadFunction{
				Name: "test",
				File: "test.go",
				Line: 1,
				Kind: DeadKindFunction,
			},
		},
		{
			name: "ReferenceEdge",
			data: ReferenceEdge{
				From: 1,
				To:   2,
				Type: RefDirectCall,
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name+"_toon", func(t *testing.T) {
			data, err := toon.Marshal(tt.data)
			if err != nil {
				t.Errorf("TOON marshal failed: %v", err)
				return
			}
			if len(data) == 0 {
				t.Error("TOON output should not be empty")
			}
		})
	}
}

// TestCustomStringTypesImplementStringer ensures all custom string types implement fmt.Stringer
// which is required for toon serialization.
func TestCustomStringTypesImplementStringer(t *testing.T) {
	tests := []struct {
		name   string
		value  interface{ String() string }
		expect string
	}{
		// DebtCategory values
		{"DebtCategory_design", DebtDesign, "design"},
		{"DebtCategory_defect", DebtDefect, "defect"},
		{"DebtCategory_requirement", DebtRequirement, "requirement"},
		{"DebtCategory_test", DebtTest, "test"},
		{"DebtCategory_performance", DebtPerformance, "performance"},
		{"DebtCategory_security", DebtSecurity, "security"},
		// Severity values
		{"Severity_low", SeverityLow, "low"},
		{"Severity_medium", SeverityMedium, "medium"},
		{"Severity_high", SeverityHigh, "high"},
		{"Severity_critical", SeverityCritical, "critical"},
		// ConfidenceLevel values
		{"ConfidenceLevel_high", ConfidenceHigh, "High"},
		{"ConfidenceLevel_medium", ConfidenceMedium, "Medium"},
		{"ConfidenceLevel_low", ConfidenceLow, "Low"},
		// DeadCodeType values
		{"DeadCodeType_function", DeadCodeTypeFunction, "function"},
		{"DeadCodeType_class", DeadCodeTypeClass, "class"},
		{"DeadCodeType_variable", DeadCodeTypeVariable, "variable"},
		{"DeadCodeType_unreachable", DeadCodeTypeUnreachable, "unreachable"},
		// ReferenceType values
		{"ReferenceType_direct_call", RefDirectCall, "direct_call"},
		{"ReferenceType_indirect_call", RefIndirectCall, "indirect_call"},
		{"ReferenceType_import", RefImport, "import"},
		// DeadCodeKind values
		{"DeadCodeKind_function", DeadKindFunction, "unused_function"},
		{"DeadCodeKind_class", DeadKindClass, "unused_class"},
		{"DeadCodeKind_variable", DeadKindVariable, "unused_variable"},
		// Grade values
		{"Grade_A", GradeA, "A"},
		{"Grade_B", GradeB, "B"},
		{"Grade_C", GradeC, "C"},
		{"Grade_D", GradeD, "D"},
		{"Grade_F", GradeF, "F"},
		// MetricCategory values
		{"MetricCategory_structural", MetricStructuralComplexity, "structural_complexity"},
		{"MetricCategory_semantic", MetricSemanticComplexity, "semantic_complexity"},
		// Language values
		{"Language_go", LanguageGo, "go"},
		{"Language_rust", LanguageRust, "rust"},
		{"Language_python", LanguagePython, "python"},
		// CloneType values
		{"CloneType_1", CloneType1, "type1"},
		{"CloneType_2", CloneType2, "type2"},
		{"CloneType_3", CloneType3, "type3"},
		// ViolationSeverity values
		{"ViolationSeverity_warning", SeverityWarning, "warning"},
		{"ViolationSeverity_error", SeverityError, "error"},
		// RiskLevel values
		{"RiskLevel_high", RiskHigh, "high"},
		{"RiskLevel_medium", RiskMedium, "medium"},
		{"RiskLevel_low", RiskLow, "low"},
		// NodeType values
		{"NodeType_package", NodePackage, "package"},
		{"NodeType_file", NodeFile, "file"},
		{"NodeType_function", NodeFunction, "function"},
		// EdgeType values
		{"EdgeType_import", EdgeImport, "import"},
		{"EdgeType_call", EdgeCall, "call"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := tt.value.String()
			if result != tt.expect {
				t.Errorf("String() = %q, want %q", result, tt.expect)
			}
		})
	}
}

// TestRoundTripJSON verifies JSON serialization and deserialization works correctly.
func TestRoundTripJSON(t *testing.T) {
	original := TechnicalDebt{
		Category:    DebtDesign,
		Severity:    SeverityHigh,
		File:        "test.go",
		Line:        42,
		Description: "Test debt",
		Marker:      "HACK",
	}

	data, err := json.Marshal(original)
	if err != nil {
		t.Fatalf("Marshal failed: %v", err)
	}

	var decoded TechnicalDebt
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatalf("Unmarshal failed: %v", err)
	}

	if decoded.Category != original.Category {
		t.Errorf("Category = %v, want %v", decoded.Category, original.Category)
	}
	if decoded.Severity != original.Severity {
		t.Errorf("Severity = %v, want %v", decoded.Severity, original.Severity)
	}
}

// TestTextOutputFormat verifies types can be formatted as text/strings for display.
func TestTextOutputFormat(t *testing.T) {
	tests := []struct {
		name   string
		format func() string
		want   string
	}{
		{
			name:   "DebtCategory_format",
			format: func() string { var buf bytes.Buffer; buf.WriteString(DebtDesign.String()); return buf.String() },
			want:   "design",
		},
		{
			name:   "Severity_format",
			format: func() string { var buf bytes.Buffer; buf.WriteString(SeverityHigh.String()); return buf.String() },
			want:   "high",
		},
		{
			name:   "Grade_format",
			format: func() string { var buf bytes.Buffer; buf.WriteString(GradeA.String()); return buf.String() },
			want:   "A",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := tt.format()
			if result != tt.want {
				t.Errorf("format() = %q, want %q", result, tt.want)
			}
		})
	}
}
