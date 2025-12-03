//go:build integration

package score_test

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/analyzer/score"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestAnalyzeProject_Integration(t *testing.T) {
	// Find the repository root by looking for go.mod
	wd, err := os.Getwd()
	require.NoError(t, err)

	repoRoot := wd
	for {
		if _, err := os.Stat(filepath.Join(repoRoot, "go.mod")); err == nil {
			break
		}
		parent := filepath.Dir(repoRoot)
		if parent == repoRoot {
			t.Skip("could not find repository root")
		}
		repoRoot = parent
	}

	// Discover files in the pkg/analyzer/score directory
	files, err := discoverGoFiles(wd)
	require.NoError(t, err)
	require.NotEmpty(t, files, "should find Go files in score package")

	analyzer := score.New()

	result, err := analyzer.AnalyzeProject(context.Background(), repoRoot, files)
	require.NoError(t, err)
	require.NotNil(t, result)

	// Verify composite score is in valid range
	assert.GreaterOrEqual(t, result.Score, 0)
	assert.LessOrEqual(t, result.Score, 100)

	// Verify grade is assigned
	assert.NotEmpty(t, result.Grade)

	// Verify all component scores are in valid range
	assert.GreaterOrEqual(t, result.Components.Complexity, 0)
	assert.LessOrEqual(t, result.Components.Complexity, 100)

	assert.GreaterOrEqual(t, result.Components.Duplication, 0)
	assert.LessOrEqual(t, result.Components.Duplication, 100)

	assert.GreaterOrEqual(t, result.Components.Defect, 0)
	assert.LessOrEqual(t, result.Components.Defect, 100)

	assert.GreaterOrEqual(t, result.Components.Debt, 0)
	assert.LessOrEqual(t, result.Components.Debt, 100)

	assert.GreaterOrEqual(t, result.Components.Coupling, 0)
	assert.LessOrEqual(t, result.Components.Coupling, 100)

	assert.GreaterOrEqual(t, result.Components.Smells, 0)
	assert.LessOrEqual(t, result.Components.Smells, 100)

	// Cohesion is reported separately
	assert.GreaterOrEqual(t, result.Cohesion, 0)
	assert.LessOrEqual(t, result.Cohesion, 100)

	// Verify weights sum to 1.0
	weights := result.Weights
	sum := weights.Complexity + weights.Duplication + weights.Defect +
		weights.Debt + weights.Coupling + weights.Smells
	assert.InDelta(t, 1.0, sum, 0.001, "weights should sum to 1.0")

	// Verify files were analyzed
	assert.Greater(t, result.FilesAnalyzed, 0)

	// Verify timestamp is set
	assert.False(t, result.Timestamp.IsZero())

	t.Logf("Score: %d/100 (%s)", result.Score, result.Grade)
	t.Logf("Components: complexity=%d, duplication=%d, defect=%d, debt=%d, coupling=%d, smells=%d",
		result.Components.Complexity, result.Components.Duplication, result.Components.Defect,
		result.Components.Debt, result.Components.Coupling, result.Components.Smells)
	t.Logf("Cohesion: %d", result.Cohesion)
	t.Logf("Files analyzed: %d", result.FilesAnalyzed)
}

func TestAnalyzeProject_WithThresholds(t *testing.T) {
	wd, err := os.Getwd()
	require.NoError(t, err)

	repoRoot := wd
	for {
		if _, err := os.Stat(filepath.Join(repoRoot, "go.mod")); err == nil {
			break
		}
		parent := filepath.Dir(repoRoot)
		if parent == repoRoot {
			t.Skip("could not find repository root")
		}
		repoRoot = parent
	}

	files, err := discoverGoFiles(wd)
	require.NoError(t, err)

	// Set very high thresholds that will likely fail
	thresholds := score.Thresholds{
		Score:       99,
		Complexity:  99,
		Duplication: 99,
		Defect:      99,
		Debt:        99,
		Coupling:    99,
		Smells:      99,
	}

	analyzer := score.New(score.WithThresholds(thresholds))

	result, err := analyzer.AnalyzeProject(context.Background(), repoRoot, files)
	require.NoError(t, err)

	// With such high thresholds, at least one should fail
	assert.False(t, result.Passed, "should fail with very high thresholds")

	// Check that threshold results are populated
	hasFailure := false
	for _, tr := range result.Thresholds {
		if tr.Min > 0 && !tr.Passed {
			hasFailure = true
			break
		}
	}
	assert.True(t, hasFailure, "should have at least one threshold failure")
}

func TestAnalyzeProject_WithProgress(t *testing.T) {
	wd, err := os.Getwd()
	require.NoError(t, err)

	repoRoot := wd
	for {
		if _, err := os.Stat(filepath.Join(repoRoot, "go.mod")); err == nil {
			break
		}
		parent := filepath.Dir(repoRoot)
		if parent == repoRoot {
			t.Skip("could not find repository root")
		}
		repoRoot = parent
	}

	files, err := discoverGoFiles(wd)
	require.NoError(t, err)

	analyzer := score.New()

	var progressCalls int
	progressFn := func(stage string) {
		progressCalls++
		t.Logf("Progress: %s", stage)
	}

	result, err := analyzer.AnalyzeProjectWithProgress(context.Background(), repoRoot, files, progressFn)
	require.NoError(t, err)
	require.NotNil(t, result)

	// Progress should have been called for each analyzer
	assert.Greater(t, progressCalls, 0, "progress function should have been called")
}

func discoverGoFiles(dir string) ([]string, error) {
	var files []string
	err := filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() {
			return nil
		}
		if filepath.Ext(path) == ".go" {
			files = append(files, path)
		}
		return nil
	})
	return files, err
}
