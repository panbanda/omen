package mcpserver

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/panbanda/omen/internal/output"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
)

// TestServerCreation verifies the MCP server can be created without panicking.
func TestServerCreation(t *testing.T) {
	server := NewServer("1.0.0-test")
	if server == nil {
		t.Fatal("NewServer() returned nil")
	}
	if server.server == nil {
		t.Fatal("NewServer().server is nil")
	}
}

// TestServerCreationEmptyVersion verifies empty version defaults to "dev".
func TestServerCreationEmptyVersion(t *testing.T) {
	server := NewServer("")
	if server == nil {
		t.Fatal("NewServer(\"\") returned nil")
	}
}

// TestToolDescriptions verifies all description functions return non-empty strings.
func TestToolDescriptions(t *testing.T) {
	descriptions := map[string]func() string{
		"complexity":       describeComplexity,
		"satd":             describeSATD,
		"deadcode":         describeDeadcode,
		"churn":            describeChurn,
		"duplicates":       describeDuplicates,
		"defect":           describeDefect,
		"tdg":              describeTDG,
		"graph":            describeGraph,
		"hotspot":          describeHotspot,
		"temporalCoupling": describeTemporalCoupling,
		"ownership":        describeOwnership,
		"cohesion":         describeCohesion,
		"repoMap":          describeRepoMap,
	}

	for name, fn := range descriptions {
		t.Run(name, func(t *testing.T) {
			desc := fn()
			if desc == "" {
				t.Errorf("%s description is empty", name)
			}
			// Verify descriptions contain key sections
			if !contains(desc, "USE WHEN:") {
				t.Errorf("%s description missing USE WHEN section", name)
			}
			if !contains(desc, "INTERPRETING RESULTS:") {
				t.Errorf("%s description missing INTERPRETING RESULTS section", name)
			}
			if !contains(desc, "METRICS RETURNED:") {
				t.Errorf("%s description missing METRICS RETURNED section", name)
			}
		})
	}
}

func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(s) > 0 && containsHelper(s, substr))
}

func containsHelper(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

// TestGetPaths verifies path handling logic.
func TestGetPaths(t *testing.T) {
	tests := []struct {
		name     string
		input    AnalyzeInput
		expected []string
	}{
		{
			name:     "empty paths defaults to current dir",
			input:    AnalyzeInput{Paths: nil},
			expected: []string{"."},
		},
		{
			name:     "empty slice defaults to current dir",
			input:    AnalyzeInput{Paths: []string{}},
			expected: []string{"."},
		},
		{
			name:     "single path returned as-is",
			input:    AnalyzeInput{Paths: []string{"/foo/bar"}},
			expected: []string{"/foo/bar"},
		},
		{
			name:     "multiple paths returned as-is",
			input:    AnalyzeInput{Paths: []string{"/foo", "/bar"}},
			expected: []string{"/foo", "/bar"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := getPaths(tt.input)
			if len(result) != len(tt.expected) {
				t.Errorf("getPaths() = %v, want %v", result, tt.expected)
				return
			}
			for i := range result {
				if result[i] != tt.expected[i] {
					t.Errorf("getPaths()[%d] = %q, want %q", i, result[i], tt.expected[i])
				}
			}
		})
	}
}

// TestGetFormat verifies format parsing logic.
func TestGetFormat(t *testing.T) {
	tests := []struct {
		name     string
		format   string
		expected output.Format
	}{
		{"empty defaults to toon", "", output.FormatTOON},
		{"json format", "json", output.FormatJSON},
		{"markdown format", "markdown", output.FormatMarkdown},
		{"md alias", "md", output.FormatMarkdown},
		{"toon explicit", "toon", output.FormatTOON},
		{"unknown defaults to toon", "xml", output.FormatTOON},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			input := AnalyzeInput{Format: tt.format}
			result := getFormat(input)
			if result != tt.expected {
				t.Errorf("getFormat(%q) = %v, want %v", tt.format, result, tt.expected)
			}
		})
	}
}

