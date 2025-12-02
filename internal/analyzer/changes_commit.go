package analyzer

import (
	"context"
	"strings"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/models"
)

// rawCommit holds commit data collected in the first pass.
// State-dependent fields (AuthorExperience, NumDevelopers, UniqueChanges)
// are computed in the second pass after sorting by timestamp.
type rawCommit struct {
	features     models.CommitFeatures
	linesPerFile map[string]int // for entropy calculation
}

// collectCommitData iterates through commits and extracts raw commit data.
// Returns commits in git log order (newest-first).
func (a *ChangesAnalyzer) collectCommitData(ctx context.Context, logIter vcs.CommitIterator) ([]rawCommit, error) {
	var rawCommits []rawCommit

	err := logIter.ForEach(func(commit vcs.Commit) error {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		if commit.NumParents() == 0 {
			return nil // Skip initial commit
		}

		raw, err := a.extractCommitFeatures(commit)
		if err != nil {
			return nil // Skip commits that can't be processed
		}

		rawCommits = append(rawCommits, raw)
		return nil
	})

	return rawCommits, err
}

// extractCommitFeatures extracts commit-local features (no state dependencies).
func (a *ChangesAnalyzer) extractCommitFeatures(commit vcs.Commit) (rawCommit, error) {
	author := commit.Author().Name
	message := commit.Message()

	parent, err := commit.Parent(0)
	if err != nil {
		return rawCommit{}, err
	}

	parentTree, err := parent.Tree()
	if err != nil {
		return rawCommit{}, err
	}

	commitTree, err := commit.Tree()
	if err != nil {
		return rawCommit{}, err
	}

	changes, err := parentTree.Diff(commitTree)
	if err != nil {
		return rawCommit{}, err
	}

	features := models.CommitFeatures{
		CommitHash:    commit.Hash().String(),
		Author:        author,
		Message:       truncateMessage(message),
		Timestamp:     commit.Author().When,
		IsFix:         isBugFixCommit(message),
		IsAutomated:   isAutomatedCommit(message),
		FilesModified: make([]string, 0),
	}

	linesPerFile := make(map[string]int)

	for _, change := range changes {
		filePath := change.ToName()
		if filePath == "" {
			filePath = change.FromName() // Deleted file
		}

		features.FilesModified = append(features.FilesModified, filePath)

		patch, err := change.Patch()
		if err == nil {
			for _, filePatch := range patch.FilePatches() {
				for _, chunk := range filePatch.Chunks() {
					content := chunk.Content()
					lines := strings.Count(content, "\n")
					switch chunk.Type() {
					case vcs.ChunkAdd:
						features.LinesAdded += lines
						linesPerFile[filePath] += lines
					case vcs.ChunkDelete:
						features.LinesDeleted += lines
						linesPerFile[filePath] += lines
					}
				}
			}
		}
	}

	features.NumFiles = len(features.FilesModified)
	features.Entropy = models.CalculateEntropy(linesPerFile)

	return rawCommit{
		features:     features,
		linesPerFile: linesPerFile,
	}, nil
}

// reverseCommits reverses the commit slice in place for chronological processing.
// Git log returns newest-first; we need oldest-first for state-dependent metrics.
func reverseCommits(commits []rawCommit) {
	for i, j := 0, len(commits)-1; i < j; i, j = i+1, j-1 {
		commits[i], commits[j] = commits[j], commits[i]
	}
}

// computeStateDependentFeatures computes features that depend on historical state.
// Processes commits chronologically (oldest-first) and tracks author experience,
// file change history, and developer counts.
func computeStateDependentFeatures(rawCommits []rawCommit) []models.CommitFeatures {
	// State tracked across commits
	authorCommits := make(map[string]int)           // author -> commits made BEFORE current
	fileChanges := make(map[string]int)             // file -> commits touching it BEFORE current
	fileAuthors := make(map[string]map[string]bool) // file -> authors who touched it BEFORE current

	var commits []models.CommitFeatures
	for _, raw := range rawCommits {
		features := raw.features
		author := features.Author

		// Look up state BEFORE this commit
		features.AuthorExperience = authorCommits[author]

		// Calculate NumDevelopers and UniqueChanges from state BEFORE this commit
		uniqueDevs := make(map[string]bool)
		priorCommits := 0
		for _, filePath := range features.FilesModified {
			priorCommits += fileChanges[filePath]
			if authors, ok := fileAuthors[filePath]; ok {
				for auth := range authors {
					uniqueDevs[auth] = true
				}
			}
		}
		features.NumDevelopers = len(uniqueDevs)
		features.UniqueChanges = priorCommits

		commits = append(commits, features)

		// Update state AFTER processing this commit (for future commits)
		authorCommits[author]++
		for _, file := range features.FilesModified {
			fileChanges[file]++
			if fileAuthors[file] == nil {
				fileAuthors[file] = make(map[string]bool)
			}
			fileAuthors[file][author] = true
		}
	}

	return commits
}
