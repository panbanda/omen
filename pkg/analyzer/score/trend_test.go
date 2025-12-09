package score

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/parser"
	"github.com/panbanda/omen/pkg/source"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// getFilesForCommit extracts the file list that trend analysis would use.
// This mirrors the logic in TrendAnalyzer.AnalyzeTrendWithProgress.
func getFilesForCommit(repoPath, commitSHA string) ([]string, error) {
	tree, err := vcs.GetTreeAtCommit(repoPath, commitSHA)
	if err != nil {
		return nil, err
	}

	entries, err := tree.Entries()
	if err != nil {
		return nil, err
	}

	// Use the same filtering as the trend analyzer
	var files []string
	for _, e := range entries {
		if !e.IsDir && parser.DetectLanguage(e.Path) != parser.LangUnknown && !shouldExcludeFromTrend(e.Path) {
			files = append(files, e.Path)
		}
	}

	return files, nil
}

func TestTrendAnalysis_RespectsDefaultExclusions(t *testing.T) {
	// This test verifies that trend analysis excludes vendor/ and node_modules/
	// using the same default patterns as the scanner.
	//
	// Bug scenario:
	// - `omen score` uses scanner.ScanDir() which excludes vendor/ by default
	// - `omen score trend` uses tree.Entries() which includes ALL committed files
	// - This causes file counts and scores to differ

	// Create a temporary git repository
	tmpDir := t.TempDir()

	// Initialize git repo
	runGit(t, tmpDir, "init")
	runGit(t, tmpDir, "config", "user.email", "test@example.com")
	runGit(t, tmpDir, "config", "user.name", "Test User")

	// NOTE: No .gitignore - the vendor/ files are COMMITTED to git
	// This matches repos where vendor/ files are committed to git

	// Create source files that should be analyzed
	writeFile(t, tmpDir, "main.go", `package main

func main() {
	println("hello")
}
`)
	writeFile(t, tmpDir, "util.go", `package main

func helper() string {
	return "help"
}
`)

	// Create vendor files that ARE COMMITTED but should be excluded by default patterns
	os.MkdirAll(filepath.Join(tmpDir, "vendor", "lib"), 0755)
	writeFile(t, tmpDir, "vendor/lib/code.go", `package lib

func vendorFunc() {
	// This is vendor code that should be excluded
}
`)

	// Create node_modules files that ARE COMMITTED but should be excluded
	os.MkdirAll(filepath.Join(tmpDir, "node_modules", "pkg"), 0755)
	writeFile(t, tmpDir, "node_modules/pkg/index.js", `
function npmCode() {
	// This is npm code that should be excluded
}
`)

	// Commit everything - force add vendor/node_modules to bypass any global gitignore
	runGit(t, tmpDir, "add", "--force", ".")
	runGit(t, tmpDir, "commit", "-m", "Initial commit")

	// Get the files that trend analysis would analyze
	out, err := exec.Command("git", "-C", tmpDir, "rev-parse", "HEAD").Output()
	require.NoError(t, err)
	commitSHA := string(out[:len(out)-1])

	files, err := getFilesForCommit(tmpDir, commitSHA)
	require.NoError(t, err)

	// The key assertion: vendor/ and node_modules/ should be excluded
	// Expected: only main.go and util.go (2 files)
	// Bug behavior: includes vendor/lib/code.go and node_modules/pkg/index.js (4 files)
	assert.Len(t, files, 2, "should exclude vendor/ and node_modules/ files")

	for _, f := range files {
		assert.NotContains(t, f, "vendor/", "should exclude vendor/ files")
		assert.NotContains(t, f, "node_modules/", "should exclude node_modules/ files")
	}
}

func TestTrendAnalysis_ConsistentWithFilesystem(t *testing.T) {
	// Create a temporary git repository
	tmpDir := t.TempDir()

	// Initialize git repo
	runGit(t, tmpDir, "init")
	runGit(t, tmpDir, "config", "user.email", "test@example.com")
	runGit(t, tmpDir, "config", "user.name", "Test User")

	// Create .gitignore
	writeFile(t, tmpDir, ".gitignore", "ignored/\n")

	// Create source files
	writeFile(t, tmpDir, "main.go", `package main

func main() {
	x := 1
	if x > 0 {
		println("positive")
	}
}

func helper() int {
	return 42
}
`)

	// Create ignored directory with duplicated code
	os.MkdirAll(filepath.Join(tmpDir, "ignored"), 0755)
	writeFile(t, tmpDir, "ignored/dup.go", `package ignored

func main() {
	x := 1
	if x > 0 {
		println("positive")
	}
}

func helper() int {
	return 42
}
`)

	runGit(t, tmpDir, "add", "-A")
	runGit(t, tmpDir, "commit", "-m", "Initial commit")

	// Run filesystem-based score analysis
	fsFiles := []string{filepath.Join(tmpDir, "main.go")}
	fsAnalyzer := New()
	fsResult, err := fsAnalyzer.Analyze(context.Background(), fsFiles, source.NewFilesystem(), "")
	require.NoError(t, err)

	// Run git-tree based trend analysis
	trendAnalyzer := NewTrendAnalyzer(
		WithTrendPeriod("monthly"),
		WithTrendSince(24*time.Hour), // Look back 1 day
	)

	trendResult, err := trendAnalyzer.AnalyzeTrend(context.Background(), tmpDir)
	require.NoError(t, err)
	require.NotEmpty(t, trendResult.Points, "should have at least one trend point")

	// The scores should be consistent
	// If they're very different, it indicates the bug where tree analysis
	// includes files that should be ignored
	lastTrendPoint := trendResult.Points[len(trendResult.Points)-1]

	// Allow some variance due to different file counts, but not massive differences
	// A 50-point difference in duplication score indicates a bug
	assert.InDelta(t, fsResult.Components.Duplication, lastTrendPoint.Components.Duplication, 20,
		"duplication scores should be consistent between filesystem and git-tree analysis")

	assert.InDelta(t, fsResult.Score, lastTrendPoint.Score, 15,
		"overall scores should be consistent between filesystem and git-tree analysis")
}

func runGit(t *testing.T, dir string, args ...string) {
	t.Helper()
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %v failed: %v\n%s", args, err, out)
	}
}

func writeFile(t *testing.T, dir, name, content string) {
	t.Helper()
	path := filepath.Join(dir, name)
	err := os.MkdirAll(filepath.Dir(path), 0755)
	require.NoError(t, err)
	err = os.WriteFile(path, []byte(content), 0644)
	require.NoError(t, err)
}
