package analyzer

import (
	"runtime"
	"sync"

	"github.com/jonathanreyes/omen-cli/pkg/parser"
	"github.com/sourcegraph/conc/pool"
)

// ProgressFunc is called after each file is processed.
type ProgressFunc func()

// MapFiles processes files in parallel, calling fn for each file with a dedicated parser.
// Results are collected and returned in arbitrary order.
// Errors from individual files are silently skipped.
// Uses 2x NumCPU workers by default (optimal for mixed I/O and CGO workloads).
func MapFiles[T any](files []string, fn func(*parser.Parser, string) (T, error)) []T {
	return MapFilesWithProgress(files, fn, nil)
}

// MapFilesWithProgress processes files in parallel with optional progress callback.
func MapFilesWithProgress[T any](files []string, fn func(*parser.Parser, string) (T, error), onProgress ProgressFunc) []T {
	return MapFilesN(files, runtime.NumCPU()*2, fn, onProgress)
}

// MapFilesN processes files with a configurable worker count.
// If maxWorkers is <= 0, defaults to 2x NumCPU.
func MapFilesN[T any](files []string, maxWorkers int, fn func(*parser.Parser, string) (T, error), onProgress ProgressFunc) []T {
	if len(files) == 0 {
		return nil
	}

	if maxWorkers <= 0 {
		maxWorkers = runtime.NumCPU() * 2
	}

	results := make([]T, 0, len(files))
	var mu sync.Mutex

	p := pool.New().WithMaxGoroutines(maxWorkers)
	for _, path := range files {
		p.Go(func() {
			psr := parser.New()
			defer psr.Close()

			result, err := fn(psr, path)

			if onProgress != nil {
				onProgress()
			}

			if err != nil {
				return
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()
		})
	}
	p.Wait()

	return results
}

// ForEachFile processes files in parallel, calling fn for each file.
// No parser is provided; use this for non-AST operations (e.g., SATD scanning).
// Uses 2x NumCPU workers by default.
func ForEachFile[T any](files []string, fn func(string) (T, error)) []T {
	return ForEachFileWithProgress(files, fn, nil)
}

// ForEachFileWithProgress processes files in parallel with optional progress callback.
func ForEachFileWithProgress[T any](files []string, fn func(string) (T, error), onProgress ProgressFunc) []T {
	if len(files) == 0 {
		return nil
	}

	results := make([]T, 0, len(files))
	var mu sync.Mutex

	p := pool.New().WithMaxGoroutines(runtime.NumCPU() * 2)
	for _, path := range files {
		p.Go(func() {
			result, err := fn(path)

			if onProgress != nil {
				onProgress()
			}

			if err != nil {
				return
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()
		})
	}
	p.Wait()

	return results
}