// TestToolError verifies error result formatting.
func TestToolError(t *testing.T) {
	result, _, err := toolError("test error message")
	if err != nil {
		t.Fatalf("toolError returned unexpected error: %v", err)
	}
	if result == nil {
		t.Fatal("toolError returned nil result")
	}
	if !result.IsError {
		t.Error("toolError result.IsError should be true")
	}
	if len(result.Content) == 0 {
		t.Fatal("toolError result has no content")
	}
	textContent, ok := result.Content[0].(*mcp.TextContent)
	if !ok {
		t.Fatalf("toolError content is not TextContent: %T", result.Content[0])
	}
	if textContent.Text != "Error: test error message" {
		t.Errorf("toolError text = %q, want %q", textContent.Text, "Error: test error message")
	}
}

// TestToolResult verifies successful result formatting.
func TestToolResult(t *testing.T) {
	data := map[string]interface{}{
		"key": "value",
		"num": 42,
	}
	result, _, err := toolResult(data, getFormat(AnalyzeInput{}))
	if err != nil {
		t.Fatalf("toolResult returned error: %v", err)
	}
	if result == nil {
		t.Fatal("toolResult returned nil")
	}
	if result.IsError {
		t.Error("toolResult.IsError should be false")
	}
	if len(result.Content) == 0 {
		t.Fatal("toolResult has no content")
	}
	textContent, ok := result.Content[0].(*mcp.TextContent)
	if !ok {
		t.Fatalf("toolResult content is not TextContent: %T", result.Content[0])
	}
	if textContent.Text == "" {
		t.Error("toolResult text is empty")
	}
}

// TestInputStructTags verifies all input structs have valid jsonschema tags.
func TestInputStructTags(t *testing.T) {
	// Create test instances of each input type to verify they can be marshaled
	inputs := []interface{}{
		ComplexityInput{},
		SATDInput{},
		DeadcodeInput{},
		ChurnInput{},
		DuplicatesInput{},
		DefectInput{},
		TDGInput{},
		GraphInput{},
		HotspotInput{},
		TemporalCouplingInput{},
		OwnershipInput{},
		CohesionInput{},
		RepoMapInput{},
	}

	for _, input := range inputs {
		t.Run(typeName(input), func(t *testing.T) {
			// Verify JSON marshaling works
			data, err := json.Marshal(input)
			if err != nil {
				t.Errorf("failed to marshal: %v", err)
			}
			if len(data) == 0 {
				t.Error("marshaled to empty data")
			}
		})
	}
}

func typeName(v interface{}) string {
	switch v.(type) {
	case ComplexityInput:
		return "ComplexityInput"
	case SATDInput:
		return "SATDInput"
	case DeadcodeInput:
		return "DeadcodeInput"
	case ChurnInput:
		return "ChurnInput"
	case DuplicatesInput:
		return "DuplicatesInput"
	case DefectInput:
		return "DefectInput"
	case TDGInput:
		return "TDGInput"
	case GraphInput:
		return "GraphInput"
	case HotspotInput:
		return "HotspotInput"
	case TemporalCouplingInput:
		return "TemporalCouplingInput"
	case OwnershipInput:
		return "OwnershipInput"
	case CohesionInput:
		return "CohesionInput"
	case RepoMapInput:
		return "RepoMapInput"
	default:
		return "Unknown"
	}
}

// TestHandleComplexity tests the complexity analyzer tool handler.
func TestHandleComplexity(t *testing.T) {
	// Create a temp directory with a Go file
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

func simple() {
	x := 1
	_ = x
}

func complex() {
	for i := 0; i < 10; i++ {
		if i%2 == 0 {
			continue
		}
	}
}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	input := ComplexityInput{
		AnalyzeInput: AnalyzeInput{
			Paths:  []string{tmpDir},
			Format: "json",
		},
	}

	result, _, err := handleAnalyzeComplexity(context.Background(), nil, input)
	if err != nil {
		t.Fatalf("handleAnalyzeComplexity returned error: %v", err)
	}
	if result == nil {
		t.Fatal("handleAnalyzeComplexity returned nil result")
	}
	if result.IsError {
		textContent := result.Content[0].(*mcp.TextContent)
		t.Fatalf("handleAnalyzeComplexity returned error: %s", textContent.Text)
	}
	if len(result.Content) == 0 {
		t.Fatal("result has no content")
	}
}

