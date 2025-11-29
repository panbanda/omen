// Package fileproc provides concurrent file processing utilities.
package fileproc

import (
	"context"
	"fmt"
	"runtime"
	"sync"

	"github.com/panbanda/omen/pkg/parser"
	"github.com/sourcegraph/conc/pool"
)

// ProcessingError represents an error that occurred while processing a file.
type ProcessingError struct {
	Path string
	Err  error
}

func (e ProcessingError) Error() string {
	return fmt.Sprintf("%s: %v", e.Path, e.Err)
}

// ProcessingErrors collects multiple file processing errors.
type ProcessingErrors struct {
	Errors []ProcessingError
	mu     sync.Mutex
}

// Add appends an error to the collection (thread-safe).
func (e *ProcessingErrors) Add(path string, err error) {
	e.mu.Lock()
	e.Errors = append(e.Errors, ProcessingError{Path: path, Err: err})
	e.mu.Unlock()
}

// HasErrors returns true if any errors were collected.
func (e *ProcessingErrors) HasErrors() bool {
	e.mu.Lock()
	defer e.mu.Unlock()
	return len(e.Errors) > 0
}

// Error implements the error interface.
func (e *ProcessingErrors) Error() string {
	e.mu.Lock()
	defer e.mu.Unlock()
	if len(e.Errors) == 0 {
		return "no errors"
	}
	if len(e.Errors) == 1 {
		return e.Errors[0].Error()
	}
	return fmt.Sprintf("%d files failed to process (first: %v)", len(e.Errors), e.Errors[0])
}

// Unwrap returns nil (ProcessingErrors doesn't wrap a single error).
func (e *ProcessingErrors) Unwrap() error {
	return nil
}

// DefaultWorkerMultiplier is the multiplier applied to NumCPU for worker count.
// 2x is optimal for mixed I/O and CGO workloads.
const DefaultWorkerMultiplier = 2

// ProgressFunc is called after each file is processed.
type ProgressFunc func()

// ErrorFunc is called when a file processing error occurs.
// Receives the file path and the error. If nil, errors are silently skipped.
type ErrorFunc func(path string, err error)

// MapFiles processes files in parallel, calling fn for each file with a dedicated parser.
// Results are collected and returned in arbitrary order.
// Errors from individual files are silently skipped; use MapFilesWithErrors for error handling.
// Uses 2x NumCPU workers by default (optimal for mixed I/O and CGO workloads).
func MapFiles[T any](files []string, fn func(*parser.Parser, string) (T, error)) []T {
	return MapFilesWithProgress(files, fn, nil)
}

// MapFilesWithProgress processes files in parallel with optional progress callback.
func MapFilesWithProgress[T any](files []string, fn func(*parser.Parser, string) (T, error), onProgress ProgressFunc) []T {
	return MapFilesN(files, 0, fn, onProgress, nil)
}

// MapFilesWithErrors processes files in parallel with error callback.
// The onError callback is invoked for each file that fails processing.
func MapFilesWithErrors[T any](files []string, fn func(*parser.Parser, string) (T, error), onError ErrorFunc) []T {
	return MapFilesN(files, 0, fn, nil, onError)
}

