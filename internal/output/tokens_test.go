package output

import (
	"testing"
)

func TestEstimateTokens(t *testing.T) {
	tests := []struct {
		name     string
		text     string
		minRange int // Minimum expected tokens
		maxRange int // Maximum expected tokens
	}{
		{
			name:     "empty string",
			text:     "",
			minRange: 0,
			maxRange: 0,
		},
		{
			name:     "simple sentence",
			text:     "Hello, world!",
			minRange: 2,
			maxRange: 5,
		},
		{
			name:     "code snippet",
			text:     "func main() { fmt.Println(\"hello\") }",
			minRange: 8,
			maxRange: 15,
		},
		{
			name: "multiline code",
			text: `func Add(a int, b int) int {
	return a + b
}`,
			minRange: 10,
			maxRange: 20,
		},
		{
			name:     "1000 characters",
			text:     string(make([]byte, 1000)),
			minRange: 200,
			maxRange: 350,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := EstimateTokens(tt.text)
			if got < tt.minRange || got > tt.maxRange {
				t.Errorf("EstimateTokens() = %v, want between %v and %v", got, tt.minRange, tt.maxRange)
			}
		})
	}
}

func TestFormatTokenCount(t *testing.T) {
	tests := []struct {
		tokens   int
		expected string
	}{
		{100, "100"},
		{1000, "1.0k"},
		{1500, "1.5k"},
		{10000, "10.0k"},
		{100000, "100.0k"},
		{1000000, "1000.0k"},
	}

	for _, tt := range tests {
		t.Run(tt.expected, func(t *testing.T) {
			got := FormatTokenCount(tt.tokens)
			if got != tt.expected {
				t.Errorf("FormatTokenCount(%d) = %v, want %v", tt.tokens, got, tt.expected)
			}
		})
	}
}

func TestTokenBudgetInfo(t *testing.T) {
	// Test 8k budget (around 32,000 chars at 4 chars/token)
	text8k := string(make([]byte, 8000)) // ~2k tokens
	info := GetTokenBudgetInfo(text8k, 8000)

	if info.Tokens < 1500 || info.Tokens > 2500 {
		t.Errorf("Expected ~2000 tokens, got %d", info.Tokens)
	}

	if info.Budget != 8000 {
		t.Errorf("Expected budget 8000, got %d", info.Budget)
	}

	// Usage should be around 25% (2k/8k)
	if info.UsagePercent < 20 || info.UsagePercent > 35 {
		t.Errorf("Expected ~25%% usage, got %.1f%%", info.UsagePercent)
	}

	if info.BudgetLabel != "8k" {
		t.Errorf("Expected budget label '8k', got '%s'", info.BudgetLabel)
	}
}
