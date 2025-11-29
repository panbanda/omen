package models

import (
	"testing"
)

func TestCohesionAnalysis_CalculateSummary(t *testing.T) {
	analysis := &CohesionAnalysis{
		Classes: []ClassMetrics{
			{Path: "a.java", ClassName: "A", WMC: 10, CBO: 3, RFC: 8, LCOM: 2},
			{Path: "b.java", ClassName: "B", WMC: 5, CBO: 5, RFC: 10, LCOM: 1},
			{Path: "c.java", ClassName: "C", WMC: 15, CBO: 2, RFC: 5, LCOM: 3},
		},
	}

	analysis.CalculateSummary()

	if analysis.Summary.TotalClasses != 3 {
		t.Errorf("TotalClasses = %d, want 3", analysis.Summary.TotalClasses)
	}
	if analysis.Summary.MaxWMC != 15 {
		t.Errorf("MaxWMC = %d, want 15", analysis.Summary.MaxWMC)
	}
	if analysis.Summary.MaxCBO != 5 {
		t.Errorf("MaxCBO = %d, want 5", analysis.Summary.MaxCBO)
	}
	if analysis.Summary.MaxRFC != 10 {
		t.Errorf("MaxRFC = %d, want 10", analysis.Summary.MaxRFC)
	}
	if analysis.Summary.MaxLCOM != 3 {
		t.Errorf("MaxLCOM = %d, want 3", analysis.Summary.MaxLCOM)
	}
	if analysis.Summary.LowCohesionCount != 2 {
		t.Errorf("LowCohesionCount = %d, want 2 (classes with LCOM > 1)", analysis.Summary.LowCohesionCount)
	}
}

func TestCohesionAnalysis_SortByLCOM(t *testing.T) {
	analysis := &CohesionAnalysis{
		Classes: []ClassMetrics{
			{ClassName: "Low", LCOM: 1},
			{ClassName: "High", LCOM: 5},
			{ClassName: "Mid", LCOM: 3},
		},
	}

	analysis.SortByLCOM()

	if analysis.Classes[0].ClassName != "High" {
		t.Errorf("First class should be 'High', got %q", analysis.Classes[0].ClassName)
	}
	if analysis.Classes[1].ClassName != "Mid" {
		t.Errorf("Second class should be 'Mid', got %q", analysis.Classes[1].ClassName)
	}
	if analysis.Classes[2].ClassName != "Low" {
		t.Errorf("Third class should be 'Low', got %q", analysis.Classes[2].ClassName)
	}
}

func TestCohesionAnalysis_SortByWMC(t *testing.T) {
	analysis := &CohesionAnalysis{
		Classes: []ClassMetrics{
			{ClassName: "Simple", WMC: 5},
			{ClassName: "Complex", WMC: 50},
			{ClassName: "Medium", WMC: 20},
		},
	}

	analysis.SortByWMC()

	if analysis.Classes[0].ClassName != "Complex" {
		t.Errorf("First class should be 'Complex', got %q", analysis.Classes[0].ClassName)
	}
}

func TestCohesionAnalysis_SortByCBO(t *testing.T) {
	analysis := &CohesionAnalysis{
		Classes: []ClassMetrics{
			{ClassName: "Loosely", CBO: 2},
			{ClassName: "Coupled", CBO: 10},
			{ClassName: "Medium", CBO: 5},
		},
	}

	analysis.SortByCBO()

	if analysis.Classes[0].ClassName != "Coupled" {
		t.Errorf("First class should be 'Coupled', got %q", analysis.Classes[0].ClassName)
	}
}

func TestCohesionAnalysis_Empty(t *testing.T) {
	analysis := &CohesionAnalysis{
		Classes: []ClassMetrics{},
	}

	analysis.CalculateSummary()

	if analysis.Summary.TotalClasses != 0 {
		t.Errorf("TotalClasses = %d, want 0", analysis.Summary.TotalClasses)
	}
}

func TestClassMetrics_Fields(t *testing.T) {
	cm := ClassMetrics{
		Path:      "/test/MyClass.java",
		ClassName: "MyClass",
		Language:  "java",
		StartLine: 10,
		EndLine:   50,
		WMC:       15,
		CBO:       5,
		RFC:       20,
		LCOM:      2,
		DIT:       1,
		NOC:       3,
		NOM:       6,
		NOF:       4,
		LOC:       41,
		Methods:   []string{"init", "process", "cleanup"},
		Fields:    []string{"data", "config"},
	}

	if cm.ClassName != "MyClass" {
		t.Errorf("ClassName = %q, want %q", cm.ClassName, "MyClass")
	}
	if cm.WMC != 15 {
		t.Errorf("WMC = %d, want 15", cm.WMC)
	}
	if cm.LCOM != 2 {
		t.Errorf("LCOM = %d, want 2", cm.LCOM)
	}
	if len(cm.Methods) != 3 {
		t.Errorf("Methods count = %d, want 3", len(cm.Methods))
	}
}

func TestCohesionAnalysis_SortByDIT(t *testing.T) {
	analysis := &CohesionAnalysis{
		Classes: []ClassMetrics{
			{ClassName: "Shallow", DIT: 1},
			{ClassName: "Deep", DIT: 5},
			{ClassName: "Medium", DIT: 3},
		},
	}

	analysis.SortByDIT()

	if analysis.Classes[0].ClassName != "Deep" {
		t.Errorf("First class should be 'Deep', got %q", analysis.Classes[0].ClassName)
	}
	if analysis.Classes[1].ClassName != "Medium" {
		t.Errorf("Second class should be 'Medium', got %q", analysis.Classes[1].ClassName)
	}
	if analysis.Classes[2].ClassName != "Shallow" {
		t.Errorf("Third class should be 'Shallow', got %q", analysis.Classes[2].ClassName)
	}
}
