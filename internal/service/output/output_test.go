package output

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestNew(t *testing.T) {
	svc, err := New()
	if err != nil {
		t.Fatalf("New() error = %v", err)
	}
	if svc == nil {
		t.Fatal("New() returned nil")
	}
	if svc.format != FormatText {
		t.Errorf("expected default format %v, got %v", FormatText, svc.format)
	}
	if svc.colored != true {
		t.Error("expected default colored = true")
	}
}

func TestNewWithFormat(t *testing.T) {
	svc, err := New(WithFormat(FormatJSON))
	if err != nil {
		t.Fatalf("New() error = %v", err)
	}
	if svc.Format() != FormatJSON {
		t.Errorf("expected format %v, got %v", FormatJSON, svc.Format())
	}
}

func TestNewWithWriter(t *testing.T) {
	var buf bytes.Buffer
	svc, err := New(WithWriter(&buf))
	if err != nil {
		t.Fatalf("New() error = %v", err)
	}
	if svc.Writer() != &buf {
		t.Error("expected writer to be set")
	}
}

func TestNewWithColor(t *testing.T) {
	svc, err := New(WithColor(false))
	if err != nil {
		t.Fatalf("New() error = %v", err)
	}
	if svc.Colored() != false {
		t.Error("expected colored = false")
	}
}

func TestNewWithFile(t *testing.T) {
	tmpDir := t.TempDir()
	filePath := filepath.Join(tmpDir, "output.txt")

	svc, err := New(WithFile(filePath))
	if err != nil {
		t.Fatalf("New() error = %v", err)
	}
	defer svc.Close()

	if svc.Colored() {
		t.Error("expected colored = false when writing to file")
	}
	if svc.file == nil {
		t.Error("expected file to be opened")
	}
}

func TestNewWithFile_Invalid(t *testing.T) {
	_, err := New(WithFile("/nonexistent/dir/file.txt"))
	if err == nil {
		t.Error("expected error for invalid file path")
	}
}

func TestClose(t *testing.T) {
	tmpDir := t.TempDir()
	filePath := filepath.Join(tmpDir, "output.txt")

	svc, err := New(WithFile(filePath))
	if err != nil {
		t.Fatalf("New() error = %v", err)
	}

	if err := svc.Close(); err != nil {
		t.Errorf("Close() error = %v", err)
	}

	// Close again should be safe
	svc.file = nil
	if err := svc.Close(); err != nil {
		t.Errorf("Close() on nil file error = %v", err)
	}
}

func TestFormatData_JSON(t *testing.T) {
	svc, _ := New(WithFormat(FormatJSON))
	data := map[string]int{"count": 42}

	result, err := svc.FormatData(data)
	if err != nil {
		t.Fatalf("FormatData() error = %v", err)
	}
	if result == "" {
		t.Error("expected non-empty result")
	}
}

func TestFormatData_Markdown(t *testing.T) {
	svc, _ := New(WithFormat(FormatMarkdown))
	data := map[string]int{"count": 42}

	result, err := svc.FormatData(data)
	if err != nil {
		t.Fatalf("FormatData() error = %v", err)
	}
	if result == "" {
		t.Error("expected non-empty result")
	}
	if len(result) < 10 || result[:3] != "```" {
		t.Error("expected markdown code block")
	}
}

func TestFormatData_TOON(t *testing.T) {
	svc, _ := New(WithFormat(FormatTOON))
	data := map[string]int{"count": 42}

	result, err := svc.FormatData(data)
	if err != nil {
		t.Fatalf("FormatData() error = %v", err)
	}
	if result == "" {
		t.Error("expected non-empty result")
	}
}

func TestFormatData_Text(t *testing.T) {
	svc, _ := New(WithFormat(FormatText))
	data := map[string]int{"count": 42}

	result, err := svc.FormatData(data)
	if err != nil {
		t.Fatalf("FormatData() error = %v", err)
	}
	if result == "" {
		t.Error("expected non-empty result")
	}
}

func TestOutput(t *testing.T) {
	var buf bytes.Buffer
	svc, _ := New(WithWriter(&buf), WithFormat(FormatJSON))

	data := map[string]string{"message": "hello"}
	if err := svc.Output(data); err != nil {
		t.Fatalf("Output() error = %v", err)
	}

	if buf.Len() == 0 {
		t.Error("expected output to be written")
	}
}

func TestOutput_ToFile(t *testing.T) {
	tmpDir := t.TempDir()
	filePath := filepath.Join(tmpDir, "output.txt")

	svc, _ := New(WithFile(filePath), WithFormat(FormatJSON))
	defer svc.Close()

	data := map[string]string{"message": "hello"}
	if err := svc.Output(data); err != nil {
		t.Fatalf("Output() error = %v", err)
	}

	svc.Close()

	content, err := os.ReadFile(filePath)
	if err != nil {
		t.Fatalf("ReadFile() error = %v", err)
	}
	if len(content) == 0 {
		t.Error("expected file to have content")
	}
}

func TestParseFormat(t *testing.T) {
	tests := []struct {
		input    string
		expected Format
	}{
		{"text", FormatText},
		{"json", FormatJSON},
		{"markdown", FormatMarkdown},
		{"toon", FormatTOON},
		{"", FormatText},
		{"unknown", FormatText},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			result := ParseFormat(tt.input)
			if result != tt.expected {
				t.Errorf("ParseFormat(%q) = %v, want %v", tt.input, result, tt.expected)
			}
		})
	}
}

func TestNewTable(t *testing.T) {
	table := NewTable(
		"Test Table",
		[]string{"Col1", "Col2"},
		[][]string{{"a", "b"}, {"c", "d"}},
		[]string{"Summary 1"},
		nil,
	)
	if table == nil {
		t.Fatal("NewTable() returned nil")
	}
}
