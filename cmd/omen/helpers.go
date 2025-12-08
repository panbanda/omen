package main

import (
	"fmt"
	"strings"
)

// getPaths returns paths from args, defaulting to ["."]
func getPaths(args []string) []string {
	if len(args) == 0 {
		return []string{"."}
	}
	return args
}

// validateDays validates the --days flag and returns an error if invalid.
func validateDays(days int) error {
	if days <= 0 {
		return fmt.Errorf("--days must be a positive integer (got %d)", days)
	}
	return nil
}

// truncate shortens a string to maxLen, adding "..." if truncated.
func truncate(s string, maxLen int) string {
	if len(s) <= maxLen {
		return s
	}
	if maxLen < 4 {
		return s[:maxLen]
	}
	return s[:maxLen-3] + "..."
}

// sanitizeID replaces non-alphanumeric characters for Mermaid diagram IDs.
func sanitizeID(id string) string {
	var result strings.Builder
	for _, c := range id {
		if (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' {
			result.WriteRune(c)
		} else {
			result.WriteRune('_')
		}
	}
	return result.String()
}
