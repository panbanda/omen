package output

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/fatih/color"
	"github.com/olekukonko/tablewriter"
	"github.com/olekukonko/tablewriter/tw"
)

// Format represents an output format.
type Format string

const (
	FormatText     Format = "text"
	FormatJSON     Format = "json"
	FormatMarkdown Format = "markdown"
)

// ParseFormat converts a string to Format, defaulting to text.
func ParseFormat(s string) Format {
	switch strings.ToLower(s) {
	case "json":
		return FormatJSON
	case "markdown", "md":
		return FormatMarkdown
	default:
		return FormatText
	}
}

// Renderable defines data that can render itself in multiple formats.
type Renderable interface {
	RenderText(w io.Writer, colored bool) error
	RenderMarkdown(w io.Writer) error
	// RenderData returns the underlying data for JSON serialization.
	RenderData() any
}

// Formatter handles output formatting.
type Formatter struct {
	format  Format
	writer  io.Writer
	file    *os.File
	colored bool
}

// NewFormatter creates a new formatter.
func NewFormatter(format Format, output string, colored bool) (*Formatter, error) {
	var writer io.Writer = os.Stdout
	var file *os.File

	if output != "" {
		f, err := os.Create(output)
		if err != nil {
			return nil, err
		}
		writer = f
		file = f
		colored = false
	}

	return &Formatter{
		format:  format,
		writer:  writer,
		file:    file,
		colored: colored,
	}, nil
}

// Close closes the formatter's writer if it's a file.
func (f *Formatter) Close() error {
	if f.file != nil {
		return f.file.Close()
	}
	return nil
}

// Writer returns the underlying writer.
func (f *Formatter) Writer() io.Writer {
	return f.writer
}

// Format returns the configured format.
func (f *Formatter) Format() Format {
	return f.format
}

// Colored returns whether colored output is enabled.
func (f *Formatter) Colored() bool {
	return f.colored
}

// Output writes data in the configured format.
func (f *Formatter) Output(data any) error {
	if r, ok := data.(Renderable); ok {
		return f.render(r)
	}
	return f.outputRaw(data)
}

// render dispatches to the appropriate format renderer.
func (f *Formatter) render(r Renderable) error {
	switch f.format {
	case FormatJSON:
		return f.outputJSON(r.RenderData())
	case FormatMarkdown:
		return r.RenderMarkdown(f.writer)
	default:
		return r.RenderText(f.writer, f.colored)
	}
}

// outputRaw handles non-Renderable data.
func (f *Formatter) outputRaw(data any) error {
	switch f.format {
	case FormatJSON:
		return f.outputJSON(data)
	case FormatMarkdown:
		fmt.Fprintln(f.writer, "```json")
		if err := f.outputJSON(data); err != nil {
			return err
		}
		fmt.Fprintln(f.writer, "```")
		return nil
	default:
		return f.outputJSON(data)
	}
}

// outputJSON writes data as formatted JSON.
func (f *Formatter) outputJSON(data any) error {
	encoder := json.NewEncoder(f.writer)
	encoder.SetIndent("", "  ")
	return encoder.Encode(data)
}

// Table is a Renderable table with headers, rows, and optional footer.
type Table struct {
	Title   string     `json:"-"`
	Headers []string   `json:"-"`
	Rows    [][]string `json:"-"`
	Footer  []string   `json:"-"`
	Data    any        `json:"data,omitempty"`
}

// NewTable creates a table that wraps structured data for serialization.
func NewTable(title string, headers []string, rows [][]string, footer []string, data any) *Table {
	return &Table{
		Title:   title,
		Headers: headers,
		Rows:    rows,
		Footer:  footer,
		Data:    data,
	}
}

func (t *Table) RenderData() any {
	if t.Data != nil {
		return t.Data
	}
	result := make([]map[string]string, len(t.Rows))
	for i, row := range t.Rows {
		m := make(map[string]string)
		for j, h := range t.Headers {
			if j < len(row) {
				m[h] = row[j]
			}
		}
		result[i] = m
	}
	return result
}

func (t *Table) RenderText(w io.Writer, colored bool) error {
	if t.Title != "" {
		if colored {
			color.New(color.Bold).Fprintln(w, t.Title)
		} else {
			fmt.Fprintln(w, t.Title)
		}
		fmt.Fprintln(w, strings.Repeat("=", len(t.Title)))
		fmt.Fprintln(w)
	}

	table := tablewriter.NewTable(w,
		tablewriter.WithConfig(tablewriter.Config{
			Header: tw.CellConfig{
				Alignment: tw.CellAlignment{Global: tw.AlignLeft},
				Formatting: tw.CellFormatting{
					AutoFormat: tw.On,
				},
			},
			Row: tw.CellConfig{
				Alignment: tw.CellAlignment{Global: tw.AlignLeft},
			},
			Footer: tw.CellConfig{
				Alignment: tw.CellAlignment{Global: tw.AlignLeft},
			},
		}),
		tablewriter.WithRendition(tw.Rendition{
			Borders: tw.Border{
				Left:   tw.Off,
				Right:  tw.Off,
				Top:    tw.Off,
				Bottom: tw.Off,
			},
			Settings: tw.Settings{
				Separators: tw.Separators{
					BetweenColumns: tw.Off,
				},
			},
		}),
	)

	table.Header(t.Headers)
	for _, row := range t.Rows {
		table.Append(row)
	}
	if len(t.Footer) > 0 {
		footerArgs := make([]any, len(t.Footer))
		for i, f := range t.Footer {
			footerArgs[i] = f
		}
		table.Footer(footerArgs...)
	}
	table.Render()
	fmt.Fprintln(w)
	return nil
}

