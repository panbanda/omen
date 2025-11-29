package analyzer

import (
	"path/filepath"
	"strings"
)

// IsTestFile checks if a file is a test file based on naming conventions.
// Supports Go, Python, JavaScript/TypeScript, Rust, Ruby, Java, and C#.
func IsTestFile(path string) bool {
	base := filepath.Base(path)

	// Go test files
	if strings.HasSuffix(path, "_test.go") {
		return true
	}

	// Python test files (test_*.py or *_test.py)
	if strings.HasSuffix(path, "_test.py") || strings.HasPrefix(base, "test_") {
		return true
	}

	// JavaScript/TypeScript test files
	if strings.HasSuffix(path, ".test.ts") || strings.HasSuffix(path, ".test.js") ||
		strings.HasSuffix(path, ".spec.ts") || strings.HasSuffix(path, ".spec.js") ||
		strings.HasSuffix(path, ".test.tsx") || strings.HasSuffix(path, ".spec.tsx") ||
		strings.HasSuffix(path, ".test.jsx") || strings.HasSuffix(path, ".spec.jsx") {
		return true
	}

	// Ruby test files (test_*.rb or *_test.rb, also spec files)
	if strings.HasSuffix(path, "_test.rb") || strings.HasPrefix(base, "test_") ||
		strings.HasSuffix(path, "_spec.rb") {
		return true
	}

	// Java test files (Test*.java or *Test.java)
	if strings.HasSuffix(path, "Test.java") || strings.HasPrefix(base, "Test") && strings.HasSuffix(path, ".java") {
		return true
	}

	// C# test files (*Tests.cs or *Test.cs)
	if strings.HasSuffix(path, "Tests.cs") || strings.HasSuffix(path, "Test.cs") {
		return true
	}

	// Test directories
	if strings.Contains(path, "/tests/") || strings.Contains(path, "\\tests\\") ||
		strings.Contains(path, "/test/") || strings.Contains(path, "\\test\\") ||
		strings.Contains(path, "/__tests__/") || strings.Contains(path, "\\__tests__\\") ||
		strings.Contains(path, "/spec/") || strings.Contains(path, "\\spec\\") {
		return true
	}

	return false
}

// IsVendorFile checks if a file is in a vendor or third-party directory.
func IsVendorFile(path string) bool {
	vendorPatterns := []string{
		"/vendor/",
		"/node_modules/",
		"/third_party/",
		"/external/",
		"/.cargo/",
		"/site-packages/",
		"/venv/",
		"/.venv/",
	}
	for _, pattern := range vendorPatterns {
		if strings.Contains(path, pattern) {
			return true
		}
	}
	return false
}
