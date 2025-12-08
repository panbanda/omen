package commit

import (
	"testing"
	"time"

	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/stretchr/testify/assert"
)

func TestCalculateTrends(t *testing.T) {
	// Create mock commit analyses
	commits := []*CommitAnalysis{
		{
			CommitHash: "abc123",
			CommitDate: time.Now().Add(-48 * time.Hour),
			Complexity: &complexity.Analysis{
				Summary: complexity.Summary{
					TotalFiles:     10,
					TotalFunctions: 50,
					AvgCyclomatic:  5.0,
					AvgCognitive:   8.0,
				},
			},
		},
		{
			CommitHash: "def456",
			CommitDate: time.Now().Add(-24 * time.Hour),
			Complexity: &complexity.Analysis{
				Summary: complexity.Summary{
					TotalFiles:     12,
					TotalFunctions: 60,
					AvgCyclomatic:  5.5,
					AvgCognitive:   8.5,
				},
			},
		},
		{
			CommitHash: "ghi789",
			CommitDate: time.Now(),
			Complexity: &complexity.Analysis{
				Summary: complexity.Summary{
					TotalFiles:     15,
					TotalFunctions: 75,
					AvgCyclomatic:  6.0,
					AvgCognitive:   9.0,
				},
			},
		},
	}

	trends := CalculateTrends(commits)

	assert.Len(t, trends.AvgCyclomaticTrend.Points, 3)
	assert.Equal(t, 1.0, trends.AvgCyclomaticTrend.Delta) // 6.0 - 5.0
	assert.Equal(t, 1.0, trends.AvgCognitiveTrend.Delta)  // 9.0 - 8.0
	assert.Equal(t, 5.0, trends.TotalFilesTrend.Delta)    // 15 - 10
}
