package models

import "testing"

func TestNewSATDSummary(t *testing.T) {
	s := NewSATDSummary()

	if s.BySeverity == nil {
		t.Error("BySeverity should be initialized")
	}
	if s.ByCategory == nil {
		t.Error("ByCategory should be initialized")
	}
	if s.ByFile == nil {
		t.Error("ByFile should be initialized")
	}

	if len(s.BySeverity) != 0 {
		t.Error("BySeverity should be empty")
	}
	if len(s.ByCategory) != 0 {
		t.Error("ByCategory should be empty")
	}
	if len(s.ByFile) != 0 {
		t.Error("ByFile should be empty")
	}
}

func TestSATDSummary_AddItem(t *testing.T) {
	tests := []struct {
		name                   string
		items                  []TechnicalDebt
		expectedTotal          int
		expectedSeverityCrit   int
		expectedSeverityHigh   int
		expectedCategoryDefect int
		expectedFileCount      int
	}{
		{
			name: "single item",
			items: []TechnicalDebt{
				{
					File:     "file1.go",
					Severity: SeverityHigh,
					Category: DebtDefect,
				},
			},
			expectedTotal:          1,
			expectedSeverityHigh:   1,
			expectedCategoryDefect: 1,
			expectedFileCount:      1,
		},
		{
			name: "multiple items same file",
			items: []TechnicalDebt{
				{File: "file1.go", Severity: SeverityHigh, Category: DebtDefect},
				{File: "file1.go", Severity: SeverityCritical, Category: DebtSecurity},
			},
			expectedTotal:        2,
			expectedSeverityCrit: 1,
			expectedSeverityHigh: 1,
			expectedFileCount:    1,
		},
		{
			name: "multiple items different files",
			items: []TechnicalDebt{
				{File: "file1.go", Severity: SeverityHigh, Category: DebtDefect},
				{File: "file2.go", Severity: SeverityMedium, Category: DebtDesign},
				{File: "file3.go", Severity: SeverityLow, Category: DebtTest},
			},
			expectedTotal:     3,
			expectedFileCount: 3,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := NewSATDSummary()

			for _, item := range tt.items {
				s.AddItem(item)
			}

			if s.TotalItems != tt.expectedTotal {
				t.Errorf("TotalItems = %d, expected %d", s.TotalItems, tt.expectedTotal)
			}

			if tt.expectedSeverityCrit > 0 && s.BySeverity[string(SeverityCritical)] != tt.expectedSeverityCrit {
				t.Errorf("BySeverity[critical] = %d, expected %d",
					s.BySeverity[string(SeverityCritical)], tt.expectedSeverityCrit)
			}

			if tt.expectedSeverityHigh > 0 && s.BySeverity[string(SeverityHigh)] != tt.expectedSeverityHigh {
				t.Errorf("BySeverity[high] = %d, expected %d",
					s.BySeverity[string(SeverityHigh)], tt.expectedSeverityHigh)
			}

			if tt.expectedCategoryDefect > 0 && s.ByCategory[string(DebtDefect)] != tt.expectedCategoryDefect {
				t.Errorf("ByCategory[defect] = %d, expected %d",
					s.ByCategory[string(DebtDefect)], tt.expectedCategoryDefect)
			}

			if len(s.ByFile) != tt.expectedFileCount {
				t.Errorf("ByFile count = %d, expected %d", len(s.ByFile), tt.expectedFileCount)
			}
		})
	}
}

func TestSeverity_Weight(t *testing.T) {
	tests := []struct {
		name     string
		severity Severity
		expected int
	}{
		{
			name:     "critical",
			severity: SeverityCritical,
			expected: 4,
		},
		{
			name:     "high",
			severity: SeverityHigh,
			expected: 3,
		},
		{
			name:     "medium",
			severity: SeverityMedium,
			expected: 2,
		},
		{
			name:     "low",
			severity: SeverityLow,
			expected: 1,
		},
		{
			name:     "unknown severity",
			severity: Severity("unknown"),
			expected: 0,
		},
		{
			name:     "empty severity",
			severity: Severity(""),
			expected: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.severity.Weight()
			if got != tt.expected {
				t.Errorf("Weight() = %d, expected %d", got, tt.expected)
			}
		})
	}
}

