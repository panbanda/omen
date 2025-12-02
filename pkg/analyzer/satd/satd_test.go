package satd

import (
	"os"
	"path/filepath"
	"testing"
)

func TestNew(t *testing.T) {
	a := New()
	if a == nil {
		t.Fatal("New() returned nil")
	}
	if len(a.patterns) == 0 {
		t.Error("analyzer should have default patterns")
	}
	if !a.includeTests {
		t.Error("includeTests should default to true")
	}
}

func TestNewWithOptions(t *testing.T) {
	a := New(
		WithSkipTests(),
		WithIncludeVendor(),
		WithStrictMode(),
		WithMaxFileSize(1024),
	)

	if a.includeTests {
		t.Error("WithSkipTests should disable includeTests")
	}
	if !a.includeVendor {
		t.Error("WithIncludeVendor should enable includeVendor")
	}
	if !a.strictMode {
		t.Error("WithStrictMode should enable strictMode")
	}
	if a.maxFileSize != 1024 {
		t.Errorf("maxFileSize = %d, want 1024", a.maxFileSize)
	}
}

func TestAnalyzeFile_TODO(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.go")

	code := `package main

// TODO: implement this function
func notImplemented() {
}

// FIXME: this is broken
func broken() {
}

// HACK: workaround for issue #123
func workaround() {
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	items, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(items) < 3 {
		t.Fatalf("len(items) = %d, want >= 3", len(items))
	}

	// Check TODO
	foundTODO := false
	foundFIXME := false
	foundHACK := false

	for _, item := range items {
		switch item.Marker {
		case "TODO":
			foundTODO = true
			if item.Category != CategoryRequirement {
				t.Errorf("TODO category = %q, want %q", item.Category, CategoryRequirement)
			}
			if item.Severity != SeverityLow {
				t.Errorf("TODO severity = %q, want %q", item.Severity, SeverityLow)
			}
		case "FIXME":
			foundFIXME = true
			if item.Category != CategoryDefect {
				t.Errorf("FIXME category = %q, want %q", item.Category, CategoryDefect)
			}
			if item.Severity != SeverityHigh {
				t.Errorf("FIXME severity = %q, want %q", item.Severity, SeverityHigh)
			}
		case "HACK":
			foundHACK = true
			if item.Category != CategoryDesign {
				t.Errorf("HACK category = %q, want %q", item.Category, CategoryDesign)
			}
		}
	}

	if !foundTODO {
		t.Error("TODO marker not found")
	}
	if !foundFIXME {
		t.Error("FIXME marker not found")
	}
	if !foundHACK {
		t.Error("HACK marker not found")
	}
}

func TestAnalyzeFile_Python(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.py")

	code := `# TODO: implement this
def not_implemented():
    pass

# FIXME: broken function
def broken():
    pass
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	items, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(items) < 2 {
		t.Fatalf("len(items) = %d, want >= 2", len(items))
	}
}

func TestAnalyzeFile_StrictMode(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.go")

	code := `package main

// TODO: explicit marker with colon
func explicit() {}

// TODO without colon - should not match in strict mode
func relaxed() {}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	// Strict mode
	a := New(WithStrictMode())
	items, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Strict mode should only match "TODO:" with colon
	if len(items) != 1 {
		t.Errorf("strict mode: len(items) = %d, want 1", len(items))
	}

	// Relaxed mode
	a2 := New()
	items2, err := a2.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Relaxed mode should match both
	if len(items2) < 2 {
		t.Errorf("relaxed mode: len(items) = %d, want >= 2", len(items2))
	}
}

func TestAnalyzeFile_SkipVendor(t *testing.T) {
	tmpDir := t.TempDir()
	vendorDir := filepath.Join(tmpDir, "vendor")
	if err := os.MkdirAll(vendorDir, 0755); err != nil {
		t.Fatalf("Failed to create vendor dir: %v", err)
	}

	path := filepath.Join(vendorDir, "lib.go")
	code := `package lib
// TODO: vendor code
func vendorFunc() {}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	// Default: exclude vendor
	a := New()
	items, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(items) != 0 {
		t.Errorf("vendor file should be skipped by default, got %d items", len(items))
	}

	// With vendor included
	a2 := New(WithIncludeVendor())
	items2, err := a2.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(items2) == 0 {
		t.Error("vendor file should be included with WithIncludeVendor")
	}
}

