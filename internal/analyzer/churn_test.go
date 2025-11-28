package analyzer

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/panbanda/omen/pkg/models"
)

func TestNewChurnAnalyzer(t *testing.T) {
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
			analyzer := NewChurnAnalyzer(tt.days)
			if analyzer == nil {
				t.Fatal("NewChurnAnalyzer() returned nil")
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

func TestChurnAnalyzer_AnalyzeRepo(t *testing.T) {
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
			name: "Empty repository",
			setupRepo: func(t *testing.T, dir string) string {
				repoPath := filepath.Join(dir, "repo")
				initGitRepo(t, repoPath)
				return repoPath
			},
			days:      90,
			wantFiles: 0,
			wantErr:   true,
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
			wantFiles: 0,
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

			analyzer := NewChurnAnalyzer(tt.days)
			result, err := analyzer.AnalyzeRepo(repoPath)

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

			if result.Summary.TotalFiles != tt.wantFiles {
				t.Errorf("Summary.TotalFiles = %v, want %v", result.Summary.TotalFiles, tt.wantFiles)
			}

			if result.Days != tt.days {
				t.Errorf("Days = %v, want %v", result.Days, tt.days)
			}

			if result.RepoPath != repoPath {
				t.Errorf("RepoPath = %v, want %v", result.RepoPath, repoPath)
			}

			if tt.wantFiles > 0 && result.Summary.TotalCommits < tt.wantMinCommits {
				t.Errorf("Summary.TotalCommits = %v, want >= %v", result.Summary.TotalCommits, tt.wantMinCommits)
			}
		})
	}
}

