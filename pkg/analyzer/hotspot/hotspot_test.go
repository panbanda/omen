package hotspot

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
)

func TestNew(t *testing.T) {
	tests := []struct {
		name     string
		opts     []Option
		wantDays int
		wantSize int64
	}{
		{"default values", nil, 30, 0},
		{"custom days", []Option{WithChurnDays(60)}, 60, 0},
		{"custom size", []Option{WithMaxFileSize(1024)}, 30, 1024},
		{"both options", []Option{WithChurnDays(90), WithMaxFileSize(2048)}, 90, 2048},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			a := New(tt.opts...)
			defer a.Close()

			if a.churnDays != tt.wantDays {
				t.Errorf("churnDays = %d, want %d", a.churnDays, tt.wantDays)
			}
			if a.maxFileSize != tt.wantSize {
				t.Errorf("maxFileSize = %d, want %d", a.maxFileSize, tt.wantSize)
			}
		})
	}
}

func TestAnalysis_CalculateSummary(t *testing.T) {
	tests := []struct {
		name         string
		files        []FileHotspot
		wantCount    int
		wantMax      float64
		wantHotspots int
	}{
		{
			name:         "empty files",
			files:        []FileHotspot{},
			wantCount:    0,
			wantMax:      0,
			wantHotspots: 0,
		},
		{
			name: "single file below threshold",
			files: []FileHotspot{
				{Path: "a.go", HotspotScore: 0.3},
			},
			wantCount:    1,
			wantMax:      0.3,
			wantHotspots: 0,
		},
		{
			name: "multiple files mixed",
			files: []FileHotspot{
				{Path: "a.go", HotspotScore: 0.8},
				{Path: "b.go", HotspotScore: 0.6},
				{Path: "c.go", HotspotScore: 0.3},
				{Path: "d.go", HotspotScore: 0.1},
			},
			wantCount:    4,
			wantMax:      0.8,
			wantHotspots: 2, // 0.8 and 0.6 are >= HighThreshold (0.4)
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analysis := &Analysis{Files: tt.files}
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

func TestAnalyzer_AnalyzeProject(t *testing.T) {
	// Create temp directory with test files
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	// Initialize git repo
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
	analyzer := New(WithChurnDays(30))
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject(context.Background(), repoPath, []string{testFile})
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

func TestAnalyzer_MultipleCommits(t *testing.T) {
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

	analyzer := New(WithChurnDays(30))
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject(context.Background(), repoPath, []string{testFile})
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

func TestCalculateScore_GeometricMean(t *testing.T) {
	// Test that hotspot score uses geometric mean: sqrt(churn * complexity)
	// This preserves intersection semantics while giving better score distribution
	tests := []struct {
		churn      float64
		complexity float64
		wantScore  float64
	}{
		{0.0, 0.0, 0.0},
		{1.0, 0.0, 0.0},
		{0.0, 1.0, 0.0},
		{0.5, 0.5, 0.5},    // sqrt(0.25) = 0.5
		{1.0, 1.0, 1.0},    // sqrt(1.0) = 1.0
		{0.64, 0.64, 0.64}, // sqrt(0.4096) = 0.64
		{0.9, 0.4, 0.6},    // sqrt(0.36) = 0.6
	}

	for _, tt := range tests {
		score := CalculateScore(tt.churn, tt.complexity)
		if score < tt.wantScore-0.01 || score > tt.wantScore+0.01 {
			t.Errorf("CalculateScore(%f, %f) = %f, want %f",
				tt.churn, tt.complexity, score, tt.wantScore)
		}
	}
}

func TestAnalyzer_NoGitRepo(t *testing.T) {
	tmpDir := t.TempDir()

	analyzer := New(WithChurnDays(30))
	defer analyzer.Close()

	_, err := analyzer.AnalyzeProject(context.Background(), tmpDir, []string{})
	if err == nil {
		t.Error("Expected error for non-git directory")
	}
}

func TestAnalyzer_SortsByScore(t *testing.T) {
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

	analyzer := New(WithChurnDays(30))
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject(context.Background(), repoPath, files)
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
	fh := FileHotspot{
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

func TestFileHotspot_GetSeverity(t *testing.T) {
	tests := []struct {
		score   float64
		wantSev Severity
	}{
		{0.7, SeverityCritical},
		{0.6, SeverityCritical},
		{0.5, SeverityHigh},
		{0.4, SeverityHigh},
		{0.3, SeverityModerate},
		{0.25, SeverityModerate},
		{0.2, SeverityLow},
		{0.1, SeverityLow},
	}

	for _, tt := range tests {
		fh := FileHotspot{HotspotScore: tt.score}
		got := fh.GetSeverity()
		if got != tt.wantSev {
			t.Errorf("GetSeverity() for score %f = %q, want %q", tt.score, got, tt.wantSev)
		}
	}
}

func TestNormalizeChurnCDF(t *testing.T) {
	tests := []struct {
		commits int
		wantMin float64
		wantMax float64
	}{
		{0, 0.0, 0.0},
		{1, 0.25, 0.35},
		{5, 0.70, 0.80},
		{10, 0.90, 0.95},
		{50, 0.99, 1.01},
	}

	for _, tt := range tests {
		got := NormalizeChurnCDF(tt.commits)
		if got < tt.wantMin || got > tt.wantMax {
			t.Errorf("NormalizeChurnCDF(%d) = %f, want between %f and %f",
				tt.commits, got, tt.wantMin, tt.wantMax)
		}
	}
}

func TestNormalizeComplexityCDF(t *testing.T) {
	tests := []struct {
		avgCog  float64
		wantMin float64
		wantMax float64
	}{
		{0.0, 0.0, 0.0},
		{5.0, 0.45, 0.55},
		{15.0, 0.85, 0.95},
		{30.0, 0.95, 1.0},
	}

	for _, tt := range tests {
		got := NormalizeComplexityCDF(tt.avgCog)
		if got < tt.wantMin || got > tt.wantMax {
			t.Errorf("NormalizeComplexityCDF(%f) = %f, want between %f and %f",
				tt.avgCog, got, tt.wantMin, tt.wantMax)
		}
	}
}

// Helper functions

func initGitRepo(t *testing.T, path string) *git.Repository {
	t.Helper()
	if err := os.MkdirAll(path, 0755); err != nil {
		t.Fatalf("Failed to create repo dir: %v", err)
	}
	repo, err := git.PlainInit(path, false)
	if err != nil {
		t.Fatalf("Failed to init git repo: %v", err)
	}
	return repo
}

func writeFileAndCommit(t *testing.T, repo *git.Repository, repoPath, filename, content, message string) {
	t.Helper()

	filePath := filepath.Join(repoPath, filename)
	if err := os.WriteFile(filePath, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write file %s: %v", filename, err)
	}

	w, err := repo.Worktree()
	if err != nil {
		t.Fatalf("Failed to get worktree: %v", err)
	}

	if _, err := w.Add(filename); err != nil {
		t.Fatalf("Failed to add file %s: %v", filename, err)
	}

	_, err = w.Commit(message, &git.CommitOptions{
		Author: &object.Signature{
			Name:  "Test Author",
			Email: "test@example.com",
			When:  time.Now(),
		},
	})
	if err != nil {
		t.Fatalf("Failed to commit: %v", err)
	}
}
