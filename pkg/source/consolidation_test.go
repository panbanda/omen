package source_test

import (
	"testing"

	"github.com/panbanda/omen/pkg/source"
)

// TestContentSourceConsolidation verifies that ContentSource is the single
// canonical interface used across the codebase.
func TestContentSourceConsolidation(t *testing.T) {
	// Verify FilesystemSource implements ContentSource
	var _ source.ContentSource = (*source.FilesystemSource)(nil)

	// Verify TreeSource implements ContentSource
	var _ source.ContentSource = (*source.TreeSource)(nil)

	// Test that FilesystemSource can be used as ContentSource
	fs := source.NewFilesystem()
	testContentSourceUsage(t, fs)
}

// testContentSourceUsage is a helper that accepts a ContentSource
// This verifies the interface is usable
func testContentSourceUsage(t *testing.T, src source.ContentSource) {
	t.Helper()
	// Read a known file
	content, err := src.Read("../../go.mod")
	if err != nil {
		t.Errorf("ContentSource.Read failed: %v", err)
	}
	if len(content) == 0 {
		t.Error("ContentSource.Read returned empty content")
	}
}

// mockContentSource is a test implementation of ContentSource
type mockContentSource struct {
	content map[string][]byte
}

func (m *mockContentSource) Read(path string) ([]byte, error) {
	if content, ok := m.content[path]; ok {
		return content, nil
	}
	return nil, &testError{path: path}
}

type testError struct {
	path string
}

func (e *testError) Error() string {
	return "file not found: " + e.path
}

// TestMockContentSource verifies custom implementations work
func TestMockContentSource(t *testing.T) {
	mock := &mockContentSource{
		content: map[string][]byte{
			"test.go": []byte("package main"),
		},
	}

	// Verify mock implements ContentSource
	var _ source.ContentSource = mock

	content, err := mock.Read("test.go")
	if err != nil {
		t.Errorf("mock.Read failed: %v", err)
	}
	if string(content) != "package main" {
		t.Errorf("unexpected content: %s", content)
	}

	_, err = mock.Read("nonexistent.go")
	if err == nil {
		t.Error("expected error for nonexistent file")
	}
}
