package fileproc

import (
	"context"
	"runtime"
	"sync"

	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/parser"
	"github.com/panbanda/omen/pkg/source"
	"github.com/sourcegraph/conc/pool"
)

// ContentSource is an alias for source.ContentSource for backward compatibility.
// New code should import from pkg/source directly.
type ContentSource = source.ContentSource

// fileWithContent holds a file path and its content.
type fileWithContent struct {
	path    string
	content []byte
}

// MapSourceFiles processes files from a ContentSource in parallel.
// Unlike MapFiles, this reads content from the source before processing.
// Progress is tracked via context using analyzer.WithTracker.
func MapSourceFiles[T any](
	ctx context.Context,
	files []string,
	src ContentSource,
	fn func(*parser.Parser, string, []byte) (T, error),
) []T {
	return MapSourceFilesWithSizeLimit(ctx, files, src, 0, fn)
}

// MapSourceFilesWithSizeLimit processes files from a ContentSource in parallel,
// skipping files that exceed maxSize bytes. If maxSize is 0, no limit is enforced.
// Progress is tracked via context using analyzer.WithTracker.
func MapSourceFilesWithSizeLimit[T any](
	ctx context.Context,
	files []string,
	src ContentSource,
	maxSize int64,
	fn func(*parser.Parser, string, []byte) (T, error),
) []T {
	if len(files) == 0 {
		return nil
	}

	// Read all file content sequentially to avoid concurrent access to git trees
	filesWithContent := make([]fileWithContent, 0, len(files))
	for _, path := range files {
		content, err := src.Read(path)
		if err != nil {
			// Skip files that can't be read
			continue
		}
		// Skip files that exceed size limit
		if maxSize > 0 && int64(len(content)) > maxSize {
			continue
		}
		filesWithContent = append(filesWithContent, fileWithContent{
			path:    path,
			content: content,
		})
	}

	// Get tracker from context
	tracker := analyzer.TrackerFromContext(ctx)
	if tracker != nil {
		tracker.Add(len(filesWithContent))
	}

	// Now process in parallel with content already loaded
	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, 0, len(filesWithContent))
	var mu sync.Mutex

	p := pool.New().WithMaxGoroutines(maxWorkers).WithContext(ctx)
	for _, fc := range filesWithContent {
		p.Go(func(ctx context.Context) error {
			defer func() {
				if tracker != nil {
					tracker.Tick(fc.path)
				}
			}()

			// Check for cancellation
			select {
			case <-ctx.Done():
				return ctx.Err()
			default:
			}

			psr := parser.New()
			defer psr.Close()

			result, err := fn(psr, fc.path, fc.content)
			if err != nil {
				return nil
			}

			mu.Lock()
			results = append(results, result)
			mu.Unlock()

			return nil
		})
	}
	_ = p.Wait()

	return results
}

// MapSourceFilesPooled processes files from a ContentSource using a parser pool.
// This is more efficient than MapSourceFiles for large file sets since it reuses
// parsers instead of creating a new one per file.
// Progress is tracked via context using analyzer.WithTracker.
func MapSourceFilesPooled[T any](
	ctx context.Context,
	files []string,
	src ContentSource,
	maxSize int64,
	fn func(*parser.Parser, string, []byte) (T, error),
) []T {
	if len(files) == 0 {
		return nil
	}

	// Read all file content sequentially to avoid concurrent access to git trees
	filesWithContent := make([]fileWithContent, 0, len(files))
	for _, path := range files {
		content, err := src.Read(path)
		if err != nil {
			continue
		}
		if maxSize > 0 && int64(len(content)) > maxSize {
			continue
		}
		filesWithContent = append(filesWithContent, fileWithContent{
			path:    path,
			content: content,
		})
	}

	if len(filesWithContent) == 0 {
		return nil
	}

	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, len(filesWithContent))

	// Use parser pool from parallel.go
	parserPl := newParserPool(maxWorkers)
	defer parserPl.close()

	tracker := analyzer.TrackerFromContext(ctx)
	if tracker != nil {
		tracker.Add(len(filesWithContent))
	}

	p := pool.New().WithMaxGoroutines(maxWorkers).WithContext(ctx)
	for i, fc := range filesWithContent {
		idx := i
		fileContent := fc
		p.Go(func(ctx context.Context) error {
			defer func() {
				if tracker != nil {
					tracker.Tick(fileContent.path)
				}
			}()

			select {
			case <-ctx.Done():
				return ctx.Err()
			default:
			}

			psr := parserPl.get()
			defer parserPl.put(psr)

			result, err := fn(psr, fileContent.path, fileContent.content)
			if err != nil {
				return nil
			}

			results[idx] = result
			return nil
		})
	}
	_ = p.Wait()

	return results
}
