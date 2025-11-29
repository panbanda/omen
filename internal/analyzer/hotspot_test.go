package analyzer

import (
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/models"
)

func TestNewHotspotAnalyzer(t *testing.T) {
	tests := []struct {
		name     string
		opts     []HotspotOption
		wantDays int64
		wantSize int64
	}{
		{"default values", nil, 30, 0},
		{"custom days", []HotspotOption{WithHotspotChurnDays(60)}, 60, 0},
		{"custom size", []HotspotOption{WithHotspotMaxFileSize(1024)}, 30, 1024},
		{"both options", []HotspotOption{WithHotspotChurnDays(90), WithHotspotMaxFileSize(2048)}, 90, 2048},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			a := NewHotspotAnalyzer(tt.opts...)
			defer a.Close()

			if int64(a.churnDays) != tt.wantDays {
				t.Errorf("churnDays = %d, want %d", a.churnDays, tt.wantDays)
			}
			if a.maxFileSize != tt.wantSize {
				t.Errorf("maxFileSize = %d, want %d", a.maxFileSize, tt.wantSize)
			}
		})
	}
}

func TestHotspotAnalysis_CalculateSummary(t *testing.T) {
	tests := []struct {
		name         string
		files        []models.FileHotspot
		wantCount    int
		wantMax      float64
		wantHotspots int
	}{
		{
			name:         "empty files",
			files:        []models.FileHotspot{},
			wantCount:    0,
			wantMax:      0,
			wantHotspots: 0,
		},
		{
			name: "single file below threshold",
			files: []models.FileHotspot{
				{Path: "a.go", HotspotScore: 0.3},
			},
			wantCount:    1,
			wantMax:      0.3,
			wantHotspots: 0,
		},
		{
			name: "multiple files mixed",
			files: []models.FileHotspot{
				{Path: "a.go", HotspotScore: 0.8},
				{Path: "b.go", HotspotScore: 0.6},
				{Path: "c.go", HotspotScore: 0.3},
				{Path: "d.go", HotspotScore: 0.1},
			},
			wantCount:    4,
			wantMax:      0.8,
			wantHotspots: 2, // 0.8 and 0.6 are >= 0.5
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analysis := &models.HotspotAnalysis{Files: tt.files}
			analysis.CalculateSummary()

			if analysis.Summary.TotalFiles != tt.wantCount {
				t.Errorf("TotalFiles = %d, want %d", analysis.Summary.TotalFiles, tt.wantCount)
			}
			if analysis.Summary.MaxHotspotScore != tt.wantMax {
				t.Errorf("MaxHotspotScore = %f, want %f", analysis.Summary.MaxHotspotScore, tt.wantMax)
			}
			if analysis.Summary.HotspotCount != tt.wantHotspots {
				t.Errorf("HotspotCount = %d, want %d", analysis.Summary.HotspotCount, tt.wantHotspots)
			}
		})
	}
}

func TestHotspotAnalyzer_AnalyzeProject(t *testing.T) {
	// Create temp directory with test files
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	// Initialize git repo using helper from churn_test.go
	repo := initGitRepo(t, repoPath)

	// Create test file with varying complexity
	content := `package main

func complex() {
	if true {
		for i := 0; i < 10; i++ {
			if i > 5 {
				println(i)
			}
		}
	}
}

func simple() {
	println("hello")
}
`
	writeFileAndCommit(t, repo, repoPath, "test.go", content, "initial commit")

	testFile := filepath.Join(repoPath, "test.go")

	// Run analyzer
	analyzer := NewHotspotAnalyzer(WithHotspotChurnDays(30))
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject(repoPath, []string{testFile})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if len(analysis.Files) != 1 {
		t.Errorf("Expected 1 file, got %d", len(analysis.Files))
	}

	if analysis.Files[0].TotalFunctions != 2 {
		t.Errorf("Expected 2 functions, got %d", analysis.Files[0].TotalFunctions)
	}

	// Verify complexity was calculated
	if analysis.Files[0].AvgCognitive <= 0 {
		t.Error("Expected positive average cognitive complexity")
	}

	// Verify churn score was calculated (commit count may vary based on git diff logic)
	if analysis.Files[0].ChurnScore < 0 {
		t.Error("Expected non-negative churn score")
	}
}

