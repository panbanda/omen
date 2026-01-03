package analysis

import (
	"path/filepath"
	"strings"
)

// findRelatedTestFile finds the test file for a given source file.
// Returns empty string if no related test file is found.
//
// This supports common test naming conventions across languages:
// - Go: foo.go -> foo_test.go
// - Python: foo.py -> test_foo.py or foo_test.py
// - TypeScript/JavaScript: foo.ts -> foo.test.ts or foo.spec.ts
// - Ruby: foo.rb -> foo_spec.rb (RSpec)
// - Java: Foo.java -> FooTest.java
// - Rust: foo.rs -> tests/foo_test.rs
func findRelatedTestFile(sourcePath string, candidates []string) string {
	// Don't find tests for test files
	if isTestFile(sourcePath) {
		return ""
	}

	dir := filepath.Dir(sourcePath)
	base := filepath.Base(sourcePath)
	ext := filepath.Ext(base)
	name := strings.TrimSuffix(base, ext)

	// Build potential test file patterns based on language conventions
	var patterns []string

	switch ext {
	case ".go":
		// Go: foo.go -> foo_test.go
		patterns = append(patterns, filepath.Join(dir, name+"_test.go"))

	case ".py":
		// Python: foo.py -> test_foo.py or foo_test.py
		patterns = append(patterns, filepath.Join(dir, "test_"+name+".py"))
		patterns = append(patterns, filepath.Join(dir, name+"_test.py"))
		// Also check tests/ subdirectory
		patterns = append(patterns, filepath.Join("tests", "test_"+name+".py"))

	case ".ts", ".tsx":
		// TypeScript: foo.ts -> foo.test.ts or foo.spec.ts
		patterns = append(patterns, filepath.Join(dir, name+".test"+ext))
		patterns = append(patterns, filepath.Join(dir, name+".spec"+ext))
		// Also check __tests__ directory
		patterns = append(patterns, filepath.Join(dir, "__tests__", name+".test"+ext))

	case ".js", ".jsx":
		// JavaScript: same patterns as TypeScript
		patterns = append(patterns, filepath.Join(dir, name+".test"+ext))
		patterns = append(patterns, filepath.Join(dir, name+".spec"+ext))
		patterns = append(patterns, filepath.Join(dir, "__tests__", name+".test"+ext))

	case ".rb":
		// Ruby (RSpec): foo.rb -> foo_spec.rb or spec/foo_spec.rb
		patterns = append(patterns, filepath.Join(dir, name+"_spec.rb"))
		// Handle lib/models/user.rb -> spec/models/user_spec.rb
		if strings.HasPrefix(dir, "lib/") {
			specDir := strings.Replace(dir, "lib/", "spec/", 1)
			patterns = append(patterns, filepath.Join(specDir, name+"_spec.rb"))
		}
		// Also check spec/ at same level
		patterns = append(patterns, filepath.Join("spec", filepath.Base(dir), name+"_spec.rb"))

	case ".java":
		// Java: Foo.java -> FooTest.java (in test/java path)
		patterns = append(patterns, filepath.Join(dir, name+"Test.java"))
		// Handle src/main/java/... -> src/test/java/...
		if strings.Contains(dir, "src/main/java") {
			testDir := strings.Replace(dir, "src/main/java", "src/test/java", 1)
			patterns = append(patterns, filepath.Join(testDir, name+"Test.java"))
		}

	case ".rs":
		// Rust: foo.rs -> tests/foo_test.rs or foo_test.rs
		patterns = append(patterns, filepath.Join(dir, name+"_test.rs"))
		patterns = append(patterns, filepath.Join("tests", name+"_test.rs"))

	case ".c", ".cpp", ".h", ".hpp":
		// C/C++: foo.c -> foo_test.c or test_foo.c
		patterns = append(patterns, filepath.Join(dir, name+"_test"+ext))
		patterns = append(patterns, filepath.Join(dir, "test_"+name+ext))
		patterns = append(patterns, filepath.Join("tests", name+"_test"+ext))

	case ".cs":
		// C#: Foo.cs -> FooTests.cs or FooTest.cs
		patterns = append(patterns, filepath.Join(dir, name+"Tests.cs"))
		patterns = append(patterns, filepath.Join(dir, name+"Test.cs"))
	}

	// Check candidates against patterns
	candidateSet := make(map[string]bool)
	for _, c := range candidates {
		candidateSet[c] = true
	}

	for _, pattern := range patterns {
		if candidateSet[pattern] {
			return pattern
		}
	}

	return ""
}

// isTestFile returns true if the path appears to be a test file.
func isTestFile(path string) bool {
	base := filepath.Base(path)
	dir := filepath.Dir(path)

	// Check directory patterns
	if strings.Contains(dir, "__tests__") || strings.HasPrefix(dir, "tests/") || strings.HasPrefix(dir, "test/") {
		return true
	}

	ext := filepath.Ext(base)
	name := strings.TrimSuffix(base, ext)

	switch ext {
	case ".go":
		return strings.HasSuffix(name, "_test")

	case ".py":
		return strings.HasPrefix(name, "test_") || strings.HasSuffix(name, "_test")

	case ".ts", ".tsx", ".js", ".jsx":
		return strings.HasSuffix(name, ".test") || strings.HasSuffix(name, ".spec")

	case ".rb":
		return strings.HasSuffix(name, "_spec")

	case ".java":
		return strings.HasSuffix(name, "Test") || strings.HasSuffix(name, "Tests")

	case ".rs":
		return strings.HasSuffix(name, "_test")

	case ".c", ".cpp", ".h", ".hpp":
		return strings.HasSuffix(name, "_test") || strings.HasPrefix(name, "test_")

	case ".cs":
		return strings.HasSuffix(name, "Test") || strings.HasSuffix(name, "Tests")
	}

	return false
}
