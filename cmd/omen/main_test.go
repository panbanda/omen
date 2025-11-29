package main

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/urfave/cli/v2"
)

// TestGetPaths verifies path handling from CLI arguments.
func TestGetPaths(t *testing.T) {
	tests := []struct {
		name     string
		args     []string
		expected []string
	}{
		{
			name:     "no args defaults to current dir",
			args:     []string{},
			expected: []string{"."},
		},
		{
			name:     "single path",
			args:     []string{"/foo/bar"},
			expected: []string{"/foo/bar"},
		},
		{
			name:     "multiple paths",
			args:     []string{"/foo", "/bar"},
			expected: []string{"/foo", "/bar"},
		},
		{
			name:     "filters out flags",
			args:     []string{"/foo", "-f", "json", "/bar"},
			expected: []string{"/foo", "/bar"},
		},
		{
			name:     "filters out format flag",
			args:     []string{"/foo", "--format", "json"},
			expected: []string{"/foo"},
		},
		{
			name:     "filters out output flag",
			args:     []string{"-o", "out.txt", "/foo"},
			expected: []string{"/foo"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			app := &cli.App{
				Action: func(c *cli.Context) error {
					result := getPaths(c)
					if len(result) != len(tt.expected) {
						t.Errorf("getPaths() = %v, want %v", result, tt.expected)
						return nil
					}
					for i := range result {
						if result[i] != tt.expected[i] {
							t.Errorf("getPaths()[%d] = %q, want %q", i, result[i], tt.expected[i])
						}
					}
					return nil
				},
			}
			args := append([]string{"test"}, tt.args...)
			_ = app.Run(args)
		})
	}
}

// TestGetTrailingFlag verifies trailing flag parsing.
func TestGetTrailingFlag(t *testing.T) {
	tests := []struct {
		name         string
		args         []string
		flagName     string
		shortName    string
		defaultValue string
		expected     string
	}{
		{
			name:         "no flag returns default",
			args:         []string{},
			flagName:     "format",
			shortName:    "f",
			defaultValue: "text",
			expected:     "text",
		},
		{
			name:         "long flag with space",
			args:         []string{"--format", "json"},
			flagName:     "format",
			shortName:    "f",
			defaultValue: "text",
			expected:     "json",
		},
		{
			name:         "short flag with space",
			args:         []string{"-f", "markdown"},
			flagName:     "format",
			shortName:    "f",
			defaultValue: "text",
			expected:     "markdown",
		},
		{
			name:         "long flag with equals",
			args:         []string{"--format=toon"},
			flagName:     "format",
			shortName:    "f",
			defaultValue: "text",
			expected:     "toon",
		},
		{
			name:         "short flag with equals",
			args:         []string{"-f=json"},
			flagName:     "format",
			shortName:    "f",
			defaultValue: "text",
			expected:     "json",
		},
		{
			name:         "trailing flag after positional",
			args:         []string{".", "-f", "json"},
			flagName:     "format",
			shortName:    "f",
			defaultValue: "text",
			expected:     "json",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			app := &cli.App{
				Flags: []cli.Flag{
					&cli.StringFlag{
						Name:    tt.flagName,
						Aliases: []string{tt.shortName},
						Value:   tt.defaultValue,
					},
				},
				Action: func(c *cli.Context) error {
					result := getTrailingFlag(c, tt.flagName, tt.shortName, tt.defaultValue)
					if result != tt.expected {
						t.Errorf("getTrailingFlag() = %q, want %q", result, tt.expected)
					}
					return nil
				},
			}
			args := append([]string{"test"}, tt.args...)
			_ = app.Run(args)
		})
	}
}

// TestValidateDays verifies the days validation function.
func TestValidateDays(t *testing.T) {
	tests := []struct {
		days    int
		wantErr bool
	}{
		{days: 1, wantErr: false},
		{days: 30, wantErr: false},
		{days: 365, wantErr: false},
		{days: 0, wantErr: true},
		{days: -1, wantErr: true},
		{days: -100, wantErr: true},
	}

	for _, tt := range tests {
		err := validateDays(tt.days)
		if (err != nil) != tt.wantErr {
			t.Errorf("validateDays(%d) error = %v, wantErr %v", tt.days, err, tt.wantErr)
		}
	}
}

