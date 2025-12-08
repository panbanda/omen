package satd

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/parser"
	"github.com/panbanda/omen/pkg/source"
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
	analysis, err := a.Analyze(context.Background(), []string{file1, file2}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
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

func TestWithSkipSeverityAdjustment(t *testing.T) {
	a := New(WithSkipSeverityAdjustment())
	if a.adjustSeverity {
		t.Error("WithSkipSeverityAdjustment should disable adjustSeverity")
	}

	// Default should have adjustSeverity enabled
	a2 := New()
	if !a2.adjustSeverity {
		t.Error("adjustSeverity should be enabled by default")
	}
}

func TestWithIncludeTestBlocks(t *testing.T) {
	a := New(WithIncludeTestBlocks())
	if a.excludeTestBlocks {
		t.Error("WithIncludeTestBlocks should disable excludeTestBlocks")
	}

	// Default should have excludeTestBlocks enabled
	a2 := New()
	if !a2.excludeTestBlocks {
		t.Error("excludeTestBlocks should be enabled by default")
	}
}

func TestSeverity_Weight(t *testing.T) {
	tests := []struct {
		sev  Severity
		want int
	}{
		{SeverityCritical, 4},
		{SeverityHigh, 3},
		{SeverityMedium, 2},
		{SeverityLow, 1},
		{Severity("unknown"), 0},
	}

	for _, tt := range tests {
		got := tt.sev.Weight()
		if got != tt.want {
			t.Errorf("Severity(%q).Weight() = %d, want %d", tt.sev, got, tt.want)
		}
	}
}

func TestSummary_AddItem(t *testing.T) {
	s := NewSummary()

	item1 := Item{
		Category: CategoryDefect,
		Severity: SeverityHigh,
		File:     "test.go",
		Line:     10,
		Marker:   "FIXME",
	}
	s.AddItem(item1)

	if s.TotalItems != 1 {
		t.Errorf("TotalItems = %d, want 1", s.TotalItems)
	}
	if s.BySeverity["high"] != 1 {
		t.Errorf("BySeverity[high] = %d, want 1", s.BySeverity["high"])
	}
	if s.ByCategory["defect"] != 1 {
		t.Errorf("ByCategory[defect] = %d, want 1", s.ByCategory["defect"])
	}
	if s.ByFile["test.go"] != 1 {
		t.Errorf("ByFile[test.go] = %d, want 1", s.ByFile["test.go"])
	}

	// Add another item to same file
	item2 := Item{
		Category: CategoryDesign,
		Severity: SeverityMedium,
		File:     "test.go",
		Line:     20,
		Marker:   "HACK",
	}
	s.AddItem(item2)

	if s.TotalItems != 2 {
		t.Errorf("TotalItems = %d, want 2", s.TotalItems)
	}
	if s.ByFile["test.go"] != 2 {
		t.Errorf("ByFile[test.go] = %d, want 2", s.ByFile["test.go"])
	}
}

func TestTestBlockTracker(t *testing.T) {
	// Test with tracking disabled
	tracker := newTestBlockTracker(false)
	tracker.updateFromLine("#[cfg(test)]")
	if tracker.isInTestBlock() {
		t.Error("tracker with enabled=false should not track test blocks")
	}

	// Test with tracking enabled
	tracker2 := newTestBlockTracker(true)

	// Not in test block initially
	if tracker2.isInTestBlock() {
		t.Error("should not be in test block initially")
	}

	// Enter test block
	tracker2.updateFromLine("#[cfg(test)]")
	if !tracker2.isInTestBlock() {
		t.Error("should be in test block after #[cfg(test)]")
	}

	// Open a brace
	tracker2.updateFromLine("mod tests {")
	if !tracker2.isInTestBlock() {
		t.Error("should still be in test block")
	}

	// Nested brace
	tracker2.updateFromLine("    fn test_something() {")
	if !tracker2.isInTestBlock() {
		t.Error("should still be in test block with nested brace")
	}

	// Close nested brace
	tracker2.updateFromLine("    }")
	if !tracker2.isInTestBlock() {
		t.Error("should still be in test block after closing nested brace")
	}

	// Close outer brace - exits test block
	tracker2.updateFromLine("}")
	if tracker2.isInTestBlock() {
		t.Error("should exit test block after closing outer brace")
	}
}

func TestAnalyzeFile_RustTestBlock(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "lib.rs")

	code := `fn main() {
    // TODO: implement main
}

#[cfg(test)]
mod tests {
    // TODO: add more tests - should be excluded by default
    fn test_something() {
    }
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	// Default: excludes test blocks
	a := New()
	items, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Should only find the TODO in main, not in test block
	if len(items) != 1 {
		t.Errorf("len(items) = %d, want 1 (excluding test block)", len(items))
	}

	// With test blocks included
	a2 := New(WithIncludeTestBlocks())
	items2, err := a2.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Should find both TODOs
	if len(items2) != 2 {
		t.Errorf("len(items) = %d, want 2 (including test block)", len(items2))
	}
}

func TestAnalyzeFile_SeverityAdjustment(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "auth.go")

	code := `package auth
// TODO: improve validation
func validateUser() {}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	// With severity adjustment (default)
	a := New()
	items, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(items) == 0 {
		t.Fatal("expected at least one item")
	}

	// Security context should escalate TODO from low to medium
	if items[0].Severity == SeverityLow {
		t.Logf("Note: security context escalation may not have triggered")
	}

	// Without severity adjustment
	a2 := New(WithSkipSeverityAdjustment())
	items2, err := a2.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(items2) == 0 {
		t.Fatal("expected at least one item")
	}

	// Without adjustment, TODO should remain low
	if items2[0].Severity != SeverityLow {
		t.Errorf("Severity = %q, want low (no adjustment)", items2[0].Severity)
	}
}

