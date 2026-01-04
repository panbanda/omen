// Package ast provides an abstraction layer for AST parsing that supports
// multiple implementations including tree-sitter (multi-language) and
// go/ast (Go-specific with type information).
//
// The Provider interface abstracts the parsing mechanism, allowing analyzers
// to work with any implementation. Tree-sitter provides broad language support
// while go/ast provides precise type information for Go code.
//
// Usage:
//
//	provider := treesitter.New()
//	defer provider.Close()
//
//	file, err := provider.Parse("main.go")
//	if err != nil {
//	    return err
//	}
//
//	for _, fn := range file.Functions() {
//	    fmt.Printf("%s at line %d\n", fn.Name, fn.Line)
//	}
package ast
