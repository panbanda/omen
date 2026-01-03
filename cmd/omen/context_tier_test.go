package main

import (
	"bytes"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// TestContextTierFlag verifies the --tier flag is recognized
func TestContextTierFlag(t *testing.T) {
	cmd := exec.Command("./omen", "context", "--help")
	cmd.Dir = filepath.Join(os.Getenv("PWD"), "..", "..")
	output, err := cmd.CombinedOutput()
	if err != nil {
		// Build omen first if not found
		build := exec.Command("go", "build", "-o", "omen", "./cmd/omen")
		build.Dir = filepath.Join(os.Getenv("PWD"), "..", "..")
		if buildErr := build.Run(); buildErr != nil {
			t.Fatalf("failed to build omen: %v", buildErr)
		}
		// Retry
		output, err = cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("failed to run omen context --help: %v, output: %s", err, output)
		}
	}

	if !bytes.Contains(output, []byte("--tier")) {
		t.Error("--tier flag not found in help output")
	}
	if !bytes.Contains(output, []byte("essential")) {
		t.Error("essential tier not mentioned in help")
	}
	if !bytes.Contains(output, []byte("standard")) {
		t.Error("standard tier not mentioned in help")
	}
	if !bytes.Contains(output, []byte("full")) {
		t.Error("full tier not mentioned in help")
	}
}

// TestContextTierLimits verifies tier-specific limits
func TestContextTierLimits(t *testing.T) {
	tests := []struct {
		tier             string
		expectedMaxFiles int
		expectedMaxGraph int
		expectedMaxSyms  int
	}{
		{"essential", 20, 10, 10},
		{"standard", 100, 50, 50},
		{"full", 0, 0, 0}, // 0 means unlimited
	}

	for _, tt := range tests {
		t.Run(tt.tier, func(t *testing.T) {
			limits := getTierLimits(tt.tier)
			if limits.MaxFiles != tt.expectedMaxFiles {
				t.Errorf("tier %s: MaxFiles = %d, want %d", tt.tier, limits.MaxFiles, tt.expectedMaxFiles)
			}
			if limits.MaxGraphNodes != tt.expectedMaxGraph {
				t.Errorf("tier %s: MaxGraphNodes = %d, want %d", tt.tier, limits.MaxGraphNodes, tt.expectedMaxGraph)
			}
			if limits.MaxSymbols != tt.expectedMaxSyms {
				t.Errorf("tier %s: MaxSymbols = %d, want %d", tt.tier, limits.MaxSymbols, tt.expectedMaxSyms)
			}
		})
	}
}

// TestContextTierDefault verifies default tier is "standard"
func TestContextTierDefault(t *testing.T) {
	limits := getTierLimits("")
	standardLimits := getTierLimits("standard")

	if limits.MaxFiles != standardLimits.MaxFiles {
		t.Error("empty tier should default to standard")
	}
}

// TestContextTierEssentialOutput verifies essential tier produces less output
func TestContextTierEssentialOutput(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping integration test in short mode")
	}

	// Create temp directory with some files
	tmpDir := t.TempDir()
	for i := 0; i < 50; i++ {
		content := "package main\nfunc foo() {}\n"
		path := filepath.Join(tmpDir, "file"+string(rune('a'+i%26))+".go")
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to create test file: %v", err)
		}
	}

	// Run with essential tier
	cmd := exec.Command("./omen", "context", tmpDir, "--tier", "essential")
	cmd.Dir = filepath.Join(os.Getenv("PWD"), "..", "..")
	essentialOut, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("essential tier failed: %v, output: %s", err, essentialOut)
	}

	// Run with standard tier
	cmd = exec.Command("./omen", "context", tmpDir, "--tier", "standard")
	cmd.Dir = filepath.Join(os.Getenv("PWD"), "..", "..")
	standardOut, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("standard tier failed: %v, output: %s", err, standardOut)
	}

	// Essential should produce less output
	essentialLines := strings.Count(string(essentialOut), "\n")
	standardLines := strings.Count(string(standardOut), "\n")

	if essentialLines >= standardLines {
		t.Errorf("essential tier (%d lines) should produce less output than standard (%d lines)",
			essentialLines, standardLines)
	}
}

// Note: TierLimits and getTierLimits are defined in context.go