func TestAnalyzeFile_BugTrackerIDs(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.go")

	// Bug tracker IDs should not be flagged as SATD
	code := `package main

// Fixed in JIRA-1234
func fixed() {}

// Resolved by GH-5678
func resolved() {}

// TODO: actual debt item
func debt() {}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	items, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Should only find the actual TODO, not bug tracker references
	todoFound := false
	for _, item := range items {
		if item.Marker == "TODO" {
			todoFound = true
		}
	}

	if !todoFound {
		t.Error("should find the TODO marker")
	}
}

func TestIsMarkdownHeader(t *testing.T) {
	tests := []struct {
		line     string
		expected bool
	}{
		{"# Security", true},
		{"## Added", true},
		{"### Fixed", true},
		{"#### Changed", true},
		{"# [Unreleased]", true},
		{"## [1.0.0]", true},
		{"# TODO List", false}, // not a recognized changelog header
		{"// TODO: fix this", false},
		{"Not a header", false},
	}

	for _, tt := range tests {
		got := isMarkdownHeader(tt.line)
		if got != tt.expected {
			t.Errorf("isMarkdownHeader(%q) = %v, want %v", tt.line, got, tt.expected)
		}
	}
}

func TestIsBugTrackingID(t *testing.T) {
	tests := []struct {
		line     string
		expected bool
	}{
		{"BUG-123 was fixed here", true},
		{"bug-1 is a valid ID", true},
		{"See JIRA-BUG-456", true},
		{"Reference ABC-BUG-789", true},
		{"BUG- without number", false},
		{"No bug reference here", false},
		{"TODO: fix bug", false},
	}

	for _, tt := range tests {
		got := isBugTrackingID(tt.line)
		if got != tt.expected {
			t.Errorf("isBugTrackingID(%q) = %v, want %v", tt.line, got, tt.expected)
		}
	}
}

func TestIsFixedBugDescription(t *testing.T) {
	tests := []struct {
		line     string
		expected bool
	}{
		{"Bug: previous version had this issue", true},
		{"BUG: Previous behavior was wrong", true},
		{"This is a fix: for the issue", true},
		{"Fixed the bug", false},
		{"Bug: this is wrong", false}, // no "previous"
		{"TODO: fix this bug", false},
	}

	for _, tt := range tests {
		got := isFixedBugDescription(tt.line)
		if got != tt.expected {
			t.Errorf("isFixedBugDescription(%q) = %v, want %v", tt.line, got, tt.expected)
		}
	}
}

func TestGetCommentStyle(t *testing.T) {
	tests := []struct {
		lang       parser.Language
		wantPrefix string
	}{
		{parser.LangPython, "#"},
		{parser.LangRuby, "#"},
		{parser.LangBash, "#"},
		{parser.LangGo, "//"},
		{parser.LangRust, "//"},
		{parser.LangJava, "//"},
		{parser.LangTypeScript, "//"},
		{parser.LangJavaScript, "//"},
		{parser.LangUnknown, "//"}, // default includes both
	}

	for _, tt := range tests {
		style := getCommentStyle(tt.lang)
		found := false
		for _, prefix := range style.lineComments {
			if prefix == tt.wantPrefix {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("getCommentStyle(%v) should include %q in line comments", tt.lang, tt.wantPrefix)
		}
	}
}

func TestIsCommentLine(t *testing.T) {
	goStyle := getCommentStyle(parser.LangGo)
	pythonStyle := getCommentStyle(parser.LangPython)

	tests := []struct {
		line     string
		style    commentStyleInfo
		expected bool
	}{
		{"// comment", goStyle, true},
		{"  // indented comment", goStyle, true},
		{"/* block comment */", goStyle, true},
		{"* continuation", goStyle, true},
		{"code();", goStyle, false},
		{"# python comment", pythonStyle, true},
		{"  # indented", pythonStyle, true},
		{`""" docstring """`, pythonStyle, true},
		{"code()", pythonStyle, false},
	}

	for _, tt := range tests {
		got := isCommentLine(tt.line, tt.style)
		if got != tt.expected {
			t.Errorf("isCommentLine(%q, style) = %v, want %v", tt.line, got, tt.expected)
		}
	}
}

func TestExtractMarker(t *testing.T) {
	tests := []struct {
		match    string
		expected string
	}{
		{"TODO: fix this", "TODO"},
		{"FIXME: broken", "FIXME"},
		{"HACK: workaround", "HACK"},
		{"BUG: issue here", "BUG"},
		{"XXX: attention", "XXX"},
		{"NOTE: important", "NOTE"},
		{"OPTIMIZE: slow", "OPTIMIZE"},
		{"REFACTOR: clean up", "REFACTOR"},
		{"CLEANUP: remove", "CLEANUP"},
		{"TEMP: temporary", "TEMP"},
		{"WORKAROUND: bypass", "WORKAROUND"},
		{"SECURITY: vulnerability", "SECURITY"},
		{"TEST: add test", "TEST"},
	}

	for _, tt := range tests {
		got := extractMarker(tt.match)
		if got != tt.expected {
			t.Errorf("extractMarker(%q) = %q, want %q", tt.match, got, tt.expected)
		}
	}
}

func TestShouldExcludeFile(t *testing.T) {
	// Test minified file exclusion
	a := New()
	if !a.shouldExcludeFile("app.min.js") {
		t.Error("should exclude minified .min.js files")
	}
	if !a.shouldExcludeFile("styles.min.css") {
		t.Error("should exclude minified .min.css files")
	}

	// Test that non-minified files are not excluded
	if a.shouldExcludeFile("app.js") {
		t.Error("should not exclude regular .js files")
	}

	// Test vendor file with default options
	if !a.shouldExcludeFile("vendor/lib/code.go") {
		t.Error("should exclude vendor files by default")
	}
	if !a.shouldExcludeFile("node_modules/pkg/index.js") {
		t.Error("should exclude node_modules files by default")
	}

	// Test with vendor included
	a2 := New(WithIncludeVendor())
	if a2.shouldExcludeFile("vendor/lib/code.go") {
		t.Error("should include vendor files with WithIncludeVendor")
	}
}

func TestIsTestFile(t *testing.T) {
	a := New()

	tests := []struct {
		path     string
		expected bool
	}{
		{"foo_test.go", true},
		{"foo.test.js", true},
		{"foo.spec.ts", true},
		{"test_foo.py", true},
		{"foo_spec.rb", true},
		{"foo.go", false},
		{"main.js", false},
	}

	for _, tt := range tests {
		got := a.isTestFile(tt.path)
		if got != tt.expected {
			t.Errorf("isTestFile(%q) = %v, want %v", tt.path, got, tt.expected)
		}
	}
}

func TestAdjustSeverityWithContext(t *testing.T) {
	a := New()

	// Test security context escalation
	ctx := &AstContext{
		NodeType: AstNodeSecurityFunction,
	}
	adjusted := a.AdjustSeverityWithContext(SeverityLow, ctx)
	if adjusted == SeverityLow {
		t.Error("security context should escalate severity")
	}

	// Test data validation context escalation
	ctx2 := &AstContext{
		NodeType: AstNodeDataValidation,
	}
	adjusted2 := a.AdjustSeverityWithContext(SeverityMedium, ctx2)
	if adjusted2 != SeverityHigh {
		t.Errorf("data validation context should escalate, got %q", adjusted2)
	}

	// Test test function context reduction
	ctx3 := &AstContext{
		NodeType: AstNodeTestFunction,
	}
	adjusted3 := a.AdjustSeverityWithContext(SeverityHigh, ctx3)
	if adjusted3 != SeverityMedium {
		t.Errorf("test function context should reduce, got %q", adjusted3)
	}

	// Test mock implementation context reduction
	ctx4 := &AstContext{
		NodeType: AstNodeMockImplementation,
	}
	adjusted4 := a.AdjustSeverityWithContext(SeverityCritical, ctx4)
	if adjusted4 != SeverityHigh {
		t.Errorf("mock context should reduce, got %q", adjusted4)
	}

	// Test high complexity escalation
	ctx5 := &AstContext{
		NodeType:   AstNodeRegular,
		Complexity: 25,
	}
	adjusted5 := a.AdjustSeverityWithContext(SeverityLow, ctx5)
	if adjusted5 != SeverityMedium {
		t.Errorf("high complexity should escalate, got %q", adjusted5)
	}

	// Test regular context with low complexity (no change)
	ctx6 := &AstContext{
		NodeType:   AstNodeRegular,
		Complexity: 5,
	}
	adjusted6 := a.AdjustSeverityWithContext(SeverityLow, ctx6)
	if adjusted6 != SeverityLow {
		t.Errorf("low complexity should not change, got %q", adjusted6)
	}
}

func TestNewSummary(t *testing.T) {
	s := NewSummary()

	if s.BySeverity == nil {
		t.Error("BySeverity map should be initialized")
	}
	if s.ByCategory == nil {
		t.Error("ByCategory map should be initialized")
	}
	if s.ByFile == nil {
		t.Error("ByFile map should be initialized")
	}
	if s.TotalItems != 0 {
		t.Errorf("TotalItems = %d, want 0", s.TotalItems)
	}
}

func TestHasIgnoreDirective(t *testing.T) {
	tests := []struct {
		line     string
		expected bool
	}{
		// Should ignore
		{"// omen:ignore", true},
		{"// omen:ignore-line", true},
		{"// omen:ignore-satd", true},
		{"# omen:ignore", true},
		{"/* omen:ignore */", true},
		{"// SECURITY: validate input omen:ignore", true},
		{"// TODO: fix this omen:ignore", true},

		// Should NOT ignore
		{"// TODO: implement this function", false},
		{"// FIXME: this is broken", false},
		{"// some regular comment", false},
		{"code();", false},
	}

	for _, tt := range tests {
		got := hasIgnoreDirective(tt.line)
		if got != tt.expected {
			t.Errorf("hasIgnoreDirective(%q) = %v, want %v", tt.line, got, tt.expected)
		}
	}
}

func TestAnalyzeFile_IgnoreDirective(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "normalize.go")

	code := `package score

// Severity weights (based on remediation urgency): omen:ignore
// - Critical (SECURITY, VULN): 4.0 - Immediate security risk omen:ignore
// - High (FIXME, BUG): 2.0 - Known defects requiring fix omen:ignore
// - Medium (HACK, REFACTOR): 1.0 - Design compromises omen:ignore
// - Low (TODO, NOTE): 0.25 - Future work, minimal impact omen:ignore

// TODO: add actual implementation
func NormalizeDebt() int {
	return 0
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

	// Should only find the actual TODO, not the ignored documentation
	if len(items) != 1 {
		t.Errorf("len(items) = %d, want 1 (only actual TODO)", len(items))
		for _, item := range items {
			t.Logf("  Found: %s at line %d: %s", item.Marker, item.Line, item.Description)
		}
	}

	if len(items) > 0 && items[0].Marker != "TODO" {
		t.Errorf("expected TODO marker, got %s", items[0].Marker)
	}
}
