package analyzer

import "context"

// FileAnalyzer is the interface that all file-based analyzers must implement.
// It provides a standard way to analyze collections of files with context support.
type FileAnalyzer[T any] interface {
	// Analyze processes a collection of files and returns the analysis result.
	// The context can be used for cancellation and progress reporting.
	Analyze(ctx context.Context, files []string) (T, error)

	// Close releases any resources held by the analyzer.
	Close()
}