func TestChurnAnalyzer_AnalyzeRepo_ChurnScores(t *testing.T) {
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

	analyzer := NewChurnAnalyzer(90)
	result, err := analyzer.AnalyzeRepo(repoPath)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	if len(result.Files) < 2 {
		t.Fatalf("Expected at least 2 files, got %d", len(result.Files))
	}

	if result.Files[0].ChurnScore <= result.Files[1].ChurnScore {
		t.Error("Files should be sorted by churn score descending")
	}

	if result.Files[0].Path != "high_churn.go" {
		t.Errorf("Highest churn file = %v, want high_churn.go", result.Files[0].Path)
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

func TestChurnAnalyzer_AnalyzeRepo_DateFiltering(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommit(t, repo, repoPath, "recent.go", "package recent\n", "Recent change")

	time.Sleep(10 * time.Millisecond)

	writeFileAndCommit(t, repo, repoPath, "recent.go", "package recent\n\nfunc New() {}\n", "Another recent change")

	analyzer := NewChurnAnalyzer(1)
	result, err := analyzer.AnalyzeRepo(repoPath)
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

func TestChurnAnalyzer_AnalyzeRepo_MultipleAuthors(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommitWithAuthor(t, repo, repoPath, "initial.go", "package init\n", "Initial", "init@example.com")
	writeFileAndCommitWithAuthor(t, repo, repoPath, "shared.go", "package shared\n", "Author 1", "alice@example.com")
	writeFileAndCommitWithAuthor(t, repo, repoPath, "shared.go", "package shared\n\nfunc A() {}\n", "Author 2", "bob@example.com")
	writeFileAndCommitWithAuthor(t, repo, repoPath, "shared.go", "package shared\n\nfunc A() {}\nfunc B() {}\n", "Author 3", "charlie@example.com")

	analyzer := NewChurnAnalyzer(90)
	result, err := analyzer.AnalyzeRepo(repoPath)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	var sharedFile *models.FileChurnMetrics
	for i := range result.Files {
		if result.Files[i].Path == "shared.go" {
			sharedFile = &result.Files[i]
			break
		}
	}

	if sharedFile == nil {
		t.Fatal("shared.go not found in results")
	}

	if sharedFile.UniqueAuthors != 3 {
		t.Errorf("UniqueAuthors = %v, want 3", sharedFile.UniqueAuthors)
	}

	if len(sharedFile.Authors) != 3 {
		t.Errorf("Authors map length = %v, want 3", len(sharedFile.Authors))
	}

	expectedAuthors := []string{"alice@example.com", "bob@example.com", "charlie@example.com"}
	for _, author := range expectedAuthors {
		if _, ok := sharedFile.Authors[author]; !ok {
			t.Errorf("Expected author %s in Authors map", author)
		}
	}
}

func TestChurnAnalyzer_AnalyzeRepo_LineCounts(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n", "Initial")
	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n\nfunc Add() {\n}\n", "Add function")
	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n\nfunc Sub() {\n}\n", "Replace with different function")

	analyzer := NewChurnAnalyzer(90)
	result, err := analyzer.AnalyzeRepo(repoPath)
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

	if result.Summary.TotalLinesAdded == 0 {
		t.Error("Summary should have TotalLinesAdded > 0")
	}

	if result.Summary.TotalLinesDeleted == 0 {
		t.Error("Summary should have TotalLinesDeleted > 0")
	}
}

func TestChurnAnalyzer_AnalyzeRepo_SummaryStatistics(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommit(t, repo, repoPath, "file1.go", "package file1\n", "Add file1")
	writeFileAndCommit(t, repo, repoPath, "file2.go", "package file2\n", "Add file2")
	writeFileAndCommit(t, repo, repoPath, "file3.go", "package file3\n", "Add file3")

	for i := 0; i < 3; i++ {
		writeFileAndCommit(t, repo, repoPath, "file1.go", strings.Repeat("// Update\n", i+1), "Update file1")
	}

	analyzer := NewChurnAnalyzer(90)
	result, err := analyzer.AnalyzeRepo(repoPath)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	if result.Summary.AvgCommitsPerFile == 0 {
		t.Error("AvgCommitsPerFile should not be 0")
	}

	expectedAvg := float64(result.Summary.TotalCommits) / float64(result.Summary.TotalFiles)
	if result.Summary.AvgCommitsPerFile != expectedAvg {
		t.Errorf("AvgCommitsPerFile = %v, want %v", result.Summary.AvgCommitsPerFile, expectedAvg)
	}

	if result.Summary.MeanChurnScore == 0 {
		t.Error("MeanChurnScore should not be 0")
	}

	if result.Summary.P50ChurnScore == 0 {
		t.Error("P50ChurnScore should not be 0")
	}

	topN := 10
	if len(result.Files) < topN {
		topN = len(result.Files)
	}
	if len(result.Summary.TopChurnedFiles) != topN {
		t.Errorf("TopChurnedFiles length = %v, want %v", len(result.Summary.TopChurnedFiles), topN)
	}

	for i := 0; i < topN; i++ {
		if result.Summary.TopChurnedFiles[i] != result.Files[i].Path {
			t.Errorf("TopChurnedFiles[%d] = %v, want %v", i, result.Summary.TopChurnedFiles[i], result.Files[i].Path)
		}
	}
}

func TestChurnAnalyzer_AnalyzeFiles(t *testing.T) {
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
			analyzer := NewChurnAnalyzer(90)
			result, err := analyzer.AnalyzeFiles(repoPath, tt.files)
			if err != nil {
				t.Fatalf("AnalyzeFiles() error = %v", err)
			}

			if len(result.Files) != tt.wantFiles {
				t.Errorf("Files count = %v, want %v", len(result.Files), tt.wantFiles)
			}

			if result.Summary.TotalFiles != tt.wantFiles {
				t.Errorf("Summary.TotalFiles = %v, want %v", result.Summary.TotalFiles, tt.wantFiles)
			}

			if result.RepoPath != repoPath {
				t.Errorf("RepoPath = %v, want %v", result.RepoPath, repoPath)
			}
		})
	}
}

