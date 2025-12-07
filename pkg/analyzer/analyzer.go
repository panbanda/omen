// Package analyzer defines interfaces for code analysis.
package analyzer

import "context"

// FileAnalyzer analyzes source code files.
// T is the result type returned by the analyzer.
type FileAnalyzer[T any] interface {
	// Analyze processes the given files and returns analysis results.
	// Progress can be tracked by passing a context with WithProgress.
	Analyze(ctx context.Context, files []string) (T, error)

	// Close releases any resources held by the analyzer.
	Close()
}

// RepoAnalyzer analyzes git repository history.
// T is the result type returned by the analyzer.
type RepoAnalyzer[T any] interface {
	// Analyze processes the repository at repoPath, optionally filtering to
	// the specified files. If files is nil or empty, all files are analyzed.
	// Progress can be tracked by passing a context with WithProgress.
	Analyze(ctx context.Context, repoPath string, files []string) (T, error)

	// Close releases any resources held by the analyzer.
	Close()
}