func TestAnalyzeFile_SecuritySeverity(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "auth.go")

	code := `package auth
// TODO: add password validation
func validatePassword(p string) bool {
	return true
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	items, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(items) == 0 {
		t.Fatal("expected at least one item")
	}

	// Security context should escalate severity
	// TODO is normally low, but in auth context might be escalated
	item := items[0]
	if item.Marker != "TODO" {
		t.Errorf("marker = %q, want TODO", item.Marker)
	}
}

func TestAnalyzeFile_ContextHash(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.go")

	code := `package main
// TODO: first item
func first() {}

// TODO: second item
func second() {}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	items, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(items) < 2 {
		t.Fatalf("expected at least 2 items, got %d", len(items))
	}

	// Each item should have unique context hash
	if items[0].ContextHash == "" {
		t.Error("first item should have context hash")
	}
	if items[1].ContextHash == "" {
		t.Error("second item should have context hash")
	}
	if items[0].ContextHash == items[1].ContextHash {
		t.Error("items should have different context hashes")
	}
}

func TestAnalyzeFile_NonexistentFile(t *testing.T) {
	a := New()
	_, err := a.AnalyzeFile("/nonexistent/path/file.go")
	if err == nil {
		t.Error("AnalyzeFile should fail for nonexistent file")
	}
}

func TestAnalyzeProject(t *testing.T) {
	tmpDir := t.TempDir()

	file1 := filepath.Join(tmpDir, "a.go")
	code1 := `package main
// TODO: first file
func a() {}
`
	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	file2 := filepath.Join(tmpDir, "b.go")
	code2 := `package main
// FIXME: second file
func b() {}

// HACK: another issue
func c() {}
`
	if err := os.WriteFile(file2, []byte(code2), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	analysis, err := a.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if analysis.Summary.TotalItems < 3 {
		t.Errorf("TotalItems = %d, want >= 3", analysis.Summary.TotalItems)
	}

	if analysis.TotalFilesAnalyzed != 2 {
		t.Errorf("TotalFilesAnalyzed = %d, want 2", analysis.TotalFilesAnalyzed)
	}

	if analysis.FilesWithDebt != 2 {
		t.Errorf("FilesWithDebt = %d, want 2", analysis.FilesWithDebt)
	}
}

func TestAddPattern(t *testing.T) {
	a := New()
	originalCount := len(a.patterns)

	err := a.AddPattern(`\bCUSTOM_MARKER\b`, CategoryDesign, SeverityMedium)
	if err != nil {
		t.Fatalf("AddPattern failed: %v", err)
	}

	if len(a.patterns) != originalCount+1 {
		t.Errorf("pattern count = %d, want %d", len(a.patterns), originalCount+1)
	}

	// Test invalid regex
	err = a.AddPattern(`[invalid`, CategoryDesign, SeverityMedium)
	if err == nil {
		t.Error("AddPattern should fail for invalid regex")
	}
}

func TestSeverity_String(t *testing.T) {
	tests := []struct {
		sev  Severity
		want string
	}{
		{SeverityLow, "low"},
		{SeverityCritical, "critical"},
	}

	for _, tt := range tests {
		got := tt.sev.String()
		if got != tt.want {
			t.Errorf("Severity(%q).String() = %q, want %q", tt.sev, got, tt.want)
		}
	}
}

func TestSeverity_Escalate(t *testing.T) {
	tests := []struct {
		sev  Severity
		want Severity
	}{
		{SeverityLow, SeverityMedium},
		{SeverityMedium, SeverityHigh},
		{SeverityHigh, SeverityCritical},
		{SeverityCritical, SeverityCritical}, // max already
	}

	for _, tt := range tests {
		got := tt.sev.Escalate()
		if got != tt.want {
			t.Errorf("Severity(%q).Escalate() = %q, want %q", tt.sev, got, tt.want)
		}
	}
}

func TestSeverity_Reduce(t *testing.T) {
	tests := []struct {
		sev  Severity
		want Severity
	}{
		{SeverityCritical, SeverityHigh},
		{SeverityHigh, SeverityMedium},
		{SeverityMedium, SeverityLow},
		{SeverityLow, SeverityLow}, // min already
	}

	for _, tt := range tests {
		got := tt.sev.Reduce()
		if got != tt.want {
			t.Errorf("Severity(%q).Reduce() = %q, want %q", tt.sev, got, tt.want)
		}
	}
}

func TestCategory_String(t *testing.T) {
	tests := []struct {
		cat  Category
		want string
	}{
		{CategoryDesign, "design"},
		{CategorySecurity, "security"},
	}

	for _, tt := range tests {
		got := tt.cat.String()
		if got != tt.want {
			t.Errorf("Category(%q).String() = %q, want %q", tt.cat, got, tt.want)
		}
	}
}
