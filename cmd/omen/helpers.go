package main

import (
	"context"
	"fmt"
	"os"
	"strings"

	"github.com/panbanda/omen/internal/remote"
)

// getPaths returns paths from args, defaulting to ["."]
func getPaths(args []string) []string {
	if len(args) == 0 {
		return []string{"."}
	}
	return args
}

// resolvePaths converts args to local paths, cloning remote repos as needed.
// Returns resolved paths, a cleanup function, and any error.
// The cleanup function must be called (via defer) to remove cloned temp directories.
//
//nolint:unused // Will be used in subsequent tasks for remote repo integration
func resolvePaths(ctx context.Context, args []string, ref string, shallow bool) ([]string, func(), error) {
	paths := getPaths(args)
	var cleanups []func()

	cleanup := func() {
		for _, fn := range cleanups {
			fn()
		}
	}

	for i, p := range paths {
		src, err := remote.Parse(p)
		if err != nil {
			cleanup()
			return nil, nil, fmt.Errorf("parse %s: %w", p, err)
		}
		if src == nil {
			continue // local path
		}

		// Override ref if flag provided
		if ref != "" {
			src.Ref = ref
		}

		fmt.Fprintf(os.Stderr, "Cloning %s", src.URL)
		if src.Ref != "" {
			fmt.Fprintf(os.Stderr, " @ %s", src.Ref)
		}
		if shallow {
			fmt.Fprintf(os.Stderr, " (shallow)")
		}
		fmt.Fprintln(os.Stderr, "...")

		if err := src.Clone(ctx, os.Stderr, shallow); err != nil {
			cleanup()
			return nil, nil, fmt.Errorf("clone %s: %w", p, err)
		}

		paths[i] = src.CloneDir
		cleanups = append(cleanups, func() {
			src.Cleanup()
		})
	}

	return paths, cleanup, nil
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
