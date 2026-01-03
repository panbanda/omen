package analysis

import (
	"testing"
)

// TestFindRelatedTestFile tests discovery of test files for source files.
// Research justification: LLMs benefit from seeing tests alongside source code
// because tests demonstrate expected usage patterns and edge cases. This is
// especially valuable given the "Lost in the Middle" phenomenon where LLMs
// perform best on context boundaries - tests provide natural examples at the
// boundary of implementation.
func TestFindRelatedTestFile(t *testing.T) {
	tests := []struct {
		name       string
		sourcePath string
		candidates []string // Available files in directory
		wantTest   string   // Expected test file, empty if none
	}{
		{
			name:       "Go source file with _test suffix",
			sourcePath: "pkg/analyzer/complexity/complexity.go",
			candidates: []string{
				"pkg/analyzer/complexity/complexity.go",
				"pkg/analyzer/complexity/complexity_test.go",
				"pkg/analyzer/complexity/options.go",
			},
			wantTest: "pkg/analyzer/complexity/complexity_test.go",
		},
		{
			name:       "Python source with test_ prefix",
			sourcePath: "src/utils/parser.py",
			candidates: []string{
				"src/utils/parser.py",
				"src/utils/test_parser.py",
			},
			wantTest: "src/utils/test_parser.py",
		},
		{
			name:       "Python source with _test suffix",
			sourcePath: "src/utils/parser.py",
			candidates: []string{
				"src/utils/parser.py",
				"src/utils/parser_test.py",
			},
			wantTest: "src/utils/parser_test.py",
		},
		{
			name:       "TypeScript source with .test. pattern",
			sourcePath: "src/components/Button.tsx",
			candidates: []string{
				"src/components/Button.tsx",
				"src/components/Button.test.tsx",
			},
			wantTest: "src/components/Button.test.tsx",
		},
		{
			name:       "TypeScript source with .spec. pattern",
			sourcePath: "src/services/api.ts",
			candidates: []string{
				"src/services/api.ts",
				"src/services/api.spec.ts",
			},
			wantTest: "src/services/api.spec.ts",
		},
		{
			name:       "JavaScript source in __tests__ directory",
			sourcePath: "src/utils/format.js",
			candidates: []string{
				"src/utils/format.js",
				"src/utils/__tests__/format.test.js",
			},
			wantTest: "src/utils/__tests__/format.test.js",
		},
		{
			name:       "Ruby source with _spec suffix (RSpec)",
			sourcePath: "lib/models/user.rb",
			candidates: []string{
				"lib/models/user.rb",
				"spec/models/user_spec.rb",
			},
			wantTest: "spec/models/user_spec.rb",
		},
		{
			name:       "Java source with Test suffix",
			sourcePath: "src/main/java/com/example/UserService.java",
			candidates: []string{
				"src/main/java/com/example/UserService.java",
				"src/test/java/com/example/UserServiceTest.java",
			},
			wantTest: "src/test/java/com/example/UserServiceTest.java",
		},
		{
			name:       "Rust source with tests module pattern",
			sourcePath: "src/parser.rs",
			candidates: []string{
				"src/parser.rs",
				"tests/parser_test.rs",
			},
			wantTest: "tests/parser_test.rs",
		},
		{
			name:       "No test file exists",
			sourcePath: "pkg/utils/helper.go",
			candidates: []string{
				"pkg/utils/helper.go",
				"pkg/utils/other.go",
			},
			wantTest: "",
		},
		{
			name:       "Test file itself should not match",
			sourcePath: "pkg/analyzer/complexity_test.go",
			candidates: []string{
				"pkg/analyzer/complexity.go",
				"pkg/analyzer/complexity_test.go",
			},
			wantTest: "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := findRelatedTestFile(tt.sourcePath, tt.candidates)
			if got != tt.wantTest {
				t.Errorf("findRelatedTestFile(%q) = %q, want %q", tt.sourcePath, got, tt.wantTest)
			}
		})
	}
}

// TestIsTestFile tests whether a file is identified as a test file.
func TestIsTestFile(t *testing.T) {
	tests := []struct {
		path   string
		isTest bool
	}{
		{"foo_test.go", true},
		{"foo.go", false},
		{"test_foo.py", true},
		{"foo_test.py", true},
		{"foo.py", false},
		{"foo.test.ts", true},
		{"foo.spec.ts", true},
		{"foo.ts", false},
		{"foo_spec.rb", true},
		{"foo.rb", false},
		{"FooTest.java", true},
		{"Foo.java", false},
		{"__tests__/foo.test.js", true},
		{"tests/test_foo.py", true},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := isTestFile(tt.path)
			if got != tt.isTest {
				t.Errorf("isTestFile(%q) = %v, want %v", tt.path, got, tt.isTest)
			}
		})
	}
}