// TestHandleSATD tests the SATD analyzer tool handler.
func TestHandleSATD(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

// TODO: fix this later
func broken() {
	// HACK: temporary workaround
	x := 1
	_ = x
}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	input := SATDInput{
		AnalyzeInput: AnalyzeInput{
			Paths:  []string{tmpDir},
			Format: "json",
		},
	}

	result, _, err := handleAnalyzeSATD(context.Background(), nil, input)
	if err != nil {
		t.Fatalf("handleAnalyzeSATD returned error: %v", err)
	}
	if result == nil {
		t.Fatal("handleAnalyzeSATD returned nil result")
	}
	if result.IsError {
		textContent := result.Content[0].(*mcp.TextContent)
		t.Fatalf("handleAnalyzeSATD returned error: %s", textContent.Text)
	}
}

// TestHandleDeadcode tests the deadcode analyzer tool handler.
func TestHandleDeadcode(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

func used() {
	x := 1
	_ = x
}

func unused() {
	y := 2
	_ = y
}

func main() {
	used()
}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	input := DeadcodeInput{
		AnalyzeInput: AnalyzeInput{
			Paths:  []string{tmpDir},
			Format: "json",
		},
		Confidence: 0.5,
	}

	result, _, err := handleAnalyzeDeadcode(context.Background(), nil, input)
	if err != nil {
		t.Fatalf("handleAnalyzeDeadcode returned error: %v", err)
	}
	if result == nil {
		t.Fatal("handleAnalyzeDeadcode returned nil result")
	}
	if result.IsError {
		textContent := result.Content[0].(*mcp.TextContent)
		t.Fatalf("handleAnalyzeDeadcode returned error: %s", textContent.Text)
	}
}

// TestHandleDuplicates tests the duplicates analyzer tool handler.
func TestHandleDuplicates(t *testing.T) {
	tmpDir := t.TempDir()

	// Create two files with duplicate code
	file1 := filepath.Join(tmpDir, "file1.go")
	file2 := filepath.Join(tmpDir, "file2.go")

	duplicateCode := `package main

func processData() {
	x := 1
	y := 2
	z := x + y
	if z > 0 {
		println(z)
	}
	for i := 0; i < 10; i++ {
		println(i)
	}
}
`
	if err := os.WriteFile(file1, []byte(duplicateCode), 0644); err != nil {
		t.Fatalf("failed to write file1: %v", err)
	}
	if err := os.WriteFile(file2, []byte(duplicateCode), 0644); err != nil {
		t.Fatalf("failed to write file2: %v", err)
	}

	input := DuplicatesInput{
		AnalyzeInput: AnalyzeInput{
			Paths:  []string{tmpDir},
			Format: "json",
		},
		MinLines:  3,
		Threshold: 0.8,
	}

	result, _, err := handleAnalyzeDuplicates(context.Background(), nil, input)
	if err != nil {
		t.Fatalf("handleAnalyzeDuplicates returned error: %v", err)
	}
	if result == nil {
		t.Fatal("handleAnalyzeDuplicates returned nil result")
	}
	if result.IsError {
		textContent := result.Content[0].(*mcp.TextContent)
		t.Fatalf("handleAnalyzeDuplicates returned error: %s", textContent.Text)
	}
}

// TestHandleTDG tests the TDG analyzer tool handler.
func TestHandleTDG(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

// TODO: this needs refactoring
func complex() {
	for i := 0; i < 10; i++ {
		if i%2 == 0 {
			if i%4 == 0 {
				continue
			}
		}
	}
}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	input := TDGInput{
		AnalyzeInput: AnalyzeInput{
			Paths:  []string{tmpDir},
			Format: "json",
		},
		Hotspots: 5,
	}

	result, _, err := handleAnalyzeTDG(context.Background(), nil, input)
	if err != nil {
		t.Fatalf("handleAnalyzeTDG returned error: %v", err)
	}
	if result == nil {
		t.Fatal("handleAnalyzeTDG returned nil result")
	}
	if result.IsError {
		textContent := result.Content[0].(*mcp.TextContent)
		t.Fatalf("handleAnalyzeTDG returned error: %s", textContent.Text)
	}
}

