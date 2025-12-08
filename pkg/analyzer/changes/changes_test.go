package changes

import (
	"context"
	"testing"
	"time"

	"github.com/go-git/go-git/v6/plumbing"
	"github.com/go-git/go-git/v6/plumbing/object"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/internal/vcs/mocks"
	"github.com/panbanda/omen/pkg/stats"
	"github.com/stretchr/testify/mock"
)

func TestIsBugFixCommit(t *testing.T) {
	tests := []struct {
		message string
		isFix   bool
	}{
		{"fix: resolve null pointer exception", true},
		{"Fix bug in login handler", true},
		{"fixes #123", true},
		{"bugfix: handle edge case", true},
		{"patch security vulnerability", true},
		{"resolve race condition", true},
		{"closes #456", true},
		{"Fixed memory leak", true},
		{"Fixing typo", true},
		{"Error handling improvement", true},
		{"crash fix for iOS", true},
		{"defect: validation missing", true},
		{"issue: timeout not respected", true},
		{"feat: add new feature", false},
		{"refactor: clean up code", false},
		{"docs: update README", false},
		{"chore: update dependencies", false},
		{"test: add unit tests", false},
		{"style: format code", false},
	}

	for _, tt := range tests {
		t.Run(tt.message, func(t *testing.T) {
			result := isBugFixCommit(tt.message)
			if result != tt.isFix {
				t.Errorf("isBugFixCommit(%q) = %v, expected %v", tt.message, result, tt.isFix)
			}
		})
	}
}

func TestCalculateEntropy(t *testing.T) {
	tests := []struct {
		name          string
		linesPerFile  map[string]int
		expectedRange [2]float64 // min, max
	}{
		{
			name:          "empty",
			linesPerFile:  map[string]int{},
			expectedRange: [2]float64{0, 0},
		},
		{
			name: "single file",
			linesPerFile: map[string]int{
				"file.go": 100,
			},
			expectedRange: [2]float64{0, 0}, // entropy = 0 for single file
		},
		{
			name: "equal distribution - 2 files",
			linesPerFile: map[string]int{
				"a.go": 50,
				"b.go": 50,
			},
			expectedRange: [2]float64{0.9, 1.1}, // should be ~1.0 (log2(2))
		},
		{
			name: "unequal distribution",
			linesPerFile: map[string]int{
				"a.go": 90,
				"b.go": 10,
			},
			expectedRange: [2]float64{0.4, 0.5}, // lower entropy
		},
		{
			name: "equal distribution - 4 files",
			linesPerFile: map[string]int{
				"a.go": 25,
				"b.go": 25,
				"c.go": 25,
				"d.go": 25,
			},
			expectedRange: [2]float64{1.9, 2.1}, // should be ~2.0 (log2(4))
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			entropy := CalculateEntropy(tt.linesPerFile)
			if entropy < tt.expectedRange[0] || entropy > tt.expectedRange[1] {
				t.Errorf("CalculateEntropy() = %f, expected in range [%f, %f]",
					entropy, tt.expectedRange[0], tt.expectedRange[1])
			}
		})
	}
}

func TestCalculateRisk(t *testing.T) {
	weights := DefaultWeights()
	norm := NormalizationStats{
		MaxLinesAdded:       1000,
		MaxLinesDeleted:     500,
		MaxNumFiles:         50,
		MaxUniqueChanges:    100,
		MaxNumDevelopers:    10,
		MaxAuthorExperience: 50,
		MaxEntropy:          4.0,
	}

	tests := []struct {
		name          string
		features      CommitFeatures
		expectedRange [2]float64
	}{
		{
			name: "low risk - small change, experienced author",
			features: CommitFeatures{
				IsFix:            false,
				LinesAdded:       10,
				LinesDeleted:     5,
				NumFiles:         1,
				UniqueChanges:    5,
				NumDevelopers:    1,
				AuthorExperience: 50, // max experience
				Entropy:          0,
			},
			expectedRange: [2]float64{0, 0.2},
		},
		{
			name: "high risk - bug fix, large change, new author",
			features: CommitFeatures{
				IsFix:            true,
				LinesAdded:       1000,
				LinesDeleted:     500,
				NumFiles:         50,
				UniqueChanges:    100,
				NumDevelopers:    10,
				AuthorExperience: 0, // no experience
				Entropy:          4.0,
			},
			expectedRange: [2]float64{0.8, 1.0},
		},
		{
			name: "medium risk - moderate change",
			features: CommitFeatures{
				IsFix:            false,
				LinesAdded:       200,
				LinesDeleted:     100,
				NumFiles:         10,
				UniqueChanges:    20,
				NumDevelopers:    3,
				AuthorExperience: 25,
				Entropy:          2.0,
			},
			expectedRange: [2]float64{0.2, 0.35},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			score := CalculateRisk(tt.features, weights, norm)
			if score < tt.expectedRange[0] || score > tt.expectedRange[1] {
				t.Errorf("CalculateRisk() = %f, expected in range [%f, %f]",
					score, tt.expectedRange[0], tt.expectedRange[1])
			}
		})
	}
}

