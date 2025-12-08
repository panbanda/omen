package remote

import (
	"testing"
)

func TestParse_LocalPath(t *testing.T) {
	// Create a temp directory that exists
	dir := t.TempDir()

	src, err := Parse(dir)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if src != nil {
		t.Errorf("expected nil for local path, got %+v", src)
	}
}