// TestOutputFlags verifies the output flags are correctly defined.
func TestOutputFlags(t *testing.T) {
	flags := outputFlags()

	if len(flags) != 3 {
		t.Errorf("outputFlags() returned %d flags, want 3", len(flags))
	}

	flagNames := make(map[string]bool)
	for _, f := range flags {
		for _, name := range f.Names() {
			flagNames[name] = true
		}
	}

	required := []string{"format", "f", "output", "o", "no-cache"}
	for _, name := range required {
		if !flagNames[name] {
			t.Errorf("outputFlags() missing flag %q", name)
		}
	}
}

// TestComplexityCommandE2E tests the complexity command end-to-end.
func TestComplexityCommandE2E(t *testing.T) {
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

	app := &cli.App{
		Name:     "omen",
		Metadata: make(map[string]interface{}),
		Commands: []*cli.Command{
			{
				Name: "analyze",
				Subcommands: []*cli.Command{
					{
						Name:   "complexity",
						Flags:  outputFlags(),
						Action: runComplexityCmd,
					},
				},
			},
		},
	}

	err := app.Run([]string{"omen", "analyze", "complexity", "-f", "json", tmpDir})
	if err != nil {
		t.Fatalf("complexity command failed: %v", err)
	}
}

// TestSATDCommandE2E tests the SATD command end-to-end.
func TestSATDCommandE2E(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

// TODO: fix this important bug
func buggy() {
	// HACK: temporary workaround
	x := 1
	_ = x
}

// FIXME: needs refactoring
func broken() {}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	app := &cli.App{
		Name:     "omen",
		Metadata: make(map[string]interface{}),
		Commands: []*cli.Command{
			{
				Name: "analyze",
				Subcommands: []*cli.Command{
					{
						Name:   "satd",
						Flags:  outputFlags(),
						Action: runSATDCmd,
					},
				},
			},
		},
	}

	err := app.Run([]string{"omen", "analyze", "satd", "-f", "json", tmpDir})
	if err != nil {
		t.Fatalf("satd command failed: %v", err)
	}
}

// TestDeadcodeCommandE2E tests the deadcode command end-to-end.
func TestDeadcodeCommandE2E(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

func used() {}

func unused() {}

func main() {
	used()
}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	app := &cli.App{
		Name:     "omen",
		Metadata: make(map[string]interface{}),
		Commands: []*cli.Command{
			{
				Name: "analyze",
				Subcommands: []*cli.Command{
					{
						Name:   "deadcode",
						Flags:  outputFlags(),
						Action: runDeadCodeCmd,
					},
				},
			},
		},
	}

	err := app.Run([]string{"omen", "analyze", "deadcode", "-f", "json", tmpDir})
	if err != nil {
		t.Fatalf("deadcode command failed: %v", err)
	}
}

// TestTDGCommandE2E tests the TDG command end-to-end.
func TestTDGCommandE2E(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

// TODO: needs refactoring
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

	app := &cli.App{
		Name:     "omen",
		Metadata: make(map[string]interface{}),
		Commands: []*cli.Command{
			{
				Name: "analyze",
				Subcommands: []*cli.Command{
					{
						Name:   "tdg",
						Flags:  outputFlags(),
						Action: runTDGCmd,
					},
				},
			},
		},
	}

	err := app.Run([]string{"omen", "analyze", "tdg", "-f", "json", tmpDir})
	if err != nil {
		t.Fatalf("tdg command failed: %v", err)
	}
}

// TestNoFilesError verifies commands handle empty directories gracefully.
func TestNoFilesError(t *testing.T) {
	tmpDir := t.TempDir()

	app := &cli.App{
		Name:     "omen",
		Metadata: make(map[string]interface{}),
		Commands: []*cli.Command{
			{
				Name: "analyze",
				Subcommands: []*cli.Command{
					{
						Name:   "complexity",
						Flags:  outputFlags(),
						Action: runComplexityCmd,
					},
				},
			},
		},
	}

	err := app.Run([]string{"omen", "analyze", "complexity", tmpDir})
	// Should not crash, may return error for no files
	_ = err
}

// TestVersionVariable verifies version variables are defined.
func TestVersionVariable(t *testing.T) {
	// These are set via ldflags at build time
	if version == "" {
		t.Error("version variable should have a default value")
	}
}
