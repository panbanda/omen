package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestReportCommandExists(t *testing.T) {
	// Verify the report command is registered
	cmd, _, err := rootCmd.Find([]string{"report"})
	if err != nil {
		t.Fatalf("report command not found: %v", err)
	}
	if cmd.Use != "report" {
		t.Errorf("command Use = %q, want %q", cmd.Use, "report")
	}
}

func TestReportGenerateCommandExists(t *testing.T) {
	cmd, _, err := rootCmd.Find([]string{"report", "generate"})
	if err != nil {
		t.Fatalf("report generate command not found: %v", err)
	}
	if cmd.Name() != "generate" {
		t.Errorf("command Name() = %q, want %q", cmd.Name(), "generate")
	}
}

func TestReportValidateCommandExists(t *testing.T) {
	cmd, _, err := rootCmd.Find([]string{"report", "validate"})
	if err != nil {
		t.Fatalf("report validate command not found: %v", err)
	}
	if cmd.Use != "validate" {
		t.Errorf("command Use = %q, want %q", cmd.Use, "validate")
	}
}

func TestReportRenderCommandExists(t *testing.T) {
	cmd, _, err := rootCmd.Find([]string{"report", "render"})
	if err != nil {
		t.Fatalf("report render command not found: %v", err)
	}
	if cmd.Use != "render" {
		t.Errorf("command Use = %q, want %q", cmd.Use, "render")
	}
}

func TestReportServeCommandExists(t *testing.T) {
	cmd, _, err := rootCmd.Find([]string{"report", "serve"})
	if err != nil {
		t.Fatalf("report serve command not found: %v", err)
	}
	if cmd.Use != "serve" {
		t.Errorf("command Use = %q, want %q", cmd.Use, "serve")
	}
}

func TestReportGenerateFlags(t *testing.T) {
	cmd, _, _ := rootCmd.Find([]string{"report", "generate"})

	// Check -o/--output flag exists
	outputFlag := cmd.Flags().Lookup("output")
	if outputFlag == nil {
		t.Error("--output flag not found")
	}
	if outputFlag != nil && outputFlag.Shorthand != "o" {
		t.Errorf("--output shorthand = %q, want %q", outputFlag.Shorthand, "o")
	}

	// Check --since flag exists
	sinceFlag := cmd.Flags().Lookup("since")
	if sinceFlag == nil {
		t.Error("--since flag not found")
	}
	if sinceFlag != nil && sinceFlag.DefValue != "1y" {
		t.Errorf("--since default = %q, want %q", sinceFlag.DefValue, "1y")
	}
}

func TestReportValidateFlags(t *testing.T) {
	cmd, _, _ := rootCmd.Find([]string{"report", "validate"})

	// Check -d/--data flag exists
	dataFlag := cmd.Flags().Lookup("data")
	if dataFlag == nil {
		t.Error("--data flag not found")
	}
	if dataFlag != nil && dataFlag.Shorthand != "d" {
		t.Errorf("--data shorthand = %q, want %q", dataFlag.Shorthand, "d")
	}
}

func TestReportRenderFlags(t *testing.T) {
	cmd, _, _ := rootCmd.Find([]string{"report", "render"})

	// Check -d/--data flag exists
	dataFlag := cmd.Flags().Lookup("data")
	if dataFlag == nil {
		t.Error("--data flag not found")
	}

	// Check -o/--output flag exists
	outputFlag := cmd.Flags().Lookup("output")
	if outputFlag == nil {
		t.Error("--output flag not found")
	}

	// Check --skip-validate flag exists
	skipFlag := cmd.Flags().Lookup("skip-validate")
	if skipFlag == nil {
		t.Error("--skip-validate flag not found")
	}
}

func TestReportServeFlags(t *testing.T) {
	cmd, _, _ := rootCmd.Find([]string{"report", "serve"})

	// Check -d/--data flag exists
	dataFlag := cmd.Flags().Lookup("data")
	if dataFlag == nil {
		t.Error("--data flag not found")
	}

	// Check -p/--port flag exists
	portFlag := cmd.Flags().Lookup("port")
	if portFlag == nil {
		t.Error("--port flag not found")
	}
	if portFlag != nil && portFlag.Shorthand != "p" {
		t.Errorf("--port shorthand = %q, want %q", portFlag.Shorthand, "p")
	}
	if portFlag != nil && portFlag.DefValue != "8080" {
		t.Errorf("--port default = %q, want %q", portFlag.DefValue, "8080")
	}
}