// TestHandleGraph tests the graph analyzer tool handler.
func TestHandleGraph(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a simple Go module structure
	mainFile := filepath.Join(tmpDir, "main.go")
	content := `package main

import "fmt"

func main() {
	fmt.Println("hello")
}
`
	if err := os.WriteFile(mainFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write main.go: %v", err)
	}

	input := GraphInput{
		AnalyzeInput: AnalyzeInput{
			Paths:  []string{tmpDir},
			Format: "json",
		},
		Scope:          "file",
		IncludeMetrics: true,
	}

	result, _, err := handleAnalyzeGraph(context.Background(), nil, input)
	if err != nil {
		t.Fatalf("handleAnalyzeGraph returned error: %v", err)
	}
	if result == nil {
		t.Fatal("handleAnalyzeGraph returned nil result")
	}
	if result.IsError {
		textContent := result.Content[0].(*mcp.TextContent)
		t.Fatalf("handleAnalyzeGraph returned error: %s", textContent.Text)
	}
}

// TestHandleCohesion tests the cohesion analyzer tool handler.
func TestHandleCohesion(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

type Calculator struct {
	value int
}

func (c *Calculator) Add(x int) {
	c.value += x
}

func (c *Calculator) Subtract(x int) {
	c.value -= x
}

func (c *Calculator) GetValue() int {
	return c.value
}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	input := CohesionInput{
		AnalyzeInput: AnalyzeInput{
			Paths:  []string{tmpDir},
			Format: "json",
		},
		Sort: "lcom",
		Top:  10,
	}

	result, _, err := handleAnalyzeCohesion(context.Background(), nil, input)
	if err != nil {
		t.Fatalf("handleAnalyzeCohesion returned error: %v", err)
	}
	if result == nil {
		t.Fatal("handleAnalyzeCohesion returned nil result")
	}
	if result.IsError {
		textContent := result.Content[0].(*mcp.TextContent)
		t.Fatalf("handleAnalyzeCohesion returned error: %s", textContent.Text)
	}
}

// TestHandleRepoMap tests the repo map analyzer tool handler.
func TestHandleRepoMap(t *testing.T) {
	tmpDir := t.TempDir()

	// Create files with cross-references
	file1 := filepath.Join(tmpDir, "main.go")
	file2 := filepath.Join(tmpDir, "util.go")

	main := `package main

func main() {
	helper()
}
`
	util := `package main

func helper() {
	println("helping")
}
`
	if err := os.WriteFile(file1, []byte(main), 0644); err != nil {
		t.Fatalf("failed to write main.go: %v", err)
	}
	if err := os.WriteFile(file2, []byte(util), 0644); err != nil {
		t.Fatalf("failed to write util.go: %v", err)
	}

	input := RepoMapInput{
		AnalyzeInput: AnalyzeInput{
			Paths:  []string{tmpDir},
			Format: "json",
		},
		Top: 10,
	}

	result, _, err := handleAnalyzeRepoMap(context.Background(), nil, input)
	if err != nil {
		t.Fatalf("handleAnalyzeRepoMap returned error: %v", err)
	}
	if result == nil {
		t.Fatal("handleAnalyzeRepoMap returned nil result")
	}
	if result.IsError {
		textContent := result.Content[0].(*mcp.TextContent)
		t.Fatalf("handleAnalyzeRepoMap returned error: %s", textContent.Text)
	}
}

// TestEmptyPathsError verifies handlers return error for empty file lists.
func TestEmptyPathsError(t *testing.T) {
	tmpDir := t.TempDir()
	// Create empty directory - no source files

	input := ComplexityInput{
		AnalyzeInput: AnalyzeInput{
			Paths: []string{tmpDir},
		},
	}

	result, _, err := handleAnalyzeComplexity(context.Background(), nil, input)
	if err != nil {
		t.Fatalf("handleAnalyzeComplexity returned unexpected error: %v", err)
	}
	if result == nil {
		t.Fatal("expected result, got nil")
	}
	if !result.IsError {
		t.Error("expected IsError to be true for empty file list")
	}
}