func TestSeverity_WeightOrdering(t *testing.T) {
	if SeverityCritical.Weight() <= SeverityHigh.Weight() {
		t.Error("Critical should have higher weight than High")
	}
	if SeverityHigh.Weight() <= SeverityMedium.Weight() {
		t.Error("High should have higher weight than Medium")
	}
	if SeverityMedium.Weight() <= SeverityLow.Weight() {
		t.Error("Medium should have higher weight than Low")
	}
}

func TestDebtCategory_Constants(t *testing.T) {
	categories := []DebtCategory{
		DebtDesign,
		DebtDefect,
		DebtRequirement,
		DebtTest,
		DebtPerformance,
		DebtSecurity,
	}

	if len(categories) != 6 {
		t.Errorf("Expected 6 debt categories, got %d", len(categories))
	}

	for _, cat := range categories {
		if string(cat) == "" {
			t.Error("Debt category should not be empty")
		}
	}
}

func TestSeverity_Constants(t *testing.T) {
	severities := []Severity{
		SeverityLow,
		SeverityMedium,
		SeverityHigh,
		SeverityCritical,
	}

	if len(severities) != 4 {
		t.Errorf("Expected 4 severity levels, got %d", len(severities))
	}

	for _, sev := range severities {
		if string(sev) == "" {
			t.Error("Severity should not be empty")
		}
	}
}

func TestSATDSummary_FileTracking(t *testing.T) {
	s := NewSATDSummary()

	s.AddItem(TechnicalDebt{File: "file1.go", Severity: SeverityHigh, Category: DebtDefect})
	s.AddItem(TechnicalDebt{File: "file1.go", Severity: SeverityLow, Category: DebtDesign})
	s.AddItem(TechnicalDebt{File: "file2.go", Severity: SeverityMedium, Category: DebtTest})

	if s.ByFile["file1.go"] != 2 {
		t.Errorf("file1.go count = %d, expected 2", s.ByFile["file1.go"])
	}
	if s.ByFile["file2.go"] != 1 {
		t.Errorf("file2.go count = %d, expected 1", s.ByFile["file2.go"])
	}
}

func TestSeverity_Escalate(t *testing.T) {
	tests := []struct {
		input    Severity
		expected Severity
	}{
		{SeverityLow, SeverityMedium},
		{SeverityMedium, SeverityHigh},
		{SeverityHigh, SeverityCritical},
		{SeverityCritical, SeverityCritical},
	}

	for _, tt := range tests {
		t.Run(string(tt.input), func(t *testing.T) {
			result := tt.input.Escalate()
			if result != tt.expected {
				t.Errorf("Escalate(%s) = %s, expected %s", tt.input, result, tt.expected)
			}
		})
	}
}

func TestSeverity_Reduce(t *testing.T) {
	tests := []struct {
		input    Severity
		expected Severity
	}{
		{SeverityCritical, SeverityHigh},
		{SeverityHigh, SeverityMedium},
		{SeverityMedium, SeverityLow},
		{SeverityLow, SeverityLow},
	}

	for _, tt := range tests {
		t.Run(string(tt.input), func(t *testing.T) {
			result := tt.input.Reduce()
			if result != tt.expected {
				t.Errorf("Reduce(%s) = %s, expected %s", tt.input, result, tt.expected)
			}
		})
	}
}

func TestSeverity_EscalateReduceRoundTrip(t *testing.T) {
	for _, sev := range []Severity{SeverityMedium, SeverityHigh} {
		escalated := sev.Escalate()
		reduced := escalated.Reduce()
		if reduced != sev {
			t.Errorf("Escalate then Reduce should return original for %s, got %s", sev, reduced)
		}
	}
}