func TestGetRiskLevel(t *testing.T) {
	// Test with percentile-based thresholds
	thresholds := RiskThresholds{
		HighThreshold:   0.7,
		MediumThreshold: 0.4,
	}

	tests := []struct {
		score    float64
		expected RiskLevel
	}{
		{0.0, RiskLevelLow},
		{0.39, RiskLevelLow},
		{0.4, RiskLevelMedium},
		{0.69, RiskLevelMedium},
		{0.7, RiskLevelHigh},
		{1.0, RiskLevelHigh},
	}

	for _, tt := range tests {
		result := GetRiskLevel(tt.score, thresholds)
		if result != tt.expected {
			t.Errorf("GetRiskLevel(%f) = %s, expected %s", tt.score, result, tt.expected)
		}
	}
}

func TestDefaultRiskThresholds(t *testing.T) {
	thresholds := DefaultRiskThresholds()

	// Verify thresholds are sensible
	if thresholds.HighThreshold <= thresholds.MediumThreshold {
		t.Errorf("HighThreshold (%f) should be > MediumThreshold (%f)",
			thresholds.HighThreshold, thresholds.MediumThreshold)
	}
	if thresholds.MediumThreshold <= 0 {
		t.Errorf("MediumThreshold should be > 0, got %f", thresholds.MediumThreshold)
	}
	if thresholds.HighThreshold > 1.0 {
		t.Errorf("HighThreshold should be <= 1.0, got %f", thresholds.HighThreshold)
	}
}

func TestDefaultWeights(t *testing.T) {
	weights := DefaultWeights()

	// Verify weights sum to 1.0
	sum := weights.FIX + weights.Entropy + weights.LA + weights.NUC +
		weights.NF + weights.LD + weights.NDEV + weights.EXP

	if sum < 0.99 || sum > 1.01 {
		t.Errorf("Changes weights sum to %f, expected 1.0", sum)
	}

	// Verify specific weights from requirements
	if weights.FIX != 0.25 {
		t.Errorf("FIX weight = %f, expected 0.25", weights.FIX)
	}
	if weights.Entropy != 0.20 {
		t.Errorf("Entropy weight = %f, expected 0.20", weights.Entropy)
	}
	if weights.LA != 0.20 {
		t.Errorf("LA weight = %f, expected 0.20", weights.LA)
	}
}

func TestTruncateMessage(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{"Short message", "Short message"},
		{"Line one\nLine two", "Line one"},
		{
			"This is a very long commit message that exceeds the eighty character limit for display purposes",
			"This is a very long commit message that exceeds the eighty character limit fo...",
		},
		{"  Whitespace  \n", "Whitespace"},
	}

	for _, tt := range tests {
		result := truncateMessage(tt.input)
		if result != tt.expected {
			t.Errorf("truncateMessage(%q) = %q, expected %q", tt.input, result, tt.expected)
		}
	}
}

func TestSafeNormalize(t *testing.T) {
	tests := []struct {
		value    float64
		max      float64
		expected float64
	}{
		{0, 100, 0},
		{50, 100, 0.5},
		{100, 100, 1.0},
		{150, 100, 1.0}, // clamped
		{50, 0, 0},      // zero max
		{50, -10, 0},    // negative max
	}

	for _, tt := range tests {
		result := safeNormalize(tt.value, tt.max)
		if result != tt.expected {
			t.Errorf("safeNormalize(%f, %f) = %f, expected %f",
				tt.value, tt.max, result, tt.expected)
		}
	}
}