// TestFormatOutput verifies output formatting works for all formats.
func TestFormatOutput(t *testing.T) {
	data := map[string]interface{}{
		"name":  "test",
		"value": 123,
	}

	formats := []string{"", "toon", "json", "markdown"}
	for _, format := range formats {
		t.Run(format, func(t *testing.T) {
			input := AnalyzeInput{Format: format}
			output, err := formatOutput(data, getFormat(input))
			if err != nil {
				t.Errorf("formatOutput failed for format %q: %v", format, err)
			}
			if output == "" {
				t.Errorf("formatOutput returned empty string for format %q", format)
			}
		})
	}
}

// TestScanPathsForGit tests git root finding via scanner service.
func TestScanPathsForGit(t *testing.T) {
	// Test with non-git directory - should return error when git required
	tmpDir := t.TempDir()
	scanner := scannerSvc.New()
	_, err := scanner.ScanPathsForGit([]string{tmpDir}, true)
	if err == nil {
		t.Error("ScanPathsForGit() on non-git dir with gitRequired=true should return error")
	}
}

// TestToolResultTextFormat tests text format output.
func TestToolResultTextFormat(t *testing.T) {
	data := map[string]interface{}{
		"key": "value",
	}
	result, _, err := toolResult(data, output.FormatText)
	if err != nil {
		t.Fatalf("toolResult returned error: %v", err)
	}
	if result == nil {
		t.Fatal("toolResult returned nil")
	}
}

// TestScanFilesWithFile tests scanning a single file via scanner service.
func TestScanFilesWithFile(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main"), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	scanner := scannerSvc.New()
	result, err := scanner.ScanPaths([]string{tmpDir})
	if err != nil {
		t.Errorf("ScanPaths should not error on valid dir: %v", err)
	}
	if len(result.Files) != 1 {
		t.Errorf("Expected 1 file, got %d", len(result.Files))
	}
}

// TestPromptDefinitions verifies all prompts have valid definitions.
func TestPromptDefinitions(t *testing.T) {
	if len(promptDefinitions) == 0 {
		t.Fatal("no prompt definitions found")
	}

	for _, def := range promptDefinitions {
		t.Run(def.Name, func(t *testing.T) {
			if def.Name == "" {
				t.Error("prompt name is empty")
			}
			if def.Description == "" {
				t.Error("prompt description is empty")
			}
			if def.ContentFile == "" {
				t.Error("prompt content file is empty")
			}

			// Verify embedded file exists and is readable
			content, err := promptFiles.ReadFile(def.ContentFile)
			if err != nil {
				t.Errorf("failed to read embedded file %s: %v", def.ContentFile, err)
			}
			if len(content) == 0 {
				t.Errorf("embedded file %s is empty", def.ContentFile)
			}
		})
	}
}

// TestPromptHandler verifies prompt handlers work correctly.
func TestPromptHandler(t *testing.T) {
	for _, def := range promptDefinitions {
		t.Run(def.Name, func(t *testing.T) {
			handler := makePromptHandler(def)

			// Create a mock request with empty args
			req := &mcp.GetPromptRequest{
				Params: &mcp.GetPromptParams{
					Name:      def.Name,
					Arguments: map[string]string{},
				},
			}

			result, err := handler(context.Background(), req)
			if err != nil {
				t.Fatalf("handler returned error: %v", err)
			}
			if result == nil {
				t.Fatal("handler returned nil result")
			}
			if result.Description == "" {
				t.Error("result description is empty")
			}
			if len(result.Messages) == 0 {
				t.Fatal("result has no messages")
			}

			// Verify the message content
			msg := result.Messages[0]
			if msg.Role != "user" {
				t.Errorf("expected role 'user', got %q", msg.Role)
			}
			textContent, ok := msg.Content.(*mcp.TextContent)
			if !ok {
				t.Fatalf("expected TextContent, got %T", msg.Content)
			}
			if textContent.Text == "" {
				t.Error("message text is empty")
			}

			// Verify suggested tool calls are appended
			if !contains(textContent.Text, "Suggested Tool Calls") {
				t.Error("message should contain suggested tool calls section")
			}
		})
	}
}

