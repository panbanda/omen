package models

// String methods for all custom string types.
// These are required for toon serialization, which uses fmt.Stringer.

// ConfidenceLevel
func (c ConfidenceLevel) String() string { return string(c) }

// DeadCodeType
func (d DeadCodeType) String() string { return string(d) }

// ReferenceType
func (r ReferenceType) String() string { return string(r) }

// DeadCodeKind
func (d DeadCodeKind) String() string { return string(d) }

// Grade
func (g Grade) String() string { return string(g) }

// MetricCategory
func (m MetricCategory) String() string { return string(m) }

// Language
func (l Language) String() string { return string(l) }

// TDGSeverity
func (t TDGSeverity) String() string { return string(t) }

// CloneType
func (c CloneType) String() string { return string(c) }

// ViolationSeverity
func (v ViolationSeverity) String() string { return string(v) }

// RiskLevel
func (r RiskLevel) String() string { return string(r) }

// NodeType
func (n NodeType) String() string { return string(n) }

// EdgeType
func (e EdgeType) String() string { return string(e) }

// PenaltyCurve
func (pc PenaltyCurve) String() string { return string(pc) }
