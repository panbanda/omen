package output

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestParseFormat(t *testing.T) {
	tests := []struct {
		input string
		want  Format
	}{
		{"text", FormatText},
		{"TEXT", FormatText},
		{"json", FormatJSON},
		{"JSON", FormatJSON},
		{"markdown", FormatMarkdown},
		{"md", FormatMarkdown},
		{"MARKDOWN", FormatMarkdown},
		{"", FormatText},
		{"invalid", FormatText},
		{"unknown", FormatText},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got := ParseFormat(tt.input)
			if got != tt.want {
				t.Errorf("ParseFormat(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestNewFormatter(t *testing.T) {
	tests := []struct {
		name    string
		format  Format
		output  string
		colored bool
	}{
		{"text_stdout_colored", FormatText, "", true},
		{"json_stdout_nocolor", FormatJSON, "", false},
		{"markdown_stdout_colored", FormatMarkdown, "", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			f, err := NewFormatter(tt.format, tt.output, tt.colored)
			if err != nil {
				t.Fatalf("NewFormatter() error: %v", err)
			}
			defer f.Close()

			if f.format != tt.format {
				t.Errorf("format = %q, want %q", f.format, tt.format)
			}

			if f.colored != tt.colored {
				t.Errorf("colored = %v, want %v", f.colored, tt.colored)
			}

			if f.file != nil {
				t.Error("file should be nil for stdout")
			}

			if f.Writer() == nil {
				t.Error("Writer() should not be nil")
			}
		})
	}
}

func TestNewFormatterWithFile(t *testing.T) {
	tmpDir := t.TempDir()
	outputPath := filepath.Join(tmpDir, "output.txt")

	f, err := NewFormatter(FormatJSON, outputPath, true)
	if err != nil {
		t.Fatalf("NewFormatter() error: %v", err)
	}

	if f.file == nil {
		t.Error("file should not be nil for file output")
	}

	if f.colored {
		t.Error("colored should be false when writing to file")
	}

	if err := f.Close(); err != nil {
		t.Errorf("Close() error: %v", err)
	}

	if _, err := os.Stat(outputPath); os.IsNotExist(err) {
		t.Error("output file should exist")
	}
}

func TestNewFormatterInvalidPath(t *testing.T) {
	_, err := NewFormatter(FormatText, "/nonexistent/directory/file.txt", false)
	if err == nil {
		t.Error("NewFormatter() should error for invalid path")
	}
}

func TestFormatterClose(t *testing.T) {
	t.Run("close_stdout", func(t *testing.T) {
		f, err := NewFormatter(FormatText, "", false)
		if err != nil {
			t.Fatalf("NewFormatter() error: %v", err)
		}

		if err := f.Close(); err != nil {
			t.Errorf("Close() should not error for stdout: %v", err)
		}
	})

	t.Run("close_file", func(t *testing.T) {
		tmpDir := t.TempDir()
		f, err := NewFormatter(FormatJSON, filepath.Join(tmpDir, "test.txt"), false)
		if err != nil {
			t.Fatalf("NewFormatter() error: %v", err)
		}

		if err := f.Close(); err != nil {
			t.Errorf("Close() error: %v", err)
		}
	})
}

func TestFormatterGetters(t *testing.T) {
	f, err := NewFormatter(FormatMarkdown, "", true)
	if err != nil {
		t.Fatalf("NewFormatter() error: %v", err)
	}
	defer f.Close()

	if f.Format() != FormatMarkdown {
		t.Errorf("Format() = %q, want %q", f.Format(), FormatMarkdown)
	}

	if !f.Colored() {
		t.Error("Colored() = false, want true")
	}

	if f.Writer() == nil {
		t.Error("Writer() should not be nil")
	}
}

func TestTableRenderText(t *testing.T) {
	tests := []struct {
		name    string
		table   *Table
		colored bool
		want    []string
	}{
		{
			name: "simple_table",
			table: NewTable(
				"Test Results",
				[]string{"File", "Status", "Score"},
				[][]string{
					{"file1.go", "Pass", "100"},
					{"file2.go", "Fail", "50"},
				},
				nil,
				nil,
			),
			colored: false,
			want:    []string{"Test Results", "FILE", "STATUS", "SCORE", "file1.go", "Pass", "100"},
		},
		{
			name: "table_with_footer",
			table: NewTable(
				"Summary",
				[]string{"Metric", "Value"},
				[][]string{
					{"Total", "10"},
					{"Passed", "8"},
				},
				[]string{"Success Rate", "80%"},
				nil,
			),
			colored: false,
			want:    []string{"Summary", "METRIC", "VALUE", "Total", "10", "80%"},
		},
		{
			name: "empty_table",
			table: NewTable(
				"Empty",
				[]string{"Col1", "Col2"},
				[][]string{},
				nil,
				nil,
			),
			colored: false,
			want:    []string{"Empty", "COL 1", "COL 2"},
		},
		{
			name: "no_title",
			table: NewTable(
				"",
				[]string{"A", "B"},
				[][]string{{"1", "2"}},
				nil,
				nil,
			),
			colored: false,
			want:    []string{"A", "B", "1", "2"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var buf bytes.Buffer
			err := tt.table.RenderText(&buf, tt.colored)
			if err != nil {
				t.Fatalf("RenderText() error: %v", err)
			}

			output := buf.String()
			for _, want := range tt.want {
				if !strings.Contains(output, want) {
					t.Errorf("RenderText() missing %q in output:\n%s", want, output)
				}
			}
		})
	}
}

func TestTableRenderMarkdown(t *testing.T) {
	tests := []struct {
		name  string
		table *Table
		want  []string
	}{
		{
			name: "simple_markdown",
			table: NewTable(
				"Results",
				[]string{"Name", "Value"},
				[][]string{{"foo", "bar"}},
				nil,
				nil,
			),
			want: []string{"## Results", "| Name | Value |", "| --- | --- |", "| foo | bar |"},
		},
		{
			name: "with_footer",
			table: NewTable(
				"Data",
				[]string{"X", "Y"},
				[][]string{{"1", "2"}},
				[]string{"Total", "3"},
				nil,
			),
			want: []string{"## Data", "| X | Y |", "| 1 | 2 |", "| Total | 3 |"},
		},
		{
			name: "no_title",
			table: NewTable(
				"",
				[]string{"A"},
				[][]string{{"B"}},
				nil,
				nil,
			),
			want: []string{"| A |", "| --- |", "| B |"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var buf bytes.Buffer
			err := tt.table.RenderMarkdown(&buf)
			if err != nil {
				t.Fatalf("RenderMarkdown() error: %v", err)
			}

			output := buf.String()
			for _, want := range tt.want {
				if !strings.Contains(output, want) {
					t.Errorf("RenderMarkdown() missing %q in output:\n%s", want, output)
				}
			}
		})
	}
}

func TestTableRenderData(t *testing.T) {
	t.Run("with_data_field", func(t *testing.T) {
		data := map[string]any{"custom": "data"}
		table := NewTable("Title", []string{"H1"}, [][]string{{"R1"}}, nil, data)

		result := table.RenderData()
		resultMap, ok := result.(map[string]any)
		if !ok {
			t.Error("RenderData() should return the Data field when set")
		}
		if resultMap["custom"] != "data" {
			t.Error("RenderData() should return the correct data")
		}
	})

	t.Run("without_data_field", func(t *testing.T) {
		table := NewTable(
			"Test",
			[]string{"Name", "Value"},
			[][]string{
				{"foo", "100"},
				{"bar", "200"},
			},
			nil,
			nil,
		)

		result := table.RenderData()
		rows, ok := result.([]map[string]string)
		if !ok {
			t.Fatalf("RenderData() should return []map[string]string, got %T", result)
		}

		if len(rows) != 2 {
			t.Errorf("RenderData() returned %d rows, want 2", len(rows))
		}

		if rows[0]["Name"] != "foo" || rows[0]["Value"] != "100" {
			t.Errorf("RenderData() row 0 = %v, want {Name: foo, Value: 100}", rows[0])
		}
	})

	t.Run("mismatched_columns", func(t *testing.T) {
		table := NewTable(
			"Test",
			[]string{"A", "B", "C"},
			[][]string{{"1", "2"}},
			nil,
			nil,
		)

		result := table.RenderData()
		rows := result.([]map[string]string)

		if len(rows[0]) != 2 {
			t.Errorf("RenderData() should handle missing columns, got %v", rows[0])
		}
	})
}

func TestSectionRenderText(t *testing.T) {
	tests := []struct {
		name    string
		section *Section
		colored bool
		want    []string
	}{
		{
			name: "simple_section",
			section: &Section{
				Title:   "Overview",
				Content: "This is the content.",
			},
			colored: false,
			want:    []string{"Overview", "===", "This is the content."},
		},
		{
			name: "nested_sections",
			section: &Section{
				Title:   "Parent",
				Content: "Parent content",
				Sections: []Section{
					{
						Title:   "Child",
						Content: "Child content",
					},
				},
			},
			colored: false,
			want:    []string{"Parent", "===", "Parent content", "Child", "---", "Child content"},
		},
		{
			name: "no_title",
			section: &Section{
				Content: "Just content",
			},
			colored: false,
			want:    []string{"Just content"},
		},
		{
			name: "no_content",
			section: &Section{
				Title: "Just Title",
			},
			colored: false,
			want:    []string{"Just Title", "==="},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var buf bytes.Buffer
			err := tt.section.RenderText(&buf, tt.colored)
			if err != nil {
				t.Fatalf("RenderText() error: %v", err)
			}

			output := buf.String()
			for _, want := range tt.want {
				if !strings.Contains(output, want) {
					t.Errorf("RenderText() missing %q in output:\n%s", want, output)
				}
			}
		})
	}
}

func TestSectionRenderMarkdown(t *testing.T) {
	tests := []struct {
		name    string
		section *Section
		want    []string
	}{
		{
			name: "simple",
			section: &Section{
				Title:   "Title",
				Content: "Content here",
			},
			want: []string{"## Title", "Content here"},
		},
		{
			name: "nested",
			section: &Section{
				Title:   "Level 1",
				Content: "L1 content",
				Sections: []Section{
					{
						Title:   "Level 2",
						Content: "L2 content",
					},
				},
			},
			want: []string{"## Level 1", "L1 content", "### Level 2", "L2 content"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var buf bytes.Buffer
			err := tt.section.RenderMarkdown(&buf)
			if err != nil {
				t.Fatalf("RenderMarkdown() error: %v", err)
			}

			output := buf.String()
			for _, want := range tt.want {
				if !strings.Contains(output, want) {
					t.Errorf("RenderMarkdown() missing %q in output:\n%s", want, output)
				}
			}
		})
	}
}

func TestSectionRenderData(t *testing.T) {
	t.Run("with_data", func(t *testing.T) {
		data := map[string]any{"test": "value"}
		section := &Section{Data: data}

		result := section.RenderData()
		resultMap, ok := result.(map[string]any)
		if !ok {
			t.Error("RenderData() should return Data field when set")
		}
		if resultMap["test"] != "value" {
			t.Error("RenderData() should return the correct data")
		}
	})

	t.Run("without_data", func(t *testing.T) {
		section := &Section{
			Title:   "Test",
			Content: "Content",
		}

		result := section.RenderData()
		if result != section {
			t.Error("RenderData() should return section itself when Data is nil")
		}
	})
}

func TestReportRenderText(t *testing.T) {
	report := &Report{
		Title: "Analysis Report",
		Sections: []Renderable{
			&Section{
				Title:   "Summary",
				Content: "Overall summary",
			},
			NewTable(
				"Results",
				[]string{"File", "Score"},
				[][]string{{"test.go", "100"}},
				nil,
				nil,
			),
		},
	}

	var buf bytes.Buffer
	err := report.RenderText(&buf, false)
	if err != nil {
		t.Fatalf("RenderText() error: %v", err)
	}

	output := buf.String()
	want := []string{"Analysis Report", "Summary", "Overall summary", "Results", "FILE", "SCORE", "test.go", "100"}
	for _, w := range want {
		if !strings.Contains(output, w) {
			t.Errorf("RenderText() missing %q in output:\n%s", w, output)
		}
	}
}

func TestReportRenderMarkdown(t *testing.T) {
	report := &Report{
		Title: "Report Title",
		Sections: []Renderable{
			&Section{Title: "Section 1", Content: "Content 1"},
			&Section{Title: "Section 2", Content: "Content 2"},
		},
	}

	var buf bytes.Buffer
	err := report.RenderMarkdown(&buf)
	if err != nil {
		t.Fatalf("RenderMarkdown() error: %v", err)
	}

	output := buf.String()
	want := []string{"# Report Title", "## Section 1", "Content 1", "## Section 2", "Content 2"}
	for _, w := range want {
		if !strings.Contains(output, w) {
			t.Errorf("RenderMarkdown() missing %q in output:\n%s", w, output)
		}
	}
}

func TestReportRenderData(t *testing.T) {
	t.Run("with_data", func(t *testing.T) {
		data := map[string]any{"custom": "report"}
		report := &Report{Data: data}

		result := report.RenderData()
		resultMap, ok := result.(map[string]any)
		if !ok {
			t.Error("RenderData() should return Data field when set")
		}
		if resultMap["custom"] != "report" {
			t.Error("RenderData() should return the correct data")
		}
	})

	t.Run("without_data", func(t *testing.T) {
		report := &Report{
			Title: "Test Report",
			Sections: []Renderable{
				&Section{Title: "S1"},
			},
		}

		result := report.RenderData()
		m, ok := result.(map[string]any)
		if !ok {
			t.Fatalf("RenderData() should return map[string]any, got %T", result)
		}

		if m["title"] != "Test Report" {
			t.Errorf("title = %v, want %v", m["title"], "Test Report")
		}

		sections, ok := m["sections"].([]any)
		if !ok || len(sections) != 1 {
			t.Errorf("sections = %v, want 1 section", sections)
		}
	})
}

func TestFormatterOutputRenderable(t *testing.T) {
	tests := []struct {
		name   string
		format Format
		data   Renderable
	}{
		{
			name:   "text_table",
			format: FormatText,
			data:   NewTable("Test", []string{"A"}, [][]string{{"1"}}, nil, nil),
		},
		{
			name:   "json_table",
			format: FormatJSON,
			data:   NewTable("Test", []string{"A"}, [][]string{{"1"}}, nil, nil),
		},
		{
			name:   "markdown_table",
			format: FormatMarkdown,
			data:   NewTable("Test", []string{"A"}, [][]string{{"1"}}, nil, nil),
		},
		{
			name:   "text_section",
			format: FormatText,
			data:   &Section{Title: "Test", Content: "Content"},
		},
		{
			name:   "json_section",
			format: FormatJSON,
			data:   &Section{Title: "Test", Content: "Content"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			outputPath := filepath.Join(tmpDir, "output.txt")

			f, err := NewFormatter(tt.format, outputPath, false)
			if err != nil {
				t.Fatalf("NewFormatter() error: %v", err)
			}
			defer f.Close()

			err = f.Output(tt.data)
			if err != nil {
				t.Errorf("Output() error: %v", err)
			}
		})
	}
}

func TestFormatterOutputRaw(t *testing.T) {
	tests := []struct {
		name   string
		format Format
		data   any
	}{
		{
			name:   "json_map",
			format: FormatJSON,
			data:   map[string]string{"key": "value"},
		},
		{
			name:   "json_struct",
			format: FormatJSON,
			data:   struct{ Name string }{Name: "test"},
		},
		{
			name:   "markdown_data",
			format: FormatMarkdown,
			data:   map[string]int{"count": 42},
		},
		{
			name:   "text_default",
			format: FormatText,
			data:   map[string]bool{"enabled": true},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			outputPath := filepath.Join(tmpDir, "output.txt")

			f, err := NewFormatter(tt.format, outputPath, false)
			if err != nil {
				t.Fatalf("NewFormatter() error: %v", err)
			}
			defer f.Close()

			err = f.Output(tt.data)
			if err != nil {
				t.Errorf("Output() error: %v", err)
			}

			content, err := os.ReadFile(outputPath)
			if err != nil {
				t.Fatalf("ReadFile() error: %v", err)
			}

			if len(content) == 0 {
				t.Error("Output file should not be empty")
			}
		})
	}
}

