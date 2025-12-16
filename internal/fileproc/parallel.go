// Package fileproc provides concurrent file processing utilities.
package fileproc

import (
	"context"
	"fmt"
	"os"
	"runtime"
	"sync"
	"sync/atomic"

	"github.com/panbanda/omen/pkg/analyzer"
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

// ErrorFunc is called when a file processing error occurs.
// Receives the file path and the error. If nil, errors are silently skipped.
type ErrorFunc func(path string, err error)

// MapFiles processes files in parallel with context cancellation and progress tracking.
// Progress is tracked via context using analyzer.WithTracker.
// Results are collected and returned in arbitrary order.
// Uses 2x NumCPU workers by default.
func MapFiles[T any](ctx context.Context, files []string, fn func(*parser.Parser, string) (T, error)) ([]T, *ProcessingErrors) {
	return MapFilesWithSizeLimit(ctx, files, 0, fn)
}

// MapFilesWithSizeLimit processes files in parallel, skipping files that exceed maxSize.
// If maxSize is 0, no limit is enforced.
// Progress is tracked via context using analyzer.WithTracker.
func MapFilesWithSizeLimit[T any](ctx context.Context, files []string, maxSize int64, fn func(*parser.Parser, string) (T, error)) ([]T, *ProcessingErrors) {
	if len(files) == 0 {
		return nil, nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, 0, len(files))
	errs := &ProcessingErrors{}
	var mu sync.Mutex

	total := len(files)
	var processed atomic.Int32
	tracker := analyzer.TrackerFromContext(ctx)

	// If using tracker, set total count
	if tracker != nil {
		tracker.Add(total)
	}

	reportProgress := func(path string) {
		processed.Add(1)
		if tracker != nil {
			tracker.Tick(path)
		}
	}

	p := pool.New().WithMaxGoroutines(maxWorkers).WithContext(ctx)
	for _, path := range files {
		p.Go(func(ctx context.Context) error {
			// Check for cancellation before processing
			select {
			case <-ctx.Done():
				errs.Add(path, ctx.Err())
				reportProgress(path)
				return ctx.Err()
			default:
			}

			// Check file size before processing
			if maxSize > 0 {
				info, err := os.Stat(path)
				if err != nil {
					errs.Add(path, err)
					reportProgress(path)
					return nil
				}
				if info.Size() > maxSize {
					errs.Add(path, fmt.Errorf("file too large: %d bytes (limit: %d)", info.Size(), maxSize))
					reportProgress(path)
					return nil
				}
			}

			psr := parser.New()
			defer psr.Close()

			result, err := fn(psr, path)

			if err != nil {
				errs.Add(path, err)
				reportProgress(path)
				return nil
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()

			reportProgress(path)
			return nil
		})
	}
	_ = p.Wait()

	if !errs.HasErrors() {
		return results, nil
	}
	return results, errs
}

// ForEachFile processes files in parallel with context cancellation and progress tracking.
// No parser is provided; use this for non-AST operations (e.g., SATD scanning).
// Progress is tracked via context using analyzer.WithTracker.
// Uses 2x NumCPU workers by default.
func ForEachFile[T any](ctx context.Context, files []string, fn func(string) (T, error)) ([]T, *ProcessingErrors) {
	if len(files) == 0 {
		return nil, nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, 0, len(files))
	errs := &ProcessingErrors{}
	var mu sync.Mutex

	total := len(files)
	var processed atomic.Int32
	tracker := analyzer.TrackerFromContext(ctx)

	// If using tracker, set total count
	if tracker != nil {
		tracker.Add(total)
	}

	reportProgress := func(path string) {
		processed.Add(1)
		if tracker != nil {
			tracker.Tick(path)
		}
	}

	p := pool.New().WithMaxGoroutines(maxWorkers).WithContext(ctx)
	for _, path := range files {
		p.Go(func(ctx context.Context) error {
			// Check for cancellation before processing
			select {
			case <-ctx.Done():
				errs.Add(path, ctx.Err())
				reportProgress(path)
				return ctx.Err()
			default:
			}

			result, err := fn(path)

			if err != nil {
				errs.Add(path, err)
				reportProgress(path)
				return nil
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()

			reportProgress(path)
			return nil
		})
	}
	_ = p.Wait()

	if !errs.HasErrors() {
		return results, nil
	}
	return results, errs
}

// ForEachFileWithResource processes files in parallel with a per-worker resource.
// The initResource function is called once per worker to create the resource (e.g., git repo handle).
// The closeResource function is called when the worker is done to release the resource.
// Progress is tracked via context using analyzer.WithTracker.
// Uses 2x NumCPU workers by default.
func ForEachFileWithResource[T any, R any](
	ctx context.Context,
	files []string,
	initResource func() (R, error),
	closeResource func(R),
	fn func(R, string) (T, error),
) ([]T, *ProcessingErrors) {
	if len(files) == 0 {
		return nil, nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, 0, len(files))
	errs := &ProcessingErrors{}
	var mu sync.Mutex

	total := len(files)
	var processed atomic.Int32
	tracker := analyzer.TrackerFromContext(ctx)

	if tracker != nil {
		tracker.Add(total)
	}

	reportProgress := func(path string) {
		processed.Add(1)
		if tracker != nil {
			tracker.Tick(path)
		}
	}

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
			resourcePool <- &resourceWrapper{valid: false}
		} else {
			resourcePool <- &resourceWrapper{resource: r, valid: true}
		}
	}

	p := pool.New().WithMaxGoroutines(maxWorkers).WithContext(ctx)
	for _, path := range files {
		p.Go(func(ctx context.Context) error {
			// Check for cancellation
			select {
			case <-ctx.Done():
				errs.Add(path, ctx.Err())
				reportProgress(path)
				return ctx.Err()
			default:
			}

			// Get resource from pool
			wrapper := <-resourcePool
			defer func() { resourcePool <- wrapper }()

			if !wrapper.valid {
				errs.Add(path, fmt.Errorf("resource unavailable"))
				reportProgress(path)
				return nil
			}

			result, err := fn(wrapper.resource, path)

			if err != nil {
				errs.Add(path, err)
				reportProgress(path)
				return nil
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()

			reportProgress(path)
			return nil
		})
	}
	_ = p.Wait()

	// Close all resources
	close(resourcePool)
	for wrapper := range resourcePool {
		if wrapper.valid && closeResource != nil {
			closeResource(wrapper.resource)
		}
	}

	if !errs.HasErrors() {
		return results, nil
	}
	return results, errs
}

// MapFilesIndexed processes files in parallel using indexed result collection
// to eliminate mutex contention. Results are returned in the same order as input files.
// Progress is tracked via context using analyzer.WithTracker.
func MapFilesIndexed[T any](ctx context.Context, files []string, fn func(*parser.Parser, string) (T, error)) ([]T, *ProcessingErrors) {
	return MapFilesIndexedWithSizeLimit(ctx, files, 0, fn)
}

// MapFilesIndexedWithSizeLimit processes files in parallel using indexed result collection,
// skipping files that exceed maxSize. If maxSize is 0, no limit is enforced.
// Results are returned in the same order as input files (zero value for skipped/errored files).
func MapFilesIndexedWithSizeLimit[T any](ctx context.Context, files []string, maxSize int64, fn func(*parser.Parser, string) (T, error)) ([]T, *ProcessingErrors) {
	if len(files) == 0 {
		return nil, nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	// Pre-allocate results slice - no mutex needed with indexed assignment
	results := make([]T, len(files))
	errs := &ProcessingErrors{}

	tracker := analyzer.TrackerFromContext(ctx)
	if tracker != nil {
		tracker.Add(len(files))
	}

	p := pool.New().WithMaxGoroutines(maxWorkers).WithContext(ctx)
	for i, path := range files {
		// Capture loop variables
		idx := i
		filePath := path

		p.Go(func(ctx context.Context) error {
			defer func() {
				if tracker != nil {
					tracker.Tick(filePath)
				}
			}()

			// Check for cancellation before processing
			select {
			case <-ctx.Done():
				errs.Add(filePath, ctx.Err())
				return ctx.Err()
			default:
			}

			// Check file size before processing
			if maxSize > 0 {
				info, err := os.Stat(filePath)
				if err != nil {
					errs.Add(filePath, err)
					return nil
				}
				if info.Size() > maxSize {
					errs.Add(filePath, fmt.Errorf("file too large: %d bytes (limit: %d)", info.Size(), maxSize))
					return nil
				}
			}

			psr := parser.New()
			defer psr.Close()

			result, err := fn(psr, filePath)
			if err != nil {
				errs.Add(filePath, err)
				return nil
			}

			// Indexed assignment - no mutex needed
			results[idx] = result
			return nil
		})
	}
	_ = p.Wait()

	if !errs.HasErrors() {
		return results, nil
	}
	return results, errs
}

// ForEachFileIndexed processes files in parallel using indexed result collection.
// No parser is provided; use this for non-AST operations.
// Results are returned in the same order as input files.
func ForEachFileIndexed[T any](ctx context.Context, files []string, fn func(string) (T, error)) ([]T, *ProcessingErrors) {
	if len(files) == 0 {
		return nil, nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, len(files))
	errs := &ProcessingErrors{}

	tracker := analyzer.TrackerFromContext(ctx)
	if tracker != nil {
		tracker.Add(len(files))
	}

	p := pool.New().WithMaxGoroutines(maxWorkers).WithContext(ctx)
	for i, path := range files {
		idx := i
		filePath := path

		p.Go(func(ctx context.Context) error {
			defer func() {
				if tracker != nil {
					tracker.Tick(filePath)
				}
			}()

			select {
			case <-ctx.Done():
				errs.Add(filePath, ctx.Err())
				return ctx.Err()
			default:
			}

			result, err := fn(filePath)
			if err != nil {
				errs.Add(filePath, err)
				return nil
			}

			results[idx] = result
			return nil
		})
	}
	_ = p.Wait()

	if !errs.HasErrors() {
		return results, nil
	}
	return results, errs
}

// parserPool provides per-worker parser pooling to reduce CGO overhead.
type parserPool struct {
	parsers chan *parser.Parser
	size    int
}

// newParserPool creates a pool of parsers for concurrent use.
func newParserPool(size int) *parserPool {
	pp := &parserPool{
		parsers: make(chan *parser.Parser, size),
		size:    size,
	}
	// Pre-populate the pool
	for i := 0; i < size; i++ {
		pp.parsers <- parser.New()
	}
	return pp
}

// get retrieves a parser from the pool.
func (pp *parserPool) get() *parser.Parser {
	return <-pp.parsers
}

// put returns a parser to the pool.
func (pp *parserPool) put(psr *parser.Parser) {
	pp.parsers <- psr
}

// close releases all parsers in the pool.
func (pp *parserPool) close() {
	close(pp.parsers)
	for psr := range pp.parsers {
		psr.Close()
	}
}

// MapFilesPooled processes files in parallel using a shared parser pool.
// This reduces CGO overhead by reusing parsers across multiple files.
// Results are returned in the same order as input files.
func MapFilesPooled[T any](ctx context.Context, files []string, fn func(*parser.Parser, string) (T, error)) ([]T, *ProcessingErrors) {
	return MapFilesPooledWithSizeLimit(ctx, files, 0, fn)
}

// MapFilesPooledWithSizeLimit processes files using a shared parser pool,
// skipping files that exceed maxSize. If maxSize is 0, no limit is enforced.
func MapFilesPooledWithSizeLimit[T any](ctx context.Context, files []string, maxSize int64, fn func(*parser.Parser, string) (T, error)) ([]T, *ProcessingErrors) {
	if len(files) == 0 {
		return nil, nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, len(files))
	errs := &ProcessingErrors{}

	// Create parser pool with one parser per worker
	parserPl := newParserPool(maxWorkers)
	defer parserPl.close()

	tracker := analyzer.TrackerFromContext(ctx)
	if tracker != nil {
		tracker.Add(len(files))
	}

	p := pool.New().WithMaxGoroutines(maxWorkers).WithContext(ctx)
	for i, path := range files {
		idx := i
		filePath := path

		p.Go(func(ctx context.Context) error {
			defer func() {
				if tracker != nil {
					tracker.Tick(filePath)
				}
			}()

			select {
			case <-ctx.Done():
				errs.Add(filePath, ctx.Err())
				return ctx.Err()
			default:
			}

			if maxSize > 0 {
				info, err := os.Stat(filePath)
				if err != nil {
					errs.Add(filePath, err)
					return nil
				}
				if info.Size() > maxSize {
					errs.Add(filePath, fmt.Errorf("file too large: %d bytes (limit: %d)", info.Size(), maxSize))
					return nil
				}
			}

			// Get parser from pool instead of creating new one
			psr := parserPl.get()
			defer parserPl.put(psr)

			result, err := fn(psr, filePath)
			if err != nil {
				errs.Add(filePath, err)
				return nil
			}

			results[idx] = result
			return nil
		})
	}
	_ = p.Wait()

	if !errs.HasErrors() {
		return results, nil
	}
	return results, errs
}
