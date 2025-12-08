package remote

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"testing"

	"github.com/go-git/go-git/v6"
	"github.com/go-git/go-git/v6/plumbing/object"
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

func TestParse_GitHubShorthand(t *testing.T) {
	tests := []struct {
		name    string
		input   string
		wantURL string
		wantRef string
	}{
		{
			name:    "simple owner/repo",
			input:   "facebook/react",
			wantURL: "https://github.com/facebook/react",
			wantRef: "",
		},
		{
			name:    "with ref suffix",
			input:   "facebook/react@v18.2.0",
			wantURL: "https://github.com/facebook/react",
			wantRef: "v18.2.0",
		},
		{
			name:    "with branch ref",
			input:   "owner/repo@feature-branch",
			wantURL: "https://github.com/owner/repo",
			wantRef: "feature-branch",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			src, err := Parse(tt.input)
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if src == nil {
				t.Fatal("expected Source, got nil")
			}
			if src.URL != tt.wantURL {
				t.Errorf("URL = %q, want %q", src.URL, tt.wantURL)
			}
			if src.Ref != tt.wantRef {
				t.Errorf("Ref = %q, want %q", src.Ref, tt.wantRef)
			}
		})
	}
}

func TestParse_FullURLs(t *testing.T) {
	tests := []struct {
		name    string
		input   string
		wantURL string
		wantRef string
	}{
		{
			name:    "github.com without scheme",
			input:   "github.com/golang/go",
			wantURL: "https://github.com/golang/go",
			wantRef: "",
		},
		{
			name:    "https URL",
			input:   "https://github.com/kubernetes/kubernetes",
			wantURL: "https://github.com/kubernetes/kubernetes",
			wantRef: "",
		},
		{
			name:    "gitlab URL",
			input:   "https://gitlab.com/group/project",
			wantURL: "https://gitlab.com/group/project",
			wantRef: "",
		},
		{
			name:    "SSH URL",
			input:   "git@github.com:owner/repo.git",
			wantURL: "git@github.com:owner/repo.git",
			wantRef: "",
		},
		{
			name:    "URL with ref",
			input:   "github.com/golang/go@go1.21.0",
			wantURL: "https://github.com/golang/go",
			wantRef: "go1.21.0",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			src, err := Parse(tt.input)
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if src == nil {
				t.Fatal("expected Source, got nil")
			}
			if src.URL != tt.wantURL {
				t.Errorf("URL = %q, want %q", src.URL, tt.wantURL)
			}
			if src.Ref != tt.wantRef {
				t.Errorf("Ref = %q, want %q", src.Ref, tt.wantRef)
			}
		})
	}
}

func TestSource_Clone(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping integration test in short mode")
	}

	src := &Source{
		URL: "https://github.com/octocat/Hello-World",
		Ref: "",
	}

	ctx := context.Background()
	err := src.Clone(ctx, io.Discard, false)
	if err != nil {
		t.Fatalf("Clone failed: %v", err)
	}
	defer src.Cleanup()

	// Verify clone directory exists and contains .git
	if src.CloneDir == "" {
		t.Fatal("CloneDir not set")
	}
	gitDir := filepath.Join(src.CloneDir, ".git")
	if _, err := os.Stat(gitDir); os.IsNotExist(err) {
		t.Errorf(".git directory not found in %s", src.CloneDir)
	}
}

func TestSource_Clone_WithRef(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping integration test in short mode")
	}

	src := &Source{
		URL: "https://github.com/octocat/Hello-World",
		Ref: "master",
	}

	ctx := context.Background()
	err := src.Clone(ctx, io.Discard, false)
	if err != nil {
		t.Fatalf("Clone failed: %v", err)
	}
	defer src.Cleanup()

	// Verify we're on the right branch
	repo, err := git.PlainOpen(src.CloneDir)
	if err != nil {
		t.Fatalf("open cloned repo: %v", err)
	}
	head, err := repo.Head()
	if err != nil {
		t.Fatalf("get HEAD: %v", err)
	}
	if !head.Name().IsBranch() || head.Name().Short() != "master" {
		t.Errorf("expected branch master, got %s", head.Name())
	}
}

func TestSource_Clone_Shallow(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping integration test in short mode")
	}

	src := &Source{
		URL: "https://github.com/octocat/Hello-World",
	}

	ctx := context.Background()
	err := src.Clone(ctx, io.Discard, true)
	if err != nil {
		t.Fatalf("Clone failed: %v", err)
	}
	defer src.Cleanup()

	// Verify it's a shallow clone by checking commit count
	repo, err := git.PlainOpen(src.CloneDir)
	if err != nil {
		t.Fatalf("open cloned repo: %v", err)
	}
	iter, err := repo.Log(&git.LogOptions{})
	if err != nil {
		t.Fatalf("get log: %v", err)
	}

	count := 0
	iter.ForEach(func(c *object.Commit) error {
		count++
		return nil
	})

	// Shallow clone should have very few commits (typically 1)
	if count > 5 {
		t.Errorf("expected shallow clone with few commits, got %d", count)
	}
}