func TestChurnAnalyzer_AnalyzeFiles_SummaryRecalculation(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	writeFileAndCommit(t, repo, repoPath, "high.go", "package high\n", "Add high")
	writeFileAndCommit(t, repo, repoPath, "low.go", "package low\n", "Add low")

	for i := 0; i < 5; i++ {
		writeFileAndCommit(t, repo, repoPath, "high.go", strings.Repeat("// Line\n", i+1), "Update high")
	}

	analyzer := NewChurnAnalyzer(90)
	fullResult, err := analyzer.AnalyzeRepo(repoPath)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	filteredResult, err := analyzer.AnalyzeFiles(repoPath, []string{"high.go"})
	if err != nil {
		t.Fatalf("AnalyzeFiles() error = %v", err)
	}

	if len(filteredResult.Files) != 1 {
		t.Fatalf("Expected 1 file, got %d", len(filteredResult.Files))
	}

	if filteredResult.Summary.TotalCommits >= fullResult.Summary.TotalCommits {
		t.Error("Filtered result should have fewer commits than full analysis")
	}

	if filteredResult.Summary.TotalFiles != 1 {
		t.Errorf("Filtered TotalFiles = %v, want 1", filteredResult.Summary.TotalFiles)
	}

	expectedAvg := float64(filteredResult.Summary.TotalCommits) / float64(filteredResult.Summary.TotalFiles)
	if filteredResult.Summary.AvgCommitsPerFile != expectedAvg {
		t.Errorf("Filtered AvgCommitsPerFile = %v, want %v", filteredResult.Summary.AvgCommitsPerFile, expectedAvg)
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

func TestChurnAnalyzer_AnalyzeRepo_FirstLastCommit(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepo(t, repoPath)

	startTime := time.Now()

	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n", "First commit")
	time.Sleep(10 * time.Millisecond)
	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n\nfunc A() {}\n", "Second commit")
	time.Sleep(10 * time.Millisecond)
	writeFileAndCommit(t, repo, repoPath, "test.go", "package test\n\nfunc A() {}\nfunc B() {}\n", "Third commit")

	analyzer := NewChurnAnalyzer(90)
	result, err := analyzer.AnalyzeRepo(repoPath)
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

func TestChurnAnalyzer_SetSpinner(t *testing.T) {
	analyzer := NewChurnAnalyzer(90)

	if analyzer.spinner != nil {
		t.Error("spinner should be nil initially")
	}
}

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
	writeFileAndCommitWithAuthor(t, repo, repoPath, filename, content, message, "test@example.com")
}

func writeFileAndCommitWithAuthor(t *testing.T, repo *git.Repository, repoPath, filename, content, message, email string) {
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
			Email: email,
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

func BenchmarkAnalyzeRepo_SmallRepo(b *testing.B) {
	tmpDir := b.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepoForBench(b, repoPath)

	for i := 0; i < 5; i++ {
		filename := filepath.Join("file", string(rune('a'+i))+".go")
		writeFileAndCommitForBench(b, repo, repoPath, filename, "package main\n", "Add file")
	}

	analyzer := NewChurnAnalyzer(90)

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := analyzer.AnalyzeRepo(repoPath)
		if err != nil {
			b.Fatalf("AnalyzeRepo() error = %v", err)
		}
	}
}

func BenchmarkAnalyzeRepo_ManyCommits(b *testing.B) {
	tmpDir := b.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")
	repo := initGitRepoForBench(b, repoPath)

	for i := 0; i < 50; i++ {
		content := strings.Repeat("// Line\n", i+1)
		writeFileAndCommitForBench(b, repo, repoPath, "main.go", content, "Update")
	}

	analyzer := NewChurnAnalyzer(90)

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := analyzer.AnalyzeRepo(repoPath)
		if err != nil {
			b.Fatalf("AnalyzeRepo() error = %v", err)
		}
	}
}

func BenchmarkCountLines(b *testing.B) {
	content := strings.Repeat("line of text\n", 1000)

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = countLines(content)
	}
}

func initGitRepoForBench(b *testing.B, path string) *git.Repository {
	b.Helper()

	repo, err := git.PlainInit(path, false)
	if err != nil {
		b.Fatalf("Failed to init git repo: %v", err)
	}

	return repo
}

func writeFileAndCommitForBench(b *testing.B, repo *git.Repository, repoPath, filename, content, message string) {
	b.Helper()

	filePath := filepath.Join(repoPath, filename)
	if err := os.MkdirAll(filepath.Dir(filePath), 0755); err != nil {
		b.Fatalf("Failed to create directory: %v", err)
	}

	if err := os.WriteFile(filePath, []byte(content), 0644); err != nil {
		b.Fatalf("Failed to write file: %v", err)
	}

	w, err := repo.Worktree()
	if err != nil {
		b.Fatalf("Failed to get worktree: %v", err)
	}

	if _, err := w.Add(filename); err != nil {
		b.Fatalf("Failed to add file: %v", err)
	}

	_, err = w.Commit(message, &git.CommitOptions{
		Author: &object.Signature{
			Name:  "Bench",
			Email: "bench@example.com",
			When:  time.Now(),
		},
	})
	if err != nil {
		b.Fatalf("Failed to commit: %v", err)
	}
}
