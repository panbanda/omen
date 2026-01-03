package output

import (
	"fmt"
	"unicode/utf8"
)

// TokenBudgetInfo contains token estimation and budget usage information.
type TokenBudgetInfo struct {
	Tokens       int     // Estimated token count
	Budget       int     // Token budget (context window size)
	BudgetLabel  string  // Human-readable budget label (e.g., "8k", "128k")
	UsagePercent float64 // Percentage of budget used
	Remaining    int     // Estimated tokens remaining
}

// Common context window sizes for LLMs
const (
	Budget8K   = 8000
	Budget16K  = 16000
	Budget32K  = 32000
	Budget64K  = 64000
	Budget128K = 128000
	Budget200K = 200000
)

// DefaultBudget is the default context window size for estimation.
const DefaultBudget = Budget128K

// CharsPerToken is the approximate character-to-token ratio.
// Research suggests:
// - Anthropic recommends ~3.5 chars/token for English text
// - Code typically has ~4 chars/token due to syntax, identifiers
// We use 4.0 as a conservative estimate for code-heavy context.
const CharsPerToken = 4.0

// EstimateTokens returns an approximate token count for the given text.
// This uses a simple character-based heuristic suitable for code context.
// For exact counts, use the Anthropic API's countTokens endpoint.
func EstimateTokens(text string) int {
	if len(text) == 0 {
		return 0
	}

	// Count actual runes (Unicode code points) for more accurate estimation
	runeCount := utf8.RuneCountInString(text)

	// Apply chars-per-token ratio
	tokens := float64(runeCount) / CharsPerToken

	return int(tokens + 0.5) // Round to nearest integer
}

// FormatTokenCount formats a token count for display.
// Counts >= 1000 are formatted as "X.Xk".
func FormatTokenCount(tokens int) string {
	if tokens < 1000 {
		return fmt.Sprintf("%d", tokens)
	}
	return fmt.Sprintf("%.1fk", float64(tokens)/1000)
}

// GetTokenBudgetInfo calculates token budget information for the given text.
func GetTokenBudgetInfo(text string, budget int) TokenBudgetInfo {
	if budget <= 0 {
		budget = DefaultBudget
	}

	tokens := EstimateTokens(text)
	remaining := budget - tokens
	if remaining < 0 {
		remaining = 0
	}

	return TokenBudgetInfo{
		Tokens:       tokens,
		Budget:       budget,
		BudgetLabel:  formatBudgetLabel(budget),
		UsagePercent: float64(tokens) / float64(budget) * 100,
		Remaining:    remaining,
	}
}

// formatBudgetLabel creates a human-readable label for a budget size.
func formatBudgetLabel(budget int) string {
	if budget >= 1000 {
		return fmt.Sprintf("%dk", budget/1000)
	}
	return fmt.Sprintf("%d", budget)
}

// BudgetTiers returns common budget tiers for display/selection.
func BudgetTiers() []int {
	return []int{Budget8K, Budget16K, Budget32K, Budget64K, Budget128K, Budget200K}
}
