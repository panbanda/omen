package commit

import (
	"github.com/panbanda/omen/pkg/analyzer/complexity"
)

// CommitAnalysis holds analysis results for a specific commit.
type CommitAnalysis struct {
	CommitHash string
	Complexity *complexity.Analysis
}