func TestGenerateRecommendations(t *testing.T) {
	features := CommitFeatures{
		IsFix: true,
	}
	factors := map[string]float64{
		"entropy":    0.20,
		"experience": 0.05,
	}
	score := 0.75

	recs := GenerateRecommendations(features, score, factors)

	if len(recs) == 0 {
		t.Error("Expected recommendations for high-risk commit")
	}

	// Should include bug fix recommendation
	found := false
	for _, rec := range recs {
		if rec == "Bug fix commit - ensure comprehensive testing of the fix" {
			found = true
			break
		}
	}
	if !found {
		t.Error("Expected bug fix recommendation")
	}
}

// TestChangesAnalyzer_AuthorExperience_TemporalOrder verifies that author experience
// is calculated correctly based on temporal order.
func TestAnalyzer_AuthorExperience_TemporalOrder(t *testing.T) {
	baseTime := time.Date(2024, 1, 1, 12, 0, 0, 0, time.UTC)

	// Create 4 commits by Alice, newest-first (simulating git log order)
	commits := []struct {
		hash      string
		author    string
		message   string
		timestamp time.Time
	}{
		{"hash4", "Alice", "Alice commit 4", baseTime.Add(3 * time.Hour)}, // newest
		{"hash3", "Alice", "Alice commit 3", baseTime.Add(2 * time.Hour)},
		{"hash2", "Alice", "Alice commit 2", baseTime.Add(1 * time.Hour)},
		{"hash1", "Alice", "Alice commit 1", baseTime}, // oldest
	}

	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)

	mockOpener.EXPECT().PlainOpen(mock.Anything).Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.Anything).Return(mockIter, nil)

	// Set up iterator to return commits in git log order (newest first)
	idx := 0
	mockIter.EXPECT().ForEach(mock.Anything).RunAndReturn(func(fn func(vcs.Commit) error) error {
		for idx < len(commits) {
			c := commits[idx]
			idx++
			mockCommit := createMockCommit(t, c.hash, c.author, c.message, c.timestamp, 1)
			if err := fn(mockCommit); err != nil {
				return err
			}
		}
		return nil
	})
	mockIter.EXPECT().Close().Return()

	analyzer := New(
		WithOpener(mockOpener),
		WithDays(90),
		WithReferenceTime(baseTime.Add(24*time.Hour)),
	)

	result, err := analyzer.Analyze(context.Background(), "/fake/path", nil)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	if len(result.Commits) != 4 {
		t.Fatalf("Expected 4 commits, got %d", len(result.Commits))
	}

	// Find commit 1 (oldest) and commit 4 (newest) by message
	var commit1, commit4 *CommitRisk
	for i := range result.Commits {
		c := &result.Commits[i]
		switch c.Message {
		case "Alice commit 1":
			commit1 = c
		case "Alice commit 4":
			commit4 = c
		}
	}

	if commit1 == nil || commit4 == nil {
		t.Fatalf("Could not find expected commits")
	}

	// Experience factor = (1.0 - normalized_exp) * weight
	// First commit (commit 1): AuthorExperience=0, factor = (1.0 - 0) * 0.05 = 0.05 (HIGH)
	// Last commit (commit 4): AuthorExperience=3, factor = (1.0 - 1) * 0.05 = 0 (LOW)
	firstExpFactor := commit1.ContributingFactors["experience"]
	lastExpFactor := commit4.ContributingFactors["experience"]

	t.Logf("Commit 1 (first, oldest): experience factor = %.4f", firstExpFactor)
	t.Logf("Commit 4 (last, newest): experience factor = %.4f", lastExpFactor)

	if firstExpFactor <= lastExpFactor {
		t.Errorf("Temporal ordering bug: first commit has experience factor %.4f, "+
			"last commit has %.4f - first should be HIGHER (less experience = more risk)",
			firstExpFactor, lastExpFactor)
	}
}

