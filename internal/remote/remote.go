package remote

import (
	"os"
)

// Source represents a remote repository to analyze.
type Source struct {
	URL      string // normalized git URL
	Ref      string // branch, tag, or SHA (empty = default branch)
	CloneDir string // temp directory after clone
}

// Parse detects if a path is a remote reference.
// Returns nil if path exists on filesystem (local path takes precedence).
func Parse(path string) (*Source, error) {
	// Check if path exists locally
	if _, err := os.Stat(path); err == nil {
		return nil, nil
	}
	return nil, nil
}