func TestFormatterOutputJSON(t *testing.T) {
	tmpDir := t.TempDir()
	outputPath := filepath.Join(tmpDir, "test.json")

	f, err := NewFormatter(FormatJSON, outputPath, false)
	if err != nil {
		t.Fatalf("NewFormatter() error: %v", err)
	}
	defer f.Close()

	data := map[string]any{
		"name":  "test",
		"value": 123,
		"items": []string{"a", "b", "c"},
	}

	err = f.outputJSON(data)
	if err != nil {
		t.Fatalf("outputJSON() error: %v", err)
	}

	f.Close()

	content, err := os.ReadFile(outputPath)
	if err != nil {
		t.Fatalf("ReadFile() error: %v", err)
	}

	var result map[string]any
	if err := json.Unmarshal(content, &result); err != nil {
		t.Fatalf("Unmarshal() error: %v", err)
	}

	if result["name"] != "test" {
		t.Errorf("name = %v, want test", result["name"])
	}

	if result["value"].(float64) != 123 {
		t.Errorf("value = %v, want 123", result["value"])
	}
}

func TestFormatterMessageMethods(t *testing.T) {
	tests := []struct {
		name    string
		method  func(*Formatter, string, ...any)
		format  string
		args    []any
		colored bool
		want    string
	}{
		{
			name:    "success_uncolored",
			method:  (*Formatter).Success,
			format:  "Operation completed",
			colored: false,
			want:    "Operation completed",
		},
		{
			name:    "warning_uncolored",
			method:  (*Formatter).Warning,
			format:  "Low disk space",
			colored: false,
			want:    "WARNING: Low disk space",
		},
		{
			name:    "error_uncolored",
			method:  (*Formatter).Error,
			format:  "File not found",
			colored: false,
			want:    "ERROR: File not found",
		},
		{
			name:    "info_uncolored",
			method:  (*Formatter).Info,
			format:  "Processing %d files",
			args:    []any{5},
			colored: false,
			want:    "Processing 5 files",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			outputPath := filepath.Join(tmpDir, "output.txt")

			f, err := NewFormatter(FormatText, outputPath, tt.colored)
			if err != nil {
				t.Fatalf("NewFormatter() error: %v", err)
			}
			defer f.Close()

			tt.method(f, tt.format, tt.args...)
			f.Close()

			content, err := os.ReadFile(outputPath)
			if err != nil {
				t.Fatalf("ReadFile() error: %v", err)
			}

			output := string(content)
			if !strings.Contains(output, tt.want) {
				t.Errorf("output = %q, want to contain %q", output, tt.want)
			}
		})
	}
}

