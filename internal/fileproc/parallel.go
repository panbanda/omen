// Package fileproc provides concurrent file processing utilities.
package fileproc

import (
	"runtime"
	"sync"

	"github.com/panbanda/omen/pkg/parser"
	"github.com/sourcegraph/conc/pool"
)

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
