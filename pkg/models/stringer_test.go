package models

import (
	"testing"
)

func TestStringerMethods(t *testing.T) {
	t.Run("ConfidenceLevel", func(t *testing.T) {
		c := ConfidenceLevel("high")
		if c.String() != "high" {
			t.Errorf("ConfidenceLevel.String() = %q, want %q", c.String(), "high")
		}
	})

	t.Run("DeadCodeType", func(t *testing.T) {
		d := DeadCodeType("function")
		if d.String() != "function" {
			t.Errorf("DeadCodeType.String() = %q, want %q", d.String(), "function")
		}
	})

	t.Run("ReferenceType", func(t *testing.T) {
		r := ReferenceType("call")
		if r.String() != "call" {
			t.Errorf("ReferenceType.String() = %q, want %q", r.String(), "call")
		}
	})

	t.Run("DeadCodeKind", func(t *testing.T) {
		d := DeadCodeKind("unused")
		if d.String() != "unused" {
			t.Errorf("DeadCodeKind.String() = %q, want %q", d.String(), "unused")
		}
	})

	t.Run("Grade", func(t *testing.T) {
		g := GradeA
		if g.String() != "A" {
			t.Errorf("Grade.String() = %q, want %q", g.String(), "A")
		}
	})

	t.Run("MetricCategory", func(t *testing.T) {
		m := MetricStructuralComplexity
		if m.String() != "structural_complexity" {
			t.Errorf("MetricCategory.String() = %q, want %q", m.String(), "structural_complexity")
		}
	})

	t.Run("Language", func(t *testing.T) {
		l := LanguageGo
		if l.String() != "go" {
			t.Errorf("Language.String() = %q, want %q", l.String(), "go")
		}
	})

	t.Run("TDGSeverity", func(t *testing.T) {
		s := TDGSeverityCritical
		if s.String() != "critical" {
			t.Errorf("TDGSeverity.String() = %q, want %q", s.String(), "critical")
		}
	})

	t.Run("CloneType", func(t *testing.T) {
		c := CloneType1
		if c.String() != "type1" {
			t.Errorf("CloneType.String() = %q, want %q", c.String(), "type1")
		}
	})

	t.Run("ViolationSeverity", func(t *testing.T) {
		v := ViolationSeverity("error")
		if v.String() != "error" {
			t.Errorf("ViolationSeverity.String() = %q, want %q", v.String(), "error")
		}
	})

	t.Run("RiskLevel", func(t *testing.T) {
		r := RiskHigh
		if r.String() != "high" {
			t.Errorf("RiskLevel.String() = %q, want %q", r.String(), "high")
		}
	})

	t.Run("NodeType", func(t *testing.T) {
		n := NodeFile
		if n.String() != "file" {
			t.Errorf("NodeType.String() = %q, want %q", n.String(), "file")
		}
	})

	t.Run("EdgeType", func(t *testing.T) {
		e := EdgeCall
		if e.String() != "call" {
			t.Errorf("EdgeType.String() = %q, want %q", e.String(), "call")
		}
	})

	t.Run("PenaltyCurve", func(t *testing.T) {
		p := PenaltyCurve("linear")
		if p.String() != "linear" {
			t.Errorf("PenaltyCurve.String() = %q, want %q", p.String(), "linear")
		}
	})
}
