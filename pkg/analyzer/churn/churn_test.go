package churn

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/go-git/go-git/v6"
	"github.com/go-git/go-git/v6/plumbing/object"
)

func TestNew(t *testing.T) {
	tests := []struct {
		name     string
		days     int
		wantDays int
	}{
		{
			name:     "Valid positive days",
			days:     90,
			wantDays: 90,
		},
		{
			name:     "Valid custom days",
			days:     30,
			wantDays: 30,
		},
		{
			name:     "Zero days defaults to 30",
			days:     0,
			wantDays: 30,
		},
		{
			name:     "Negative days defaults to 30",
			days:     -5,
			wantDays: 30,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analyzer := New(WithDays(tt.days))
			if analyzer == nil {
				t.Fatal("New() returned nil")
			}
			if analyzer.days != tt.wantDays {
				t.Errorf("analyzer.days = %v, want %v", analyzer.days, tt.wantDays)
			}
			if analyzer.spinner != nil {
				t.Error("analyzer.spinner should be nil by default")
			}
		})
	}
}

func TestAnalyzer_AnalyzeRepo(t *testing.T) {
	tests := []struct {
		name           string
		setupRepo      func(t *testing.T, dir string) string
		days           int
		wantFiles      int
		wantMinCommits int
		wantErr        bool
	}{
		{
			name: "Simple repository with commits",
			setupRepo: func(t *testing.T, dir string) string {
				repoPath := filepath.Join(dir, "repo")
				repo := initGitRepo(t, repoPath)

				writeFileAndCommit(t, repo, repoPath, "file1.go", "package main\n", "Initial commit")
				writeFileAndCommit(t, repo, repoPath, "file1.go", "package main\n\nfunc main() {}\n", "Add main function")

				return repoPath
			},
			days:           90,
			wantFiles:      1,
			wantMinCommits: 1,
			wantErr:        false,
		},
		{
			name: "Multiple files with different churn",
			setupRepo: func(t *testing.T, dir string) string {
				repoPath := filepath.Join(dir, "repo")
				repo := initGitRepo(t, repoPath)

				writeFileAndCommit(t, repo, repoPath, "file1.go", "package main\n", "Add file1")
				writeFileAndCommit(t, repo, repoPath, "file2.go", "package util\n", "Add file2")
				writeFileAndCommit(t, repo, repoPath, "file1.go", "package main\n\nfunc main() {}\n", "Update file1")
				writeFileAndCommit(t, repo, repoPath, "file1.go", "package main\n\nfunc main() {\n\tprintln(\"hello\")\n}\n", "Update file1 again")

				return repoPath
			},
			days:           90,
			wantFiles:      2,
			wantMinCommits: 1,
			wantErr:        false,
		},
		{
			name: "Repository with only initial commit",
			setupRepo: func(t *testing.T, dir string) string {
				repoPath := filepath.Join(dir, "repo")
				repo := initGitRepo(t, repoPath)
				writeFileAndCommit(t, repo, repoPath, "README.md", "# Project\n", "Initial commit")
				return repoPath
			},
			days:      90,
			wantFiles: 1, // initial commit is now counted with native git
			wantErr:   false,
		},
		{
			name: "Non-git directory",
			setupRepo: func(t *testing.T, dir string) string {
				nonGitPath := filepath.Join(dir, "notgit")
				if err := os.MkdirAll(nonGitPath, 0755); err != nil {
					t.Fatal(err)
				}
				return nonGitPath
			},
			days:    90,
			wantErr: true,
		},
		{
			name: "File deletions tracked",
			setupRepo: func(t *testing.T, dir string) string {
				repoPath := filepath.Join(dir, "repo")
				repo := initGitRepo(t, repoPath)

				writeFileAndCommit(t, repo, repoPath, "temp.go", "package temp\n", "Add temp file")
				deleteFileAndCommit(t, repo, repoPath, "temp.go", "Delete temp file")

				return repoPath
			},
			days:           90,
			wantFiles:      1,
			wantMinCommits: 1,
			wantErr:        false,
		},
		{
			name: "Multiple authors",
			setupRepo: func(t *testing.T, dir string) string {
				repoPath := filepath.Join(dir, "repo")
				repo := initGitRepo(t, repoPath)

				writeFileAndCommitWithAuthor(t, repo, repoPath, "shared.go", "package shared\n", "Add shared", "alice@example.com")
				writeFileAndCommitWithAuthor(t, repo, repoPath, "shared.go", "package shared\n\nfunc Helper() {}\n", "Update shared", "bob@example.com")

				return repoPath
			},
			days:           90,
			wantFiles:      1,
			wantMinCommits: 1,
			wantErr:        false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			repoPath := tt.setupRepo(t, tmpDir)

			analyzer := New(WithDays(tt.days))
			result, err := analyzer.Analyze(context.Background(), repoPath, nil)

			if (err != nil) != tt.wantErr {
				t.Fatalf("AnalyzeRepo() error = %v, wantErr %v", err, tt.wantErr)
			}

			if tt.wantErr {
				return
			}

			if result == nil {
				t.Fatal("AnalyzeRepo() returned nil result")
			}

			if len(result.Files) != tt.wantFiles {
				t.Errorf("Files count = %v, want %v", len(result.Files), tt.wantFiles)
			}

			if result.Summary.TotalFilesChanged != tt.wantFiles {
				t.Errorf("Summary.TotalFilesChanged = %v, want %v", result.Summary.TotalFilesChanged, tt.wantFiles)
			}

			if result.PeriodDays != tt.days {
				t.Errorf("PeriodDays = %v, want %v", result.PeriodDays, tt.days)
			}

			if result.RepositoryRoot == "" {
				t.Error("RepositoryRoot should not be empty")
			}

			if tt.wantFiles > 0 && result.Summary.TotalFileChanges < tt.wantMinCommits {
				t.Errorf("Summary.TotalFileChanges = %v, want >= %v", result.Summary.TotalFileChanges, tt.wantMinCommits)
			}
		})
	}
}

