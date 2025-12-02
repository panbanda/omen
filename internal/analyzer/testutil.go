package analyzer

import (
	"path/filepath"
	"strings"
)

// testFileSuffixes are path suffixes that indicate test files.
var testFileSuffixes = []string{
	// Go
	"_test.go",
	// Python
	"_test.py",
	// JavaScript/TypeScript
	".test.ts", ".test.js", ".spec.ts", ".spec.js",
	".test.tsx", ".spec.tsx", ".test.jsx", ".spec.jsx",
	// Ruby
	"_test.rb", "_spec.rb",
	// Java
	"Test.java",
	// C#
	"Tests.cs", "Test.cs",
}

// testBasePrefixes are filename prefixes that indicate test files.
var testBasePrefixes = []string{
	"test_", // Python, Ruby
	"Test",  // Java (Test*.java)
}

// testDirPatterns are directory patterns that indicate test directories.
var testDirPatterns = []string{
	"/tests/", "\\tests\\",
	"/test/", "\\test\\",
	"/__tests__/", "\\__tests__\\",
	"/spec/", "\\spec\\",
}

// IsTestFile checks if a file is a test file based on naming conventions.
// Supports Go, Python, JavaScript/TypeScript, Rust, Ruby, Java, and C#.
func IsTestFile(path string) bool {
	for _, suffix := range testFileSuffixes {
		if strings.HasSuffix(path, suffix) {
			return true
		}
	}

	base := filepath.Base(path)
	for _, prefix := range testBasePrefixes {
		if strings.HasPrefix(base, prefix) {
			return true
		}
	}

	for _, pattern := range testDirPatterns {
		if strings.Contains(path, pattern) {
			return true
		}
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
