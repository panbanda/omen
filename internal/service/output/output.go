package output

import (
	"io"
	"os"

	"github.com/panbanda/omen/internal/output"
	toon "github.com/toon-format/toon-go"
)

// Format represents output format.
type Format = output.Format

// Supported formats (re-exported for convenience).
const (
	FormatText     = output.FormatText
	FormatJSON     = output.FormatJSON
	FormatMarkdown = output.FormatMarkdown
	FormatTOON     = output.FormatTOON
)

// Service handles output formatting.
type Service struct {
	format   Format
	writer   io.Writer
	colored  bool
	filePath string
	file     *os.File
}

// Option configures a Service.
type Option func(*Service)

// WithFormat sets the output format.
func WithFormat(f Format) Option {
	return func(s *Service) {
		s.format = f
	}
}

// WithWriter sets the output writer.
func WithWriter(w io.Writer) Option {
	return func(s *Service) {
		s.writer = w
	}
}

// WithColor enables or disables colored output.
func WithColor(enabled bool) Option {
	return func(s *Service) {
		s.colored = enabled
	}
}

// WithFile sets output to a file.
func WithFile(path string) Option {
	return func(s *Service) {
		s.filePath = path
	}
}

// New creates a new output service.
func New(opts ...Option) (*Service, error) {
	s := &Service{
		format:  FormatText,
		writer:  os.Stdout,
		colored: true,
	}

	for _, opt := range opts {
		opt(s)
	}

	// If file path is set, open the file
	if s.filePath != "" {
		f, err := os.Create(s.filePath)
		if err != nil {
			return nil, err
		}
		s.file = f
		s.writer = f
		s.colored = false // No colors when writing to file
	}

	return s, nil
}

// Close closes the output service and any open files.
func (s *Service) Close() error {
	if s.file != nil {
		return s.file.Close()
	}
	return nil
}

// Format returns the current format.
func (s *Service) Format() Format {
	return s.format
}

// Writer returns the current writer.
func (s *Service) Writer() io.Writer {
	return s.writer
}

// Colored returns whether output should be colored.
func (s *Service) Colored() bool {
	return s.colored
}

// FormatData formats data according to the service's format.
func (s *Service) FormatData(data any) (string, error) {
	switch s.format {
	case FormatJSON:
		out, err := toon.Marshal(data, toon.WithIndent(2))
		if err != nil {
			return "", err
		}
		return string(out), nil
	case FormatMarkdown:
		out, err := toon.Marshal(data, toon.WithIndent(2))
		if err != nil {
			return "", err
		}
		return "```\n" + string(out) + "\n```", nil
	case FormatTOON:
		out, err := toon.Marshal(data, toon.WithIndent(2))
		if err != nil {
			return "", err
		}
		return string(out), nil
	default:
		// Text format - use toon as fallback
		out, err := toon.Marshal(data, toon.WithIndent(2))
		if err != nil {
			return "", err
		}
		return string(out), nil
	}
}

// Output writes formatted data to the writer.
func (s *Service) Output(data any) error {
	formatted, err := s.FormatData(data)
	if err != nil {
		return err
	}

	_, err = s.writer.Write([]byte(formatted))
	return err
}

// OutputTable outputs data as a table using the underlying output package.
func (s *Service) OutputTable(table *output.Table) error {
	formatter, err := output.NewFormatter(s.format, s.filePath, s.colored)
	if err != nil {
		return err
	}
	defer formatter.Close()

	return formatter.Output(table)
}

// NewTable creates a new table for output.
func NewTable(title string, headers []string, rows [][]string, summary []string, rawData any) *output.Table {
	return output.NewTable(title, headers, rows, summary, rawData)
}

// ParseFormat parses a format string into a Format.
func ParseFormat(s string) Format {
	return output.ParseFormat(s)
}