func TestAnalyzer_AnalyzeRepo_ChurnScores(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommit(t, repo, repoPath, "initial.go", "package init\n", "Initial commit")

	writeFileAndCommit(t, repo, repoPath, "low_churn.go", "package low\n", "Add low churn file")

	writeFileAndCommit(t, repo, repoPath, "high_churn.go", "package high\n", "Add high churn file")
	for i := 0; i < 5; i++ {
		content := strings.Repeat("// Line\n", i+1)
		writeFileAndCommit(t, repo, repoPath, "high_churn.go", content, "Update high churn")
	}

	analyzer := New(WithDays(90))
	result, err := analyzer.Analyze(context.Background(), repoPath, nil)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	if len(result.Files) < 2 {
		t.Fatalf("Expected at least 2 files, got %d", len(result.Files))
	}

	if result.Files[0].ChurnScore <= result.Files[1].ChurnScore {
		t.Error("Files should be sorted by churn score descending")
	}

	if result.Files[0].RelativePath != "high_churn.go" {
		t.Errorf("Highest churn file = %v, want high_churn.go", result.Files[0].RelativePath)
	}

	if result.Summary.MaxChurnScore != result.Files[0].ChurnScore {
		t.Errorf("MaxChurnScore = %v, want %v", result.Summary.MaxChurnScore, result.Files[0].ChurnScore)
	}

	for _, file := range result.Files {
		if file.ChurnScore < 0 || file.ChurnScore > 1 {
			t.Errorf("ChurnScore for %s = %v, should be in range [0, 1]", file.Path, file.ChurnScore)
		}
	}
}

func TestAnalyzer_AnalyzeRepo_DateFiltering(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommit(t, repo, repoPath, "recent.go", "package recent\n", "Recent change")

	time.Sleep(10 * time.Millisecond)

	writeFileAndCommit(t, repo, repoPath, "recent.go", "package recent\n\nfunc New() {}\n", "Another recent change")

	analyzer := New(WithDays(1))
	result, err := analyzer.Analyze(context.Background(), repoPath, nil)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	if len(result.Files) == 0 {
		t.Error("Expected files within date range")
	}

	for _, file := range result.Files {
		if file.LastCommit.Before(time.Now().AddDate(0, 0, -1)) {
			t.Errorf("File %s has LastCommit before date range", file.Path)
		}
	}
}

func TestAnalyzer_AnalyzeRepo_MultipleAuthors(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommitWithAuthor(t, repo, repoPath, "initial.go", "package init\n", "Initial", "Init Author")
	writeFileAndCommitWithAuthor(t, repo, repoPath, "shared.go", "package shared\n", "Author 1", "Alice")
	writeFileAndCommitWithAuthor(t, repo, repoPath, "shared.go", "package shared\n\nfunc A() {}\n", "Author 2", "Bob")
	writeFileAndCommitWithAuthor(t, repo, repoPath, "shared.go", "package shared\n\nfunc A() {}\nfunc B() {}\n", "Author 3", "Charlie")

	analyzer := New(WithDays(90))
	result, err := analyzer.Analyze(context.Background(), repoPath, nil)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	var sharedFile *FileMetrics
	for i := range result.Files {
		if result.Files[i].RelativePath == "shared.go" {
			sharedFile = &result.Files[i]
			break
		}
	}

	if sharedFile == nil {
		t.Fatal("shared.go not found in results")
	}

	if len(sharedFile.UniqueAuthors) != 3 {
		t.Errorf("UniqueAuthors = %v, want 3", len(sharedFile.UniqueAuthors))
	}

	if len(sharedFile.AuthorCounts) != 3 {
		t.Errorf("AuthorCounts map length = %v, want 3", len(sharedFile.AuthorCounts))
	}

	expectedAuthors := []string{"Alice", "Bob", "Charlie"}
	for _, author := range expectedAuthors {
		if _, ok := sharedFile.AuthorCounts[author]; !ok {
			t.Errorf("Expected author '%s' in AuthorCounts map, got %v", author, sharedFile.AuthorCounts)
		}
	}
}