func (t *Table) RenderMarkdown(w io.Writer) error {
	if t.Title != "" {
		fmt.Fprintf(w, "## %s\n\n", t.Title)
	}

	fmt.Fprintf(w, "| %s |\n", strings.Join(t.Headers, " | "))

	seps := make([]string, len(t.Headers))
	for i := range seps {
		seps[i] = "---"
	}
	fmt.Fprintf(w, "| %s |\n", strings.Join(seps, " | "))

	for _, row := range t.Rows {
		fmt.Fprintf(w, "| %s |\n", strings.Join(row, " | "))
	}

	if len(t.Footer) > 0 {
		fmt.Fprintf(w, "| %s |\n", strings.Join(t.Footer, " | "))
	}

	fmt.Fprintln(w)
	return nil
}

// Section is a Renderable titled section with content and subsections.
type Section struct {
	Title    string    `json:"title,omitempty"`
	Content  string    `json:"content,omitempty"`
	Sections []Section `json:"sections,omitempty"`
	Data     any       `json:"data,omitempty"`
}

func (s *Section) RenderData() any {
	if s.Data != nil {
		return s.Data
	}
	return s
}

func (s *Section) RenderText(w io.Writer, colored bool) error {
	return s.renderTextAtLevel(w, colored, 0)
}

func (s *Section) renderTextAtLevel(w io.Writer, colored bool, level int) error {
	if s.Title != "" {
		if colored {
			color.New(color.Bold).Fprintln(w, s.Title)
		} else {
			fmt.Fprintln(w, s.Title)
		}
		underline := "="
		if level > 0 {
			underline = "-"
		}
		fmt.Fprintln(w, strings.Repeat(underline, len(s.Title)))
	}

	if s.Content != "" {
		fmt.Fprintln(w, s.Content)
	}

	for _, sub := range s.Sections {
		fmt.Fprintln(w)
		sub.renderTextAtLevel(w, colored, level+1)
	}

	return nil
}

func (s *Section) RenderMarkdown(w io.Writer) error {
	return s.renderMarkdownAtLevel(w, 2)
}

func (s *Section) renderMarkdownAtLevel(w io.Writer, level int) error {
	if s.Title != "" {
		fmt.Fprintf(w, "%s %s\n\n", strings.Repeat("#", level), s.Title)
	}

	if s.Content != "" {
		fmt.Fprintln(w, s.Content)
		fmt.Fprintln(w)
	}

	for _, sub := range s.Sections {
		sub.renderMarkdownAtLevel(w, level+1)
	}

	return nil
}

// Report is a compound Renderable containing multiple sections and tables.
type Report struct {
	Title    string       `json:"title,omitempty"`
	Sections []Renderable `json:"-"`
	Data     any          `json:"data,omitempty"`
}

func (r *Report) RenderData() any {
	if r.Data != nil {
		return r.Data
	}
	parts := make([]any, len(r.Sections))
	for i, s := range r.Sections {
		parts[i] = s.RenderData()
	}
	return map[string]any{
		"title":    r.Title,
		"sections": parts,
	}
}

func (r *Report) RenderText(w io.Writer, colored bool) error {
	if r.Title != "" {
		if colored {
			color.New(color.Bold, color.FgCyan).Fprintln(w, r.Title)
		} else {
			fmt.Fprintln(w, r.Title)
		}
		fmt.Fprintln(w, strings.Repeat("=", len(r.Title)))
		fmt.Fprintln(w)
	}

	for i, s := range r.Sections {
		if err := s.RenderText(w, colored); err != nil {
			return err
		}
		if i < len(r.Sections)-1 {
			fmt.Fprintln(w)
		}
	}
	return nil
}

func (r *Report) RenderMarkdown(w io.Writer) error {
	if r.Title != "" {
		fmt.Fprintf(w, "# %s\n\n", r.Title)
	}

	for _, s := range r.Sections {
		if err := s.RenderMarkdown(w); err != nil {
			return err
		}
	}
	return nil
}

// Message helpers for colored output

func (f *Formatter) Success(format string, args ...any) {
	if f.colored {
		color.Green(format, args...)
	} else {
		fmt.Fprintf(f.writer, format+"\n", args...)
	}
}

func (f *Formatter) Warning(format string, args ...any) {
	if f.colored {
		color.Yellow(format, args...)
	} else {
		fmt.Fprintf(f.writer, "WARNING: "+format+"\n", args...)
	}
}

func (f *Formatter) Error(format string, args ...any) {
	if f.colored {
		color.Red(format, args...)
	} else {
		fmt.Fprintf(f.writer, "ERROR: "+format+"\n", args...)
	}
}

func (f *Formatter) Info(format string, args ...any) {
	if f.colored {
		color.Cyan(format, args...)
	} else {
		fmt.Fprintf(f.writer, format+"\n", args...)
	}
}

// SeverityColor returns a colored string based on severity level.
func SeverityColor(severity, text string) string {
	switch strings.ToLower(severity) {
	case "critical", "high", "high_risk":
		return color.RedString(text)
	case "medium", "moderate":
		return color.YellowString(text)
	case "low", "good", "excellent":
		return color.GreenString(text)
	default:
		return text
	}
}

// Legacy type aliases for backwards compatibility

type TextTable = Table
type TextSection = Section