// TestAnalyzer_NumDevelopers_TemporalOrder verifies that the number of developers
// who previously touched a file is tracked correctly over time.
func TestAnalyzer_NumDevelopers_TemporalOrder(t *testing.T) {
	baseTime := time.Date(2024, 1, 1, 12, 0, 0, 0, time.UTC)

	// 4 different authors modify shared.go in sequence, git log order (newest first)
	commits := []struct {
		hash      string
		author    string
		message   string
		timestamp time.Time
		file      string
	}{
		{"hash4", "Dave", "Dave modifies shared.go", baseTime.Add(3 * time.Hour), "shared.go"}, // newest
		{"hash3", "Charlie", "Charlie modifies shared.go", baseTime.Add(2 * time.Hour), "shared.go"},
		{"hash2", "Bob", "Bob modifies shared.go", baseTime.Add(1 * time.Hour), "shared.go"},
		{"hash1", "Alice", "Alice creates shared.go", baseTime, "shared.go"}, // oldest
	}

	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)

	mockOpener.EXPECT().PlainOpen(mock.Anything).Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.Anything).Return(mockIter, nil)

	idx := 0
	mockIter.EXPECT().ForEach(mock.Anything).RunAndReturn(func(fn func(vcs.Commit) error) error {
		for idx < len(commits) {
			c := commits[idx]
			idx++
			mockCommit := createMockCommitWithFile(t, c.hash, c.author, c.message, c.timestamp, c.file)
			if err := fn(mockCommit); err != nil {
				return err
			}
		}
		return nil
	})
	mockIter.EXPECT().Close().Return()

	analyzer := New(
		WithOpener(mockOpener),
		WithDays(90),
		WithReferenceTime(baseTime.Add(24*time.Hour)),
	)

	result, err := analyzer.Analyze(context.Background(), "/fake/path", nil)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	// Find Alice's commit (first) and Dave's commit (last)
	var aliceCommit, daveCommit *CommitRisk
	for i := range result.Commits {
		c := &result.Commits[i]
		switch c.Author {
		case "Alice":
			aliceCommit = c
		case "Dave":
			daveCommit = c
		}
	}

	if aliceCommit == nil || daveCommit == nil {
		t.Fatalf("Could not find expected commits")
	}

	// Alice's commit (first): NDEV=0 (no prior developers)
	// Dave's commit (last): NDEV=3 (Alice, Bob, Charlie touched it before)
	aliceNDEV := aliceCommit.ContributingFactors["num_developers"]
	daveNDEV := daveCommit.ContributingFactors["num_developers"]

	t.Logf("Alice's commit (first): num_developers factor = %.4f", aliceNDEV)
	t.Logf("Dave's commit (last): num_developers factor = %.4f", daveNDEV)

	if aliceNDEV >= daveNDEV {
		t.Errorf("Temporal ordering bug: first commit has num_developers factor %.4f, "+
			"last commit has %.4f - first should be LOWER (fewer prior developers)",
			aliceNDEV, daveNDEV)
	}
}

// TestAnalyzer_UniqueChanges_TemporalOrder verifies that the count of prior commits
// to touched files (NUC) increases over time.
func TestAnalyzer_UniqueChanges_TemporalOrder(t *testing.T) {
	baseTime := time.Date(2024, 1, 1, 12, 0, 0, 0, time.UTC)

	// Same author makes 4 commits to main.go, git log order (newest first)
	commits := []struct {
		hash      string
		author    string
		message   string
		timestamp time.Time
		file      string
	}{
		{"hash4", "Dev", "Update 4", baseTime.Add(3 * time.Hour), "main.go"}, // newest
		{"hash3", "Dev", "Update 3", baseTime.Add(2 * time.Hour), "main.go"},
		{"hash2", "Dev", "Update 2", baseTime.Add(1 * time.Hour), "main.go"},
		{"hash1", "Dev", "Update 1", baseTime, "main.go"}, // oldest
	}

	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)

	mockOpener.EXPECT().PlainOpen(mock.Anything).Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.Anything).Return(mockIter, nil)

	idx := 0
	mockIter.EXPECT().ForEach(mock.Anything).RunAndReturn(func(fn func(vcs.Commit) error) error {
		for idx < len(commits) {
			c := commits[idx]
			idx++
			mockCommit := createMockCommitWithFile(t, c.hash, c.author, c.message, c.timestamp, c.file)
			if err := fn(mockCommit); err != nil {
				return err
			}
		}
		return nil
	})
	mockIter.EXPECT().Close().Return()

	analyzer := New(
		WithOpener(mockOpener),
		WithDays(90),
		WithReferenceTime(baseTime.Add(24*time.Hour)),
	)

	result, err := analyzer.Analyze(context.Background(), "/fake/path", nil)
	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}

	// Find Update 1 (first) and Update 4 (last) by message
	var firstCommit, lastCommit *CommitRisk
	for i := range result.Commits {
		c := &result.Commits[i]
		switch c.Message {
		case "Update 1":
			firstCommit = c
		case "Update 4":
			lastCommit = c
		}
	}

	if firstCommit == nil || lastCommit == nil {
		t.Fatalf("Could not find expected commits")
	}

	// First commit: NUC=0 (no prior commits to main.go)
	// Last commit: NUC=3 (3 prior commits to main.go)
	firstNUC := firstCommit.ContributingFactors["unique_changes"]
	lastNUC := lastCommit.ContributingFactors["unique_changes"]

	t.Logf("First commit: unique_changes factor = %.4f", firstNUC)
	t.Logf("Last commit: unique_changes factor = %.4f", lastNUC)

	if firstNUC >= lastNUC {
		t.Errorf("Temporal ordering bug: first commit has unique_changes factor %.4f, "+
			"last commit has %.4f - first should be LOWER (fewer prior commits)",
			firstNUC, lastNUC)
	}
}