func TestAnalyzer_AnalyzeRepo_LineCounts(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n", "Initial")
	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n\nfunc Add() {\n}\n", "Add function")
	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n\nfunc Sub() {\n}\n", "Replace with different function")

	analyzer := New(WithDays(90))
	result, err := analyzer.Analyze(context.Background(), repoPath, nil)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	if len(result.Files) != 1 {
		t.Fatalf("Expected 1 file, got %d", len(result.Files))
	}

	file := result.Files[0]
	if file.LinesAdded == 0 {
		t.Error("Expected some lines added")
	}

	if file.LinesDeleted == 0 {
		t.Error("Expected some lines deleted")
	}

	if result.Summary.TotalAdditions == 0 {
		t.Error("Summary should have TotalAdditions > 0")
	}

	if result.Summary.TotalDeletions == 0 {
		t.Error("Summary should have TotalDeletions > 0")
	}
}

func TestAnalyzer_AnalyzeRepo_SummaryStatistics(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommit(t, repo, repoPath, "file1.go", "package file1\n", "Add file1")
	writeFileAndCommit(t, repo, repoPath, "file2.go", "package file2\n", "Add file2")
	writeFileAndCommit(t, repo, repoPath, "file3.go", "package file3\n", "Add file3")

	for i := 0; i < 3; i++ {
		writeFileAndCommit(t, repo, repoPath, "file1.go", strings.Repeat("// Update\n", i+1), "Update file1")
	}

	analyzer := New(WithDays(90))
	result, err := analyzer.Analyze(context.Background(), repoPath, nil)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	if result.Summary.AvgCommitsPerFile == 0 {
		t.Error("AvgCommitsPerFile should not be 0")
	}

	expectedAvg := float64(result.Summary.TotalFileChanges) / float64(result.Summary.TotalFilesChanged)
	if result.Summary.AvgCommitsPerFile != expectedAvg {
		t.Errorf("AvgCommitsPerFile = %v, want %v", result.Summary.AvgCommitsPerFile, expectedAvg)
	}

	if result.Summary.MeanChurnScore == 0 {
		t.Error("MeanChurnScore should not be 0")
	}

	if result.Summary.P50ChurnScore == 0 {
		t.Error("P50ChurnScore should not be 0")
	}

	// Verify files are sorted by churn score (top churned files)
	for i := 1; i < len(result.Files); i++ {
		if result.Files[i-1].ChurnScore < result.Files[i].ChurnScore {
			t.Error("Files should be sorted by churn score descending")
		}
	}
}

func TestAnalyzer_AnalyzeFiles(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommit(t, repo, repoPath, "file1.go", "package file1\n", "Add file1")
	writeFileAndCommit(t, repo, repoPath, "file2.go", "package file2\n", "Add file2")
	writeFileAndCommit(t, repo, repoPath, "file3.go", "package file3\n", "Add file3")

	writeFileAndCommit(t, repo, repoPath, "file1.go", "package file1\n\nfunc F1() {}\n", "Update file1")
	writeFileAndCommit(t, repo, repoPath, "file2.go", "package file2\n\nfunc F2() {}\n", "Update file2")

	tests := []struct {
		name      string
		files     []string
		wantFiles int
	}{
		{
			name:      "Single file",
			files:     []string{"file1.go"},
			wantFiles: 1,
		},
		{
			name:      "Multiple files",
			files:     []string{"file1.go", "file2.go"},
			wantFiles: 2,
		},
		{
			name:      "Absolute path",
			files:     []string{filepath.Join(repoPath, "file1.go")},
			wantFiles: 1,
		},
		{
			name:      "Non-existent file",
			files:     []string{"nonexistent.go"},
			wantFiles: 0,
		},
		{
			name:      "Mixed existing and non-existing",
			files:     []string{"file1.go", "nonexistent.go"},
			wantFiles: 1,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analyzer := New(WithDays(90))
			result, err := analyzer.Analyze(context.Background(), repoPath, tt.files)
			if err != nil {
				t.Fatalf("AnalyzeFiles() error = %v", err)
			}

			if len(result.Files) != tt.wantFiles {
				t.Errorf("Files count = %v, want %v", len(result.Files), tt.wantFiles)
			}

			if result.Summary.TotalFilesChanged != tt.wantFiles {
				t.Errorf("Summary.TotalFilesChanged = %v, want %v", result.Summary.TotalFilesChanged, tt.wantFiles)
			}

			if result.RepositoryRoot == "" {
				t.Error("RepositoryRoot should not be empty")
			}
		})
	}
}

