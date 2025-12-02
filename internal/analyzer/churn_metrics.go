package analyzer

import (
	"bufio"
	"os"
	"sort"
	"strings"
	"time"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/models"
)

// processCommit extracts churn data from a single commit.
func (a *ChurnAnalyzer) processCommit(commit vcs.Commit, fileMetrics map[string]*models.FileChurnMetrics) error {
	if commit.NumParents() == 0 {
		return nil
	}

	parent, err := commit.Parent(0)
	if err != nil {
		return nil
	}

	parentTree, err := parent.Tree()
	if err != nil {
		return nil
	}

	commitTree, err := commit.Tree()
	if err != nil {
		return nil
	}

	changes, err := parentTree.Diff(commitTree)
	if err != nil {
		return nil
	}

	for _, change := range changes {
		relativePath := change.ToName()
		if relativePath == "" {
			relativePath = change.FromName() // Deleted file
		}

		if _, exists := fileMetrics[relativePath]; !exists {
			fileMetrics[relativePath] = &models.FileChurnMetrics{
				Path:         "./" + relativePath, // pmat prefixes with ./
				RelativePath: relativePath,
				AuthorCounts: make(map[string]int),
				FirstCommit:  commit.Author().When,
				LastCommit:   commit.Author().When,
			}
		}

		fm := fileMetrics[relativePath]
		fm.Commits++
		fm.AuthorCounts[commit.Author().Name]++

		if commit.Author().When.Before(fm.FirstCommit) {
			fm.FirstCommit = commit.Author().When
		}
		if commit.Author().When.After(fm.LastCommit) {
			fm.LastCommit = commit.Author().When
		}

		patch, err := change.Patch()
		if err == nil {
			for _, filePatch := range patch.FilePatches() {
				for _, chunk := range filePatch.Chunks() {
					content := chunk.Content()
					switch chunk.Type() {
					case vcs.ChunkAdd:
						fm.LinesAdded += countLines(content)
					case vcs.ChunkDelete:
						fm.LinesDeleted += countLines(content)
					}
				}
			}
		}
	}

	return nil
}

// buildChurnAnalysis constructs the final analysis from collected metrics.
func buildChurnAnalysis(fileMetrics map[string]*models.FileChurnMetrics, absPath string, days int) *models.ChurnAnalysis {
	analysis := &models.ChurnAnalysis{
		GeneratedAt:    time.Now().UTC(),
		PeriodDays:     days,
		RepositoryRoot: absPath,
		Files:          make([]models.FileChurnMetrics, 0, len(fileMetrics)),
		Summary:        models.NewChurnSummary(),
	}

	// Find max values for normalization
	var maxCommits, maxChanges int
	for _, fm := range fileMetrics {
		if fm.Commits > maxCommits {
			maxCommits = fm.Commits
		}
		changes := fm.LinesAdded + fm.LinesDeleted
		if changes > maxChanges {
			maxChanges = changes
		}
	}

	// Calculate scores and collect stats
	var totalCommits, totalAdded, totalDeleted int
	now := time.Now()

	for _, fm := range fileMetrics {
		fm.UniqueAuthors = make([]string, 0, len(fm.AuthorCounts))
		for author := range fm.AuthorCounts {
			fm.UniqueAuthors = append(fm.UniqueAuthors, author)
		}

		fm.CalculateChurnScoreWithMax(maxCommits, maxChanges)

		filePath := absPath + "/" + fm.RelativePath
		fm.TotalLOC, fm.LOCReadError = countFileLOC(filePath)
		fm.CalculateRelativeChurn(now)

		analysis.Files = append(analysis.Files, *fm)

		totalCommits += fm.Commits
		totalAdded += fm.LinesAdded
		totalDeleted += fm.LinesDeleted

		for author, count := range fm.AuthorCounts {
			analysis.Summary.AuthorContributions[author] += count
		}
	}

	// Sort by churn score (highest first)
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].ChurnScore > analysis.Files[j].ChurnScore
	})

	// Build summary
	analysis.Summary.TotalFilesChanged = len(analysis.Files)
	analysis.Summary.TotalCommits = totalCommits
	analysis.Summary.TotalAdditions = totalAdded
	analysis.Summary.TotalDeletions = totalDeleted

	if len(analysis.Files) > 0 {
		analysis.Summary.AvgCommitsPerFile = float64(totalCommits) / float64(len(analysis.Files))
		analysis.Summary.MaxChurnScore = analysis.Files[0].ChurnScore
	}

	analysis.Summary.CalculateStatistics(analysis.Files)
	analysis.Summary.IdentifyHotspotAndStableFiles(analysis.Files)

	return analysis
}

// countLines counts the number of newlines in content.
func countLines(content string) int {
	return strings.Count(content, "\n")
}

// countFileLOC counts the number of lines in a file on disk.
// Returns the line count and whether an error occurred.
func countFileLOC(path string) (int, bool) {
	f, err := os.Open(path)
	if err != nil {
		return 0, true
	}
	defer f.Close()

	count := 0
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		count++
	}
	if scanner.Err() != nil {
		return count, true
	}
	return count, false
}