func TestNew(t *testing.T) {
	t.Run("default options", func(t *testing.T) {
		analyzer := New()
		defer analyzer.Close()

		if analyzer.days != 30 {
			t.Errorf("days = %d, want 30", analyzer.days)
		}
	})

	t.Run("with days", func(t *testing.T) {
		analyzer := New(WithDays(60))
		defer analyzer.Close()

		if analyzer.days != 60 {
			t.Errorf("days = %d, want 60", analyzer.days)
		}
	})

	t.Run("with custom weights", func(t *testing.T) {
		weights := Weights{FIX: 0.5}
		analyzer := New(WithWeights(weights))
		defer analyzer.Close()

		if analyzer.weights.FIX != 0.5 {
			t.Errorf("FIX weight = %f, want 0.5", analyzer.weights.FIX)
		}
	})
}

func TestIsAutomatedCommit(t *testing.T) {
	tests := []struct {
		message     string
		isAutomated bool
	}{
		{"chore: updated image tag", true},
		{"[skip ci] minor update", true},
		{"Merge pull request #123", true},
		{"chore(deps): bump version", true},
		{"ci: update workflow", true},
		{"docs: update readme", true},
		{"style: format code", true},
		{"feat: add new feature", false},
		{"fix: resolve bug", false},
	}

	for _, tt := range tests {
		t.Run(tt.message, func(t *testing.T) {
			result := isAutomatedCommit(tt.message)
			if result != tt.isAutomated {
				t.Errorf("isAutomatedCommit(%q) = %v, expected %v", tt.message, result, tt.isAutomated)
			}
		})
	}
}

func TestSafeNormalizeInt(t *testing.T) {
	tests := []struct {
		value    int
		max      int
		expected float64
	}{
		{0, 100, 0},
		{50, 100, 0.5},
		{100, 100, 1.0},
		{150, 100, 1.0}, // clamped
		{50, 0, 0},      // zero max
	}

	for _, tt := range tests {
		result := safeNormalizeInt(tt.value, tt.max)
		if result != tt.expected {
			t.Errorf("safeNormalizeInt(%d, %d) = %f, expected %f",
				tt.value, tt.max, result, tt.expected)
		}
	}
}

func TestCalculateNormalizationStats(t *testing.T) {
	commits := []CommitFeatures{
		{LinesAdded: 100, LinesDeleted: 50, NumFiles: 5, UniqueChanges: 10, NumDevelopers: 2, AuthorExperience: 5, Entropy: 1.5},
		{LinesAdded: 200, LinesDeleted: 30, NumFiles: 10, UniqueChanges: 20, NumDevelopers: 3, AuthorExperience: 10, Entropy: 2.0},
	}

	stats := calculateNormalizationStats(commits)

	if stats.MaxLinesAdded != 200 {
		t.Errorf("MaxLinesAdded = %d, want 200", stats.MaxLinesAdded)
	}
	if stats.MaxLinesDeleted != 50 {
		t.Errorf("MaxLinesDeleted = %d, want 50", stats.MaxLinesDeleted)
	}
	if stats.MaxNumFiles != 10 {
		t.Errorf("MaxNumFiles = %d, want 10", stats.MaxNumFiles)
	}
}

