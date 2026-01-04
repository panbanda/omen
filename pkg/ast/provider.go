package ast

import (
	"errors"
)

// ErrTypesUnavailable is returned when type information is requested
// from a provider that doesn't support it (e.g., tree-sitter).
var ErrTypesUnavailable = errors.New("type information not available")

// ErrUnsupportedLanguage is returned when parsing a file with an unsupported language.
var ErrUnsupportedLanguage = errors.New("unsupported language")

// Language represents a programming language.
type Language string

const (
	LangGo         Language = "go"
	LangRust       Language = "rust"
	LangPython     Language = "python"
	LangTypeScript Language = "typescript"
	LangJavaScript Language = "javascript"
	LangTSX        Language = "tsx"
	LangJava       Language = "java"
	LangC          Language = "c"
	LangCPP        Language = "cpp"
	LangCSharp     Language = "csharp"
	LangRuby       Language = "ruby"
	LangPHP        Language = "php"
	LangBash       Language = "bash"
	LangUnknown    Language = "unknown"
)

// Position represents a location in source code.
type Position struct {
	File   string
	Line   int
	Column int
	Offset int
}

// SymbolKind represents the type of a symbol.
type SymbolKind string

const (
	SymbolFunction  SymbolKind = "function"
	SymbolMethod    SymbolKind = "method"
	SymbolType      SymbolKind = "type"
	SymbolVariable  SymbolKind = "variable"
	SymbolConstant  SymbolKind = "constant"
	SymbolInterface SymbolKind = "interface"
	SymbolStruct    SymbolKind = "struct"
	SymbolClass     SymbolKind = "class"
)

// Symbol represents a named code element.
type Symbol struct {
	Name     string
	Kind     SymbolKind
	Package  string // Empty for tree-sitter (unknown)
	Type     string // Empty for tree-sitter (unknown)
	Exported bool
	Pos      Position
}

// FunctionDecl represents a function or method declaration.
type FunctionDecl struct {
	Name       string
	Receiver   string // Empty if not a method
	Parameters []string
	Returns    []string
	Signature  string
	Pos        Position
	EndLine    int
}

// CallInfo represents a function or method call.
type CallInfo struct {
	Callee   string
	Receiver string // Empty if not a method call
	Args     int
	Pos      Position
}

// Import represents an import statement.
type Import struct {
	Path  string
	Alias string
	Pos   Position
}

// Provider abstracts AST access with optional type information.
type Provider interface {
	// Parse parses a file and returns syntax-level information.
	Parse(path string) (File, error)

	// ParseWithTypes parses a file with type information.
	// Returns ErrTypesUnavailable for providers that don't support types.
	ParseWithTypes(path string) (TypedFile, error)

	// Language returns the detected language for a file path.
	Language(path string) Language

	// Close releases provider resources.
	Close()
}

// File provides syntax-level access to parsed code.
type File interface {
	// Path returns the file path.
	Path() string

	// Language returns the detected language.
	Language() Language

	// Functions returns all function/method declarations.
	Functions() []FunctionDecl

	// Calls returns all function/method calls.
	Calls() []CallInfo

	// Symbols returns all symbol definitions.
	Symbols() []Symbol

	// Imports returns all import statements.
	Imports() []Import
}

// TypedFile extends File with type information (Go-specific).
type TypedFile interface {
	File

	// HasTypes returns true if type information is available.
	HasTypes() bool

	// ResolveSymbol resolves an identifier at the given position to its definition.
	ResolveSymbol(pos Position) (*Symbol, error)

	// MethodSet returns all methods for a named type.
	MethodSet(typeName string) []Symbol

	// Implements checks if a type implements an interface.
	Implements(typeName, interfaceName string) bool
}
