package models

import (
	"testing"
)

func TestRepoMap_SortByPageRank(t *testing.T) {
	repoMap := &RepoMap{
		Symbols: []Symbol{
			{Name: "low", PageRank: 0.1},
			{Name: "high", PageRank: 0.9},
			{Name: "mid", PageRank: 0.5},
		},
	}

	repoMap.SortByPageRank()

	if repoMap.Symbols[0].Name != "high" {
		t.Errorf("Expected first symbol to be 'high', got %q", repoMap.Symbols[0].Name)
	}
	if repoMap.Symbols[1].Name != "mid" {
		t.Errorf("Expected second symbol to be 'mid', got %q", repoMap.Symbols[1].Name)
	}
	if repoMap.Symbols[2].Name != "low" {
		t.Errorf("Expected third symbol to be 'low', got %q", repoMap.Symbols[2].Name)
	}
}

func TestRepoMap_CalculateSummary(t *testing.T) {
	repoMap := &RepoMap{
		Symbols: []Symbol{
			{Name: "a", File: "file1.go", PageRank: 0.5, InDegree: 2, OutDegree: 1},
			{Name: "b", File: "file1.go", PageRank: 0.3, InDegree: 1, OutDegree: 2},
			{Name: "c", File: "file2.go", PageRank: 0.2, InDegree: 0, OutDegree: 1},
		},
	}

	repoMap.CalculateSummary()

	if repoMap.Summary.TotalSymbols != 3 {
		t.Errorf("TotalSymbols = %d, want 3", repoMap.Summary.TotalSymbols)
	}
	if repoMap.Summary.TotalFiles != 2 {
		t.Errorf("TotalFiles = %d, want 2", repoMap.Summary.TotalFiles)
	}
	if repoMap.Summary.MaxPageRank != 0.5 {
		t.Errorf("MaxPageRank = %f, want 0.5", repoMap.Summary.MaxPageRank)
	}
}

func TestRepoMap_TopN(t *testing.T) {
	repoMap := &RepoMap{
		Symbols: []Symbol{
			{Name: "a", PageRank: 0.1},
			{Name: "b", PageRank: 0.5},
			{Name: "c", PageRank: 0.3},
			{Name: "d", PageRank: 0.9},
			{Name: "e", PageRank: 0.7},
		},
	}

	top3 := repoMap.TopN(3)
	if len(top3) != 3 {
		t.Fatalf("Expected 3 symbols, got %d", len(top3))
	}

	expected := []string{"d", "e", "b"}
	for i, name := range expected {
		if top3[i].Name != name {
			t.Errorf("TopN[%d] = %q, want %q", i, top3[i].Name, name)
		}
	}
}

func TestRepoMap_TopN_MoreThanTotal(t *testing.T) {
	repoMap := &RepoMap{
		Symbols: []Symbol{
			{Name: "a", PageRank: 0.5},
			{Name: "b", PageRank: 0.3},
		},
	}

	top5 := repoMap.TopN(5)
	if len(top5) != 2 {
		t.Errorf("Expected 2 symbols when requesting more than available, got %d", len(top5))
	}
}

func TestRepoMap_Empty(t *testing.T) {
	repoMap := &RepoMap{
		Symbols: []Symbol{},
	}

	repoMap.CalculateSummary()

	if repoMap.Summary.TotalSymbols != 0 {
		t.Errorf("TotalSymbols = %d, want 0", repoMap.Summary.TotalSymbols)
	}
	if repoMap.Summary.TotalFiles != 0 {
		t.Errorf("TotalFiles = %d, want 0", repoMap.Summary.TotalFiles)
	}
}

func TestSymbol_Fields(t *testing.T) {
	s := Symbol{
		Name:      "TestFunc",
		Kind:      "function",
		File:      "/test/file.go",
		Line:      42,
		Signature: "func TestFunc()",
		PageRank:  0.75,
		InDegree:  3,
		OutDegree: 2,
	}

	if s.Name != "TestFunc" {
		t.Errorf("Name = %q, want %q", s.Name, "TestFunc")
	}
	if s.Kind != "function" {
		t.Errorf("Kind = %q, want %q", s.Kind, "function")
	}
	if s.Line != 42 {
		t.Errorf("Line = %d, want 42", s.Line)
	}
	if s.PageRank != 0.75 {
		t.Errorf("PageRank = %f, want 0.75", s.PageRank)
	}
}