// MapFilesN processes files with configurable worker count and callbacks.
// If maxWorkers is <= 0, defaults to 2x NumCPU.
func MapFilesN[T any](files []string, maxWorkers int, fn func(*parser.Parser, string) (T, error), onProgress ProgressFunc, onError ErrorFunc) []T {
	if len(files) == 0 {
		return nil
	}

	if maxWorkers <= 0 {
		maxWorkers = runtime.NumCPU() * DefaultWorkerMultiplier
	}

	results := make([]T, 0, len(files))
	var mu sync.Mutex

	p := pool.New().WithMaxGoroutines(maxWorkers)
	for _, path := range files {
		p.Go(func() {
			psr := parser.New()
			defer psr.Close()

			result, err := fn(psr, path)

			if err != nil {
				if onError != nil {
					onError(path, err)
				}
				if onProgress != nil {
					onProgress()
				}
				return
			}

			if onProgress != nil {
				onProgress()
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()
		})
	}
	p.Wait()

	return results
}

// ForEachFileWithResource processes files in parallel, calling fn for each file with a per-worker resource.
// The initResource function is called once per worker to create the resource (e.g., git repo handle).
// The closeResource function is called when the worker is done to release the resource.
// Uses 2x NumCPU workers by default.
func ForEachFileWithResource[T any, R any](
	files []string,
	initResource func() (R, error),
	closeResource func(R),
	fn func(R, string) (T, error),
	onProgress ProgressFunc,
) []T {
	if len(files) == 0 {
		return nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, 0, len(files))
	var mu sync.Mutex

	// Create a pool of resources matching worker count
	type resourceWrapper struct {
		resource R
		valid    bool
	}
	resourcePool := make(chan *resourceWrapper, maxWorkers)

	// Pre-create resources for the pool
	for i := 0; i < maxWorkers; i++ {
		r, err := initResource()
		if err != nil {
			// If we can't create a resource, add an invalid wrapper
			resourcePool <- &resourceWrapper{valid: false}
		} else {
			resourcePool <- &resourceWrapper{resource: r, valid: true}
		}
	}

	p := pool.New().WithMaxGoroutines(maxWorkers)
	for _, path := range files {
		p.Go(func() {
			// Get resource from pool
			wrapper := <-resourcePool
			defer func() { resourcePool <- wrapper }()

			if !wrapper.valid {
				if onProgress != nil {
					onProgress()
				}
				return
			}

			result, err := fn(wrapper.resource, path)

			if err != nil {
				if onProgress != nil {
					onProgress()
				}
				return
			}

			if onProgress != nil {
				onProgress()
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()
		})
	}
	p.Wait()

	// Close all resources
	close(resourcePool)
	for wrapper := range resourcePool {
		if wrapper.valid && closeResource != nil {
			closeResource(wrapper.resource)
		}
	}

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
	return ForEachFileN(files, 0, fn, onProgress, nil)
}

// ForEachFileWithErrors processes files in parallel with error callback.
func ForEachFileWithErrors[T any](files []string, fn func(string) (T, error), onError ErrorFunc) []T {
	return ForEachFileN(files, 0, fn, nil, onError)
}

// ForEachFileN processes files with configurable worker count and callbacks.
// If maxWorkers is <= 0, defaults to 2x NumCPU.
func ForEachFileN[T any](files []string, maxWorkers int, fn func(string) (T, error), onProgress ProgressFunc, onError ErrorFunc) []T {
	if len(files) == 0 {
		return nil
	}

	if maxWorkers <= 0 {
		maxWorkers = runtime.NumCPU() * DefaultWorkerMultiplier
	}

	results := make([]T, 0, len(files))
	var mu sync.Mutex

	p := pool.New().WithMaxGoroutines(maxWorkers)
	for _, path := range files {
		p.Go(func() {
			result, err := fn(path)

			if err != nil {
				if onError != nil {
					onError(path, err)
				}
				if onProgress != nil {
					onProgress()
				}
				return
			}

			if onProgress != nil {
				onProgress()
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()
		})
	}
	p.Wait()

	return results
}

// MapFilesCollectErrors processes files in parallel and collects all errors.
// Returns results and any errors that occurred during processing.
func MapFilesCollectErrors[T any](files []string, fn func(*parser.Parser, string) (T, error)) ([]T, *ProcessingErrors) {
	return MapFilesCollectErrorsWithProgress(files, fn, nil)
}

// MapFilesCollectErrorsWithProgress processes files in parallel with progress callback and collects errors.
func MapFilesCollectErrorsWithProgress[T any](files []string, fn func(*parser.Parser, string) (T, error), onProgress ProgressFunc) ([]T, *ProcessingErrors) {
	if len(files) == 0 {
		return nil, nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, 0, len(files))
	errs := &ProcessingErrors{}
	var mu sync.Mutex

	p := pool.New().WithMaxGoroutines(maxWorkers)
	for _, path := range files {
		p.Go(func() {
			psr := parser.New()
			defer psr.Close()

			result, err := fn(psr, path)

			if err != nil {
				errs.Add(path, err)
				if onProgress != nil {
					onProgress()
				}
				return
			}

			if onProgress != nil {
				onProgress()
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()
		})
	}
	p.Wait()

	if !errs.HasErrors() {
		return results, nil
	}
	return results, errs
}

// ForEachFileCollectErrors processes files in parallel and collects all errors.
// Returns results and any errors that occurred during processing.
func ForEachFileCollectErrors[T any](files []string, fn func(string) (T, error)) ([]T, *ProcessingErrors) {
	return ForEachFileCollectErrorsWithProgress(files, fn, nil)
}

// ForEachFileCollectErrorsWithProgress processes files in parallel with progress callback and collects errors.
func ForEachFileCollectErrorsWithProgress[T any](files []string, fn func(string) (T, error), onProgress ProgressFunc) ([]T, *ProcessingErrors) {
	if len(files) == 0 {
		return nil, nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, 0, len(files))
	errs := &ProcessingErrors{}
	var mu sync.Mutex

	p := pool.New().WithMaxGoroutines(maxWorkers)
	for _, path := range files {
		p.Go(func() {
			result, err := fn(path)

			if err != nil {
				errs.Add(path, err)
				if onProgress != nil {
					onProgress()
				}
				return
			}

			if onProgress != nil {
				onProgress()
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()
		})
	}
	p.Wait()

	if !errs.HasErrors() {
		return results, nil
	}
	return results, errs
}

// MapFilesWithContext processes files in parallel with context cancellation support.
// Returns results collected before cancellation and any errors including context errors.
func MapFilesWithContext[T any](ctx context.Context, files []string, fn func(*parser.Parser, string) (T, error)) ([]T, *ProcessingErrors) {
	return MapFilesWithContextAndProgress(ctx, files, fn, nil)
}

// MapFilesWithContextAndProgress processes files with context and progress callback.
func MapFilesWithContextAndProgress[T any](ctx context.Context, files []string, fn func(*parser.Parser, string) (T, error), onProgress ProgressFunc) ([]T, *ProcessingErrors) {
	if len(files) == 0 {
		return nil, nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, 0, len(files))
	errs := &ProcessingErrors{}
	var mu sync.Mutex

	p := pool.New().WithMaxGoroutines(maxWorkers).WithContext(ctx)
	for _, path := range files {
		p.Go(func(ctx context.Context) error {
			// Check for cancellation before processing
			select {
			case <-ctx.Done():
				errs.Add(path, ctx.Err())
				if onProgress != nil {
					onProgress()
				}
				return ctx.Err()
			default:
			}

			psr := parser.New()
			defer psr.Close()

			result, err := fn(psr, path)

			if err != nil {
				errs.Add(path, err)
				if onProgress != nil {
					onProgress()
				}
				return nil // Don't stop pool on individual file errors
			}

			if onProgress != nil {
				onProgress()
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()
			return nil
		})
	}
	_ = p.Wait() // Context errors are already captured in errs

	if !errs.HasErrors() {
		return results, nil
	}
	return results, errs
}

// ForEachFileWithContext processes files in parallel with context cancellation support.
func ForEachFileWithContext[T any](ctx context.Context, files []string, fn func(string) (T, error)) ([]T, *ProcessingErrors) {
	return ForEachFileWithContextAndProgress(ctx, files, fn, nil)
}

// ForEachFileWithContextAndProgress processes files with context and progress callback.
func ForEachFileWithContextAndProgress[T any](ctx context.Context, files []string, fn func(string) (T, error), onProgress ProgressFunc) ([]T, *ProcessingErrors) {
	if len(files) == 0 {
		return nil, nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, 0, len(files))
	errs := &ProcessingErrors{}
	var mu sync.Mutex

	p := pool.New().WithMaxGoroutines(maxWorkers).WithContext(ctx)
	for _, path := range files {
		p.Go(func(ctx context.Context) error {
			// Check for cancellation before processing
			select {
			case <-ctx.Done():
				errs.Add(path, ctx.Err())
				if onProgress != nil {
					onProgress()
				}
				return ctx.Err()
			default:
			}

			result, err := fn(path)

			if err != nil {
				errs.Add(path, err)
				if onProgress != nil {
					onProgress()
				}
				return nil
			}

			if onProgress != nil {
				onProgress()
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()
			return nil
		})
	}
	_ = p.Wait()

	if !errs.HasErrors() {
		return results, nil
	}
	return results, errs
}