func TestSeverityColor(t *testing.T) {
	tests := []struct {
		severity string
		text     string
	}{
		{"critical", "Critical Issue"},
		{"high", "High Priority"},
		{"high_risk", "High Risk"},
		{"medium", "Medium Level"},
		{"moderate", "Moderate Impact"},
		{"low", "Low Priority"},
		{"good", "Good Status"},
		{"excellent", "Excellent"},
		{"unknown", "Unknown"},
		{"", "Empty"},
	}

	for _, tt := range tests {
		t.Run(tt.severity, func(t *testing.T) {
			result := SeverityColor(tt.severity, tt.text)
			if result == "" {
				t.Error("SeverityColor() returned empty string")
			}
		})
	}
}

func TestFormatterOutputEmptyData(t *testing.T) {
	tests := []struct {
		name   string
		format Format
		data   Renderable
	}{
		{
			name:   "empty_table",
			format: FormatJSON,
			data:   NewTable("", []string{}, [][]string{}, nil, nil),
		},
		{
			name:   "empty_section",
			format: FormatText,
			data:   &Section{},
		},
		{
			name:   "empty_report",
			format: FormatMarkdown,
			data:   &Report{},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			outputPath := filepath.Join(tmpDir, "output.txt")

			f, err := NewFormatter(tt.format, outputPath, false)
			if err != nil {
				t.Fatalf("NewFormatter() error: %v", err)
			}
			defer f.Close()

			err = f.Output(tt.data)
			if err != nil {
				t.Errorf("Output() error with empty data: %v", err)
			}
		})
	}
}