func TestReportGenerateCreatesDirectory(t *testing.T) {
	tmpDir := t.TempDir()
	outputDir := filepath.Join(tmpDir, "test-report")

	// Create a minimal Go file to analyze
	testFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(testFile, []byte("package main\nfunc main() {}\n"), 0644); err != nil {
		t.Fatalf("failed to create test file: %v", err)
	}

	rootCmd.SetArgs([]string{"report", "generate", "-o", outputDir, "--since", "1m", tmpDir})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("report generate failed: %v", err)
	}

	// Verify directory was created
	if _, err := os.Stat(outputDir); os.IsNotExist(err) {
		t.Error("output directory was not created")
	}

	// Verify metadata.json was created
	metadataPath := filepath.Join(outputDir, "metadata.json")
	if _, err := os.Stat(metadataPath); os.IsNotExist(err) {
		t.Error("metadata.json was not created")
	}

	// Verify score.json was created
	scorePath := filepath.Join(outputDir, "score.json")
	if _, err := os.Stat(scorePath); os.IsNotExist(err) {
		t.Error("score.json was not created")
	}
}

func TestReportGenerateOutputsAllDataFiles(t *testing.T) {
	tmpDir := t.TempDir()
	outputDir := filepath.Join(tmpDir, "test-report")

	// Create test files
	testFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(testFile, []byte("package main\n\n// TODO: fix this\nfunc main() {\n\tfor i := 0; i < 10; i++ {\n\t\tif i%2 == 0 {\n\t\t\tcontinue\n\t\t}\n\t}\n}\n"), 0644); err != nil {
		t.Fatalf("failed to create test file: %v", err)
	}

	rootCmd.SetArgs([]string{"report", "generate", "-o", outputDir, "--since", "1m", tmpDir})
	if err := rootCmd.Execute(); err != nil {
		t.Fatalf("report generate failed: %v", err)
	}

	// Expected data files
	expectedFiles := []string{
		"metadata.json",
		"score.json",
		"complexity.json",
		"satd.json",
		"duplicates.json",
		"smells.json",
		"cohesion.json",
		"flags.json",
	}

	for _, file := range expectedFiles {
		path := filepath.Join(outputDir, file)
		if _, err := os.Stat(path); os.IsNotExist(err) {
			t.Errorf("%s was not created", file)
		}
	}
}

func TestReportValidateValidDataDir(t *testing.T) {
	tmpDir := t.TempDir()

	// Create minimal valid data files
	files := map[string]string{
		"metadata.json":   `{"repository":"test","generated_at":"2024-12-10T00:00:00Z","since":"1y","omen_version":"1.0.0","paths":["."]}`,
		"score.json":      `{"score":85,"passed":true}`,
		"complexity.json": `{"files":[]}`,
		"satd.json":       `{"items":[]}`,
		"duplicates.json": `{"clone_groups":[]}`,
		"smells.json":     `{"smells":[]}`,
		"cohesion.json":   `{"classes":[]}`,
		"flags.json":      `{"flags":[]}`,
	}

	for name, content := range files {
		if err := os.WriteFile(filepath.Join(tmpDir, name), []byte(content), 0644); err != nil {
			t.Fatalf("failed to create %s: %v", name, err)
		}
	}

	rootCmd.SetArgs([]string{"report", "validate", "-d", tmpDir})
	err := rootCmd.Execute()
	if err != nil {
		t.Errorf("validate failed on valid data: %v", err)
	}
}

func TestReportValidateMissingFiles(t *testing.T) {
	tmpDir := t.TempDir()

	// Create only metadata.json
	if err := os.WriteFile(filepath.Join(tmpDir, "metadata.json"), []byte(`{"repository":"test"}`), 0644); err != nil {
		t.Fatalf("failed to create metadata.json: %v", err)
	}

	rootCmd.SetArgs([]string{"report", "validate", "-d", tmpDir})
	err := rootCmd.Execute()
	if err == nil {
		t.Error("validate should fail with missing files")
	}
}

func TestReportValidateInvalidJSON(t *testing.T) {
	tmpDir := t.TempDir()

	// Create invalid JSON
	if err := os.WriteFile(filepath.Join(tmpDir, "metadata.json"), []byte(`{invalid json}`), 0644); err != nil {
		t.Fatalf("failed to create metadata.json: %v", err)
	}

	rootCmd.SetArgs([]string{"report", "validate", "-d", tmpDir})
	err := rootCmd.Execute()
	if err == nil {
		t.Error("validate should fail with invalid JSON")
	}
}