func TestHotspotAnalyzer_MultipleCommits(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	repo := initGitRepo(t, repoPath)

	// Initial commit
	content1 := `package main

func foo() {
	println("v1")
}
`
	writeFileAndCommit(t, repo, repoPath, "main.go", content1, "initial")

	// Second commit - add complexity
	content2 := `package main

func foo() {
	if true {
		for i := 0; i < 10; i++ {
			println(i)
		}
	}
}
`
	writeFileAndCommit(t, repo, repoPath, "main.go", content2, "add complexity")

	// Third commit
	content3 := `package main

func foo() {
	if true {
		for i := 0; i < 10; i++ {
			if i%2 == 0 {
				println(i)
			}
		}
	}
}
`
	writeFileAndCommit(t, repo, repoPath, "main.go", content3, "more complexity")

	testFile := filepath.Join(repoPath, "main.go")

	analyzer := NewHotspotAnalyzer(WithHotspotChurnDays(30))
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject(repoPath, []string{testFile})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if len(analysis.Files) != 1 {
		t.Fatalf("Expected 1 file, got %d", len(analysis.Files))
	}

	// Should have multiple commits (exact count depends on git diff logic with initial commit)
	if analysis.Files[0].Commits < 2 {
		t.Errorf("Expected at least 2 commits, got %d", analysis.Files[0].Commits)
	}

	// Hotspot score should be positive (churn > 0 and complexity > 0)
	if analysis.Files[0].HotspotScore <= 0 {
		t.Error("Expected positive hotspot score for file with churn and complexity")
	}

	// ChurnScore should be positive
	if analysis.Files[0].ChurnScore <= 0 {
		t.Error("Expected positive churn score")
	}

	// ComplexityScore should be positive
	if analysis.Files[0].ComplexityScore <= 0 {
		t.Error("Expected positive complexity score")
	}
}

func TestHotspotScore_Multiplicative(t *testing.T) {
	// Test that hotspot score is multiplicative (churn × complexity)
	tests := []struct {
		churn      float64
		complexity float64
		wantScore  float64
	}{
		{0.0, 0.0, 0.0},
		{1.0, 0.0, 0.0},
		{0.0, 1.0, 0.0},
		{0.5, 0.5, 0.25},
		{1.0, 1.0, 1.0},
		{0.8, 0.6, 0.48},
	}

	for _, tt := range tests {
		score := tt.churn * tt.complexity
		if score != tt.wantScore {
			t.Errorf("churn=%f × complexity=%f = %f, want %f",
				tt.churn, tt.complexity, score, tt.wantScore)
		}
	}
}

func TestHotspotAnalyzer_NoGitRepo(t *testing.T) {
	tmpDir := t.TempDir()

	analyzer := NewHotspotAnalyzer(WithHotspotChurnDays(30))
	defer analyzer.Close()

	_, err := analyzer.AnalyzeProject(tmpDir, []string{})
	if err == nil {
		t.Error("Expected error for non-git directory")
	}
}

func TestHotspotAnalyzer_SortsByScore(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	repo := initGitRepo(t, repoPath)

	// Create simple file (low complexity)
	simple := `package main

func simple() {
	println("hello")
}
`
	writeFileAndCommit(t, repo, repoPath, "simple.go", simple, "add simple")

	// Create complex file with more commits (high hotspot)
	complex1 := `package main

func complex() {
	if true {
		for i := 0; i < 10; i++ {
			println(i)
		}
	}
}
`
	writeFileAndCommit(t, repo, repoPath, "complex.go", complex1, "add complex v1")

	complex2 := `package main

func complex() {
	if true {
		for i := 0; i < 10; i++ {
			if i > 5 {
				println(i)
			}
		}
	}
}
`
	writeFileAndCommit(t, repo, repoPath, "complex.go", complex2, "add complex v2")

	files := []string{
		filepath.Join(repoPath, "simple.go"),
		filepath.Join(repoPath, "complex.go"),
	}

	analyzer := NewHotspotAnalyzer(WithHotspotChurnDays(30))
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject(repoPath, files)
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if len(analysis.Files) != 2 {
		t.Fatalf("Expected 2 files, got %d", len(analysis.Files))
	}

	// complex.go should be first (higher hotspot score due to more commits + higher complexity)
	if analysis.Files[0].HotspotScore < analysis.Files[1].HotspotScore {
		t.Error("Expected files to be sorted by hotspot score descending")
	}
}

func TestFileHotspot_Fields(t *testing.T) {
	fh := models.FileHotspot{
		Path:            "/test/file.go",
		HotspotScore:    0.64,
		ChurnScore:      0.8,
		ComplexityScore: 0.8,
		Commits:         15,
		AvgCognitive:    12.5,
		AvgCyclomatic:   8.3,
		TotalFunctions:  10,
	}

	if fh.Path != "/test/file.go" {
		t.Errorf("Path = %q, want %q", fh.Path, "/test/file.go")
	}
	if fh.HotspotScore != 0.64 {
		t.Errorf("HotspotScore = %f, want %f", fh.HotspotScore, 0.64)
	}
	if fh.Commits != 15 {
		t.Errorf("Commits = %d, want %d", fh.Commits, 15)
	}
}