func TestAnalyzer_AnalyzeRepo_FirstLastCommit(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	startTime := time.Now()

	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n", "First commit")
	time.Sleep(10 * time.Millisecond)
	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n\nfunc A() {}\n", "Second commit")
	time.Sleep(10 * time.Millisecond)
	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n\nfunc A() {}\nfunc B() {}\n", "Third commit")

	analyzer := New(WithDays(90))
	result, err := analyzer.Analyze(context.Background(), repoPath, nil)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	if len(result.Files) != 1 {
		t.Fatalf("Expected 1 file, got %d", len(result.Files))
	}

	file := result.Files[0]

	if file.FirstCommit.Before(startTime.Add(-1 * time.Second)) {
		t.Error("FirstCommit should be after test start time")
	}

	if file.LastCommit.Before(file.FirstCommit) {
		t.Error("LastCommit should be after or equal to FirstCommit")
	}

	if file.FirstCommit.After(file.LastCommit) {
		t.Error("FirstCommit should be before or equal to LastCommit")
	}
}

func TestCountLines(t *testing.T) {
	tests := []struct {
		name    string
		content string
		want    int
	}{
		{
			name:    "Empty string",
			content: "",
			want:    0,
		},
		{
			name:    "Single line",
			content: "line1\n",
			want:    1,
		},
		{
			name:    "Multiple lines",
			content: "line1\nline2\nline3\n",
			want:    3,
		},
		{
			name:    "No trailing newline",
			content: "line1\nline2",
			want:    1,
		},
		{
			name:    "Only newlines",
			content: "\n\n\n",
			want:    3,
		},
		{
			name:    "Windows line endings",
			content: "line1\r\nline2\r\n",
			want:    2,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := countLines(tt.content)
			if got != tt.want {
				t.Errorf("countLines() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestFileMetrics_CalculateChurnScore(t *testing.T) {
	tests := []struct {
		name       string
		commits    int
		added      int
		deleted    int
		maxCommits int
		maxChanges int
		wantMin    float64
		wantMax    float64
	}{
		{
			name:       "Zero values",
			commits:    0,
			added:      0,
			deleted:    0,
			maxCommits: 100,
			maxChanges: 1000,
			wantMin:    0,
			wantMax:    0,
		},
		{
			name:       "Max values",
			commits:    100,
			added:      500,
			deleted:    500,
			maxCommits: 100,
			maxChanges: 1000,
			wantMin:    1.0,
			wantMax:    1.0,
		},
		{
			name:       "Half values",
			commits:    50,
			added:      250,
			deleted:    250,
			maxCommits: 100,
			maxChanges: 1000,
			wantMin:    0.4,
			wantMax:    0.6,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			fm := &FileMetrics{
				Commits:      tt.commits,
				LinesAdded:   tt.added,
				LinesDeleted: tt.deleted,
			}
			got := fm.CalculateChurnScoreWithMax(tt.maxCommits, tt.maxChanges)
			if got < tt.wantMin || got > tt.wantMax {
				t.Errorf("CalculateChurnScoreWithMax() = %v, want between %v and %v", got, tt.wantMin, tt.wantMax)
			}
		})
	}
}

func TestFileMetrics_IsHotspot(t *testing.T) {
	tests := []struct {
		name      string
		score     float64
		threshold float64
		want      bool
	}{
		{"Above threshold", 0.8, 0.5, true},
		{"At threshold", 0.5, 0.5, true},
		{"Below threshold", 0.3, 0.5, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			fm := &FileMetrics{ChurnScore: tt.score}
			if got := fm.IsHotspot(tt.threshold); got != tt.want {
				t.Errorf("IsHotspot() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestSummary_CalculateStatistics(t *testing.T) {
	files := []FileMetrics{
		{ChurnScore: 0.2},
		{ChurnScore: 0.4},
		{ChurnScore: 0.6},
		{ChurnScore: 0.8},
	}

	s := NewSummary()
	s.CalculateStatistics(files)

	if s.MeanChurnScore != 0.5 {
		t.Errorf("MeanChurnScore = %v, want 0.5", s.MeanChurnScore)
	}

	if s.P50ChurnScore == 0 {
		t.Error("P50ChurnScore should not be 0")
	}
}

func TestSummary_IdentifyHotspotAndStableFiles(t *testing.T) {
	files := []FileMetrics{
		{Path: "hot1.go", ChurnScore: 0.9},
		{Path: "hot2.go", ChurnScore: 0.7},
		{Path: "mid.go", ChurnScore: 0.3, Commits: 1},
		{Path: "stable.go", ChurnScore: 0.05, Commits: 1},
	}

	s := NewSummary()
	s.IdentifyHotspotAndStableFiles(files)

	if len(s.HotspotFiles) != 2 {
		t.Errorf("HotspotFiles count = %d, want 2", len(s.HotspotFiles))
	}

	if len(s.StableFiles) != 1 {
		t.Errorf("StableFiles count = %d, want 1", len(s.StableFiles))
	}
}

// TestSummary_TotalFileChanges_NotTotalCommits verifies the field name is clear.
// TotalFileChanges represents the sum of file touches across all commits,
// NOT the count of unique commits. This test ensures the misleading name is fixed.
func TestSummary_TotalFileChanges_NotTotalCommits(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	// Create commits touching multiple files
	// Commit 1: add file1 (file change: +1)
	// Commit 2: add file2 (file change: +1)
	// Commit 3: update file1 (file change: +1)
	// Commit 4: update file2 (file change: +1)
	// Total file changes: 4
	writeFileAndCommit(t, repo, repoPath, "file1.go", "package a\n", "Commit 1: add file1")
	writeFileAndCommit(t, repo, repoPath, "file2.go", "package b\n", "Commit 2: add file2")
	writeFileAndCommit(t, repo, repoPath, "file1.go", "package a\n\nfunc A() {}\n", "Commit 3: update file1")
	writeFileAndCommit(t, repo, repoPath, "file2.go", "package b\n\nfunc B() {}\n", "Commit 4: update file2")

	analyzer := New(WithDays(90))
	result, err := analyzer.Analyze(context.Background(), repoPath, nil)
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	// The field is named TotalFileChanges (not TotalCommits) to clarify it's the
	// sum of file touches across commits, not the count of unique commits.
	// All 4 commits are counted (including initial commit).
	if result.Summary.TotalFileChanges != 4 {
		t.Errorf("Summary.TotalFileChanges = %d, want 4", result.Summary.TotalFileChanges)
	}

	// Verify it differs from number of unique files (2) and unique commits (4)
	if result.Summary.TotalFilesChanged != 2 {
		t.Errorf("Summary.TotalFilesChanged = %d, want 2", result.Summary.TotalFilesChanged)
	}
}

// Helper functions

func initGitRepo(t *testing.T, path string) *git.Repository {
	t.Helper()

	repo, err := git.PlainInit(path, false)
	if err != nil {
		t.Fatalf("Failed to init git repo: %v", err)
	}

	return repo
}

func writeFileAndCommit(t *testing.T, repo *git.Repository, repoPath, filename, content, message string) {
	t.Helper()
	writeFileAndCommitWithAuthor(t, repo, repoPath, filename, content, message, "Test Author")
}

func writeFileAndCommitWithAuthor(t *testing.T, repo *git.Repository, repoPath, filename, content, message, authorName string) {
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
			Name:  authorName,
			Email: authorName + "@example.com",
			When:  time.Now(),
		},
	})
	if err != nil {
		t.Fatalf("Failed to commit: %v", err)
	}
}

func deleteFileAndCommit(t *testing.T, repo *git.Repository, repoPath, filename, message string) {
	t.Helper()

	filePath := filepath.Join(repoPath, filename)
	if err := os.Remove(filePath); err != nil {
		t.Fatalf("Failed to delete file %s: %v", filename, err)
	}

	w, err := repo.Worktree()
	if err != nil {
		t.Fatalf("Failed to get worktree: %v", err)
	}

	if _, err := w.Remove(filename); err != nil {
		t.Fatalf("Failed to remove file %s from git: %v", filename, err)
	}

	_, err = w.Commit(message, &git.CommitOptions{
		Author: &object.Signature{
			Name:  "Test Author",
			Email: "test@example.com",
			When:  time.Now(),
		},
	})
	if err != nil {
		t.Fatalf("Failed to commit deletion: %v", err)
	}
}