func TestReportRenderCreatesHTML(t *testing.T) {
	tmpDir := t.TempDir()
	outputFile := filepath.Join(tmpDir, "output.html")

	// Create minimal valid data files
	files := map[string]string{
		"metadata.json":   `{"repository":"test-repo","generated_at":"2024-12-10T00:00:00Z","since":"1y","omen_version":"1.0.0","paths":["."]}`,
		"score.json":      `{"score":85,"passed":true,"components":{"complexity":90,"duplication":80,"satd":75,"coupling":95,"smells":100,"cohesion":85}}`,
		"complexity.json": `{"files":[],"summary":{"average_cyclomatic":5,"average_cognitive":8}}`,
		"satd.json":       `{"items":[],"summary":{"total":10,"by_severity":{"critical":1,"high":2,"medium":3,"low":4}}}`,
		"duplicates.json": `{"clone_groups":[],"summary":{"duplication_ratio":0.05}}`,
		"smells.json":     `{"smells":[]}`,
		"cohesion.json":   `{"classes":[]}`,
		"flags.json":      `{"flags":[]}`,
	}

	for name, content := range files {
		if err := os.WriteFile(filepath.Join(tmpDir, name), []byte(content), 0644); err != nil {
			t.Fatalf("failed to create %s: %v", name, err)
		}
	}

	rootCmd.SetArgs([]string{"report", "render", "-d", tmpDir, "-o", outputFile, "--skip-validate"})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("render failed: %v", err)
	}

	// Verify output file was created
	if _, err := os.Stat(outputFile); os.IsNotExist(err) {
		t.Error("output HTML file was not created")
	}

	// Verify it contains expected content
	content, err := os.ReadFile(outputFile)
	if err != nil {
		t.Fatalf("failed to read output file: %v", err)
	}

	// Check for basic HTML structure
	if !strings.Contains(string(content), "<!DOCTYPE html>") {
		t.Error("output should contain DOCTYPE")
	}
	if !strings.Contains(string(content), "test-repo") {
		t.Error("output should contain repository name")
	}
	if !strings.Contains(string(content), "85") {
		t.Error("output should contain score")
	}
}

func TestReportRenderWithInsights(t *testing.T) {
	tmpDir := t.TempDir()
	outputFile := filepath.Join(tmpDir, "output.html")

	// Create minimal valid data files
	files := map[string]string{
		"metadata.json":   `{"repository":"test-repo","generated_at":"2024-12-10T00:00:00Z","since":"1y","omen_version":"1.0.0","paths":["."]}`,
		"score.json":      `{"score":85,"passed":true}`,
		"complexity.json": `{"files":[]}`,
		"satd.json":       `{"items":[]}`,
		"duplicates.json": `{"clone_groups":[]}`,
		"smells.json":     `{"smells":[]}`,
		"cohesion.json":   `{"classes":[]}`,
		"flags.json":      `{"flags":[]}`,
	}

	for name, content := range files {
		if err := os.WriteFile(filepath.Join(tmpDir, name), []byte(content), 0644); err != nil {
			t.Fatalf("failed to create %s: %v", name, err)
		}
	}

	// Create insights directory and files
	insightsDir := filepath.Join(tmpDir, "insights")
	if err := os.MkdirAll(insightsDir, 0755); err != nil {
		t.Fatalf("failed to create insights directory: %v", err)
	}

	insightFiles := map[string]string{
		"summary.json": `{"executive_summary":"Test summary","key_findings":["Finding 1"],"recommendations":{"high_priority":[],"medium_priority":[],"ongoing":[]}}`,
	}

	for name, content := range insightFiles {
		if err := os.WriteFile(filepath.Join(insightsDir, name), []byte(content), 0644); err != nil {
			t.Fatalf("failed to create insights/%s: %v", name, err)
		}
	}

	rootCmd.SetArgs([]string{"report", "render", "-d", tmpDir, "-o", outputFile, "--skip-validate"})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("render failed: %v", err)
	}

	// Verify output contains insight content
	content, err := os.ReadFile(outputFile)
	if err != nil {
		t.Fatalf("failed to read output file: %v", err)
	}

	if !strings.Contains(string(content), "Test summary") {
		t.Error("output should contain executive summary from insights")
	}
}

func TestReportServeRequiresDataFlag(t *testing.T) {
	// Save and restore flag value
	oldDataDir := reportDataDir
	t.Cleanup(func() { reportDataDir = oldDataDir })

	reportDataDir = ""
	rootCmd.SetArgs([]string{"report", "serve"})
	err := rootCmd.Execute()
	if err == nil {
		t.Error("serve should fail without --data flag")
	}
	// Check that error message mentions --data flag
	if err != nil && !strings.Contains(err.Error(), "--data") {
		t.Errorf("error should mention --data flag, got: %v", err)
	}
}