func TestPercentile(t *testing.T) {
	tests := []struct {
		name   string
		sorted []float64
		p      int
		want   float64
	}{
		{"empty", []float64{}, 50, 0},
		{"single", []float64{5.0}, 50, 5.0},
		{"median", []float64{1.0, 2.0, 3.0, 4.0, 5.0}, 50, 3.0},
		{"high percentile", []float64{1.0, 2.0, 3.0, 4.0, 5.0}, 95, 5.0},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := stats.Percentile(tt.sorted, tt.p)
			if got != tt.want {
				t.Errorf("stats.Percentile() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestBoolToFloat(t *testing.T) {
	if boolToFloat(true) != 1.0 {
		t.Error("boolToFloat(true) should be 1.0")
	}
	if boolToFloat(false) != 0.0 {
		t.Error("boolToFloat(false) should be 0.0")
	}
}

func TestNewAnalysis(t *testing.T) {
	analysis := NewAnalysis()

	if analysis.Commits == nil {
		t.Error("Commits should be initialized")
	}
	if analysis.GeneratedAt.IsZero() {
		t.Error("GeneratedAt should be set")
	}
}

// Helper to create a mock commit with minimal file changes
func createMockCommit(t *testing.T, hash, author, message string, timestamp time.Time, numParents int) *mocks.MockCommit {
	mockCommit := mocks.NewMockCommit(t)
	mockParent := mocks.NewMockCommit(t)
	mockTree := mocks.NewMockTree(t)
	mockParentTree := mocks.NewMockTree(t)

	mockCommit.EXPECT().Hash().Return(plumbing.NewHash(hash)).Maybe()
	mockCommit.EXPECT().Author().Return(object.Signature{Name: author, When: timestamp}).Maybe()
	mockCommit.EXPECT().Message().Return(message).Maybe()
	mockCommit.EXPECT().NumParents().Return(numParents).Maybe()

	if numParents > 0 {
		mockCommit.EXPECT().Parent(0).Return(mockParent, nil).Maybe()
		mockParent.EXPECT().Tree().Return(mockParentTree, nil).Maybe()
		mockCommit.EXPECT().Tree().Return(mockTree, nil).Maybe()
		mockParentTree.EXPECT().Diff(mockTree).Return([]vcs.Change{}, nil).Maybe()
	}

	return mockCommit
}

// Helper to create a mock commit that modifies a specific file
func createMockCommitWithFile(t *testing.T, hash, author, message string, timestamp time.Time, file string) *mocks.MockCommit {
	mockCommit := mocks.NewMockCommit(t)
	mockParent := mocks.NewMockCommit(t)
	mockTree := mocks.NewMockTree(t)
	mockParentTree := mocks.NewMockTree(t)
	mockChange := mocks.NewMockChange(t)
	mockPatch := mocks.NewMockPatch(t)

	mockCommit.EXPECT().Hash().Return(plumbing.NewHash(hash)).Maybe()
	mockCommit.EXPECT().Author().Return(object.Signature{Name: author, When: timestamp}).Maybe()
	mockCommit.EXPECT().Message().Return(message).Maybe()
	mockCommit.EXPECT().NumParents().Return(1).Maybe()
	mockCommit.EXPECT().Parent(0).Return(mockParent, nil).Maybe()
	mockParent.EXPECT().Tree().Return(mockParentTree, nil).Maybe()
	mockCommit.EXPECT().Tree().Return(mockTree, nil).Maybe()

	mockChange.EXPECT().ToName().Return(file).Maybe()
	mockChange.EXPECT().FromName().Return(file).Maybe()
	mockChange.EXPECT().Patch().Return(mockPatch, nil).Maybe()
	mockPatch.EXPECT().FilePatches().Return([]vcs.FilePatch{}).Maybe()

	mockParentTree.EXPECT().Diff(mockTree).Return([]vcs.Change{mockChange}, nil).Maybe()

	return mockCommit
}