func TestFormatterNilInputs(t *testing.T) {
	tmpDir := t.TempDir()
	outputPath := filepath.Join(tmpDir, "output.txt")

	f, err := NewFormatter(FormatJSON, outputPath, false)
	if err != nil {
		t.Fatalf("NewFormatter() error: %v", err)
	}
	defer f.Close()

	var nilMap map[string]any
	err = f.Output(nilMap)
	if err != nil {
		t.Errorf("Output() should handle nil map: %v", err)
	}
}

func TestTableRenderTextColored(t *testing.T) {
	table := NewTable(
		"Colored Output",
		[]string{"Name", "Value"},
		[][]string{{"test", "123"}},
		nil,
		nil,
	)

	var buf bytes.Buffer
	err := table.RenderText(&buf, true)
	if err != nil {
		t.Fatalf("RenderText() error: %v", err)
	}

	output := buf.String()
	if len(output) == 0 {
		t.Error("RenderText() with colored=true should produce output")
	}
}

func TestSectionRenderTextColored(t *testing.T) {
	section := &Section{
		Title:   "Colored Section",
		Content: "Some content",
	}

	var buf bytes.Buffer
	err := section.RenderText(&buf, true)
	if err != nil {
		t.Fatalf("RenderText() error: %v", err)
	}

	output := buf.String()
	if len(output) == 0 {
		t.Error("RenderText() with colored=true should produce output")
	}
}

