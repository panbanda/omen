package semantic

import (
	"testing"

	"github.com/panbanda/omen/pkg/parser"
)

func TestForLanguage_SupportedLanguages(t *testing.T) {
	tests := []struct {
		lang     parser.Language
		wantNil  bool
		langName string
	}{
		{parser.LangRuby, false, "Ruby"},
		{parser.LangGo, false, "Go"},
		{parser.LangTypeScript, false, "TypeScript"},
		{parser.LangTSX, false, "TSX"},
		{parser.LangJavaScript, false, "JavaScript"},
	}

	for _, tt := range tests {
		t.Run(tt.langName, func(t *testing.T) {
			ext := ForLanguage(tt.lang)
			if tt.wantNil && ext != nil {
				t.Errorf("ForLanguage(%s) = %v, want nil", tt.langName, ext)
			}
			if !tt.wantNil && ext == nil {
				t.Errorf("ForLanguage(%s) = nil, want non-nil", tt.langName)
			}
			if ext != nil {
				ext.Close()
			}
		})
	}
}

func TestForLanguage_UnsupportedLanguages(t *testing.T) {
	unsupported := []struct {
		lang     parser.Language
		langName string
	}{
		{parser.LangC, "C"},
		{parser.LangCPP, "C++"},
		{parser.LangJava, "Java"},
		{parser.LangPython, "Python"},
		{parser.LangRust, "Rust"},
		{parser.LangPHP, "PHP"},
		{parser.LangCSharp, "C#"},
		{parser.LangBash, "Bash"},
	}

	for _, tt := range unsupported {
		t.Run(tt.langName, func(t *testing.T) {
			ext := ForLanguage(tt.lang)
			if ext != nil {
				ext.Close()
				t.Errorf("ForLanguage(%s) = %v, want nil for unsupported language", tt.langName, ext)
			}
		})
	}
}

func TestSupportedLanguages(t *testing.T) {
	supported := SupportedLanguages()

	if len(supported) == 0 {
		t.Error("SupportedLanguages() returned empty slice")
	}

	// Verify the expected languages are in the list
	expected := map[parser.Language]bool{
		parser.LangRuby:       true,
		parser.LangGo:         true,
		parser.LangTypeScript: true,
		parser.LangTSX:        true,
		parser.LangJavaScript: true,
	}

	for _, lang := range supported {
		if !expected[lang] {
			t.Errorf("SupportedLanguages() contains unexpected language: %v", lang)
		}
		delete(expected, lang)
	}

	for lang := range expected {
		t.Errorf("SupportedLanguages() missing expected language: %v", lang)
	}
}

func TestRefKind_String(t *testing.T) {
	tests := []struct {
		kind RefKind
		want string
	}{
		{RefCallback, "callback"},
		{RefDecorator, "decorator"},
		{RefFunctionValue, "function_value"},
		{RefDynamicCall, "dynamic_call"},
		{RefKind(99), "unknown"},
	}

	for _, tt := range tests {
		t.Run(tt.want, func(t *testing.T) {
			if got := tt.kind.String(); got != tt.want {
				t.Errorf("RefKind(%d).String() = %q, want %q", tt.kind, got, tt.want)
			}
		})
	}
}