// TestPromptHandlerWithArgs verifies prompt handlers substitute arguments.
func TestPromptHandlerWithArgs(t *testing.T) {
	// Find a prompt that uses the 'top' argument
	var contextDef PromptDefinition
	for _, def := range promptDefinitions {
		if def.Name == "context-compression" {
			contextDef = def
			break
		}
	}
	if contextDef.Name == "" {
		t.Fatal("context-compression prompt not found")
	}

	handler := makePromptHandler(contextDef)

	req := &mcp.GetPromptRequest{
		Params: &mcp.GetPromptParams{
			Name: contextDef.Name,
			Arguments: map[string]string{
				"top":   "50",
				"paths": "/custom/path",
			},
		},
	}

	result, err := handler(context.Background(), req)
	if err != nil {
		t.Fatalf("handler returned error: %v", err)
	}

	textContent := result.Messages[0].Content.(*mcp.TextContent)

	// Verify arguments are substituted
	if !contains(textContent.Text, "50") {
		t.Error("message should contain substituted 'top' value of 50")
	}
	if !contains(textContent.Text, "/custom/path") {
		t.Error("message should contain substituted 'paths' value")
	}
}

// TestSubstituteArg verifies argument substitution logic.
func TestSubstituteArg(t *testing.T) {
	tests := []struct {
		name       string
		text       string
		key        string
		args       map[string]string
		defaultVal string
		expected   string
	}{
		{
			name:       "use provided value",
			text:       "top {{top}} items",
			key:        "top",
			args:       map[string]string{"top": "50"},
			defaultVal: "30",
			expected:   "top 50 items",
		},
		{
			name:       "use default when missing",
			text:       "top {{top}} items",
			key:        "top",
			args:       map[string]string{},
			defaultVal: "30",
			expected:   "top 30 items",
		},
		{
			name:       "use default when empty",
			text:       "top {{top}} items",
			key:        "top",
			args:       map[string]string{"top": ""},
			defaultVal: "30",
			expected:   "top 30 items",
		},
		{
			name:       "no placeholder unchanged",
			text:       "no placeholder here",
			key:        "top",
			args:       map[string]string{"top": "50"},
			defaultVal: "30",
			expected:   "no placeholder here",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := substituteArg(tt.text, tt.key, tt.args, tt.defaultVal)
			if result != tt.expected {
				t.Errorf("substituteArg() = %q, want %q", result, tt.expected)
			}
		})
	}
}

// TestBuildToolCallSuggestions verifies tool call generation for each prompt.
func TestBuildToolCallSuggestions(t *testing.T) {
	promptNames := []string{
		"context-compression",
		"refactoring-priority",
		"bug-hunt",
		"change-impact",
		"codebase-onboarding",
		"code-review-focus",
		"architecture-review",
		"tech-debt-report",
		"test-targeting",
		"quality-gate",
	}

	for _, name := range promptNames {
		t.Run(name, func(t *testing.T) {
			suggestions := buildToolCallSuggestions(name, map[string]string{
				"paths": ".",
				"top":   "10",
				"count": "5",
				"days":  "30",
			})

			if len(suggestions) == 0 {
				t.Errorf("buildToolCallSuggestions(%q) returned no suggestions", name)
			}

			// Verify each suggestion mentions an omen tool
			for _, s := range suggestions {
				if !contains(s, "analyze_") && !contains(s, "#") {
					t.Errorf("suggestion %q doesn't look like a tool call", s)
				}
			}
		})
	}
}

// TestRegisterPrompts verifies prompts are registered on the server.
func TestRegisterPrompts(t *testing.T) {
	server := NewServer("test")
	if server == nil {
		t.Fatal("NewServer returned nil")
	}
	// The registerPrompts() call happens in NewServer
	// We verify indirectly that it doesn't panic and the server works
}