func TestReportRenderTextColored(t *testing.T) {
	report := &Report{
		Title: "Colored Report",
		Sections: []Renderable{
			&Section{Title: "Section", Content: "Content"},
		},
	}

	var buf bytes.Buffer
	err := report.RenderText(&buf, true)
	if err != nil {
		t.Fatalf("RenderText() error: %v", err)
	}

	output := buf.String()
	if len(output) == 0 {
		t.Error("RenderText() with colored=true should produce output")
	}
}

func TestFormatterComplexReport(t *testing.T) {
	report := &Report{
		Title: "Comprehensive Analysis",
		Sections: []Renderable{
			&Section{
				Title:   "Overview",
				Content: "Analysis of codebase",
				Sections: []Section{
					{Title: "Subsection", Content: "Details"},
				},
			},
			NewTable(
				"Metrics",
				[]string{"Metric", "Value", "Status"},
				[][]string{
					{"Complexity", "15", "Medium"},
					{"Coverage", "85%", "Good"},
				},
				[]string{"Total", "2", "OK"},
				nil,
			),
			&Section{
				Title:   "Recommendations",
				Content: "Improve test coverage",
			},
		},
	}

	formats := []Format{FormatText, FormatJSON, FormatMarkdown}

	for _, format := range formats {
		t.Run(string(format), func(t *testing.T) {
			tmpDir := t.TempDir()
			outputPath := filepath.Join(tmpDir, "complex."+string(format))

			f, err := NewFormatter(format, outputPath, false)
			if err != nil {
				t.Fatalf("NewFormatter() error: %v", err)
			}
			defer f.Close()

			err = f.Output(report)
			if err != nil {
				t.Errorf("Output() error for %s: %v", format, err)
			}

			f.Close()

			content, err := os.ReadFile(outputPath)
			if err != nil {
				t.Fatalf("ReadFile() error: %v", err)
			}

			if len(content) == 0 {
				t.Errorf("Output file for %s should not be empty", format)
			}
		})
	}
}

func TestFormatterMarkdownRawData(t *testing.T) {
	tmpDir := t.TempDir()
	outputPath := filepath.Join(tmpDir, "markdown.md")

	f, err := NewFormatter(FormatMarkdown, outputPath, false)
	if err != nil {
		t.Fatalf("NewFormatter() error: %v", err)
	}
	defer f.Close()

	data := map[string]string{"key": "value"}
	err = f.Output(data)
	if err != nil {
		t.Fatalf("Output() error: %v", err)
	}

	f.Close()

	content, err := os.ReadFile(outputPath)
	if err != nil {
		t.Fatalf("ReadFile() error: %v", err)
	}

	output := string(content)
	if !strings.Contains(output, "```json") {
		t.Error("Markdown output for raw data should contain json code block")
	}

	if !strings.Contains(output, "```") {
		t.Error("Markdown output should close code block")
	}
}

func TestFormatterMultipleOutputs(t *testing.T) {
	tmpDir := t.TempDir()
	outputPath := filepath.Join(tmpDir, "multiple.txt")

	f, err := NewFormatter(FormatText, outputPath, false)
	if err != nil {
		t.Fatalf("NewFormatter() error: %v", err)
	}
	defer f.Close()

	section1 := &Section{Title: "First", Content: "Content 1"}
	section2 := &Section{Title: "Second", Content: "Content 2"}

	if err := f.Output(section1); err != nil {
		t.Errorf("First Output() error: %v", err)
	}

	if err := f.Output(section2); err != nil {
		t.Errorf("Second Output() error: %v", err)
	}

	f.Close()

	content, err := os.ReadFile(outputPath)
	if err != nil {
		t.Fatalf("ReadFile() error: %v", err)
	}

	output := string(content)
	if !strings.Contains(output, "First") || !strings.Contains(output, "Second") {
		t.Error("Multiple outputs should both be written to file")
	}
}
