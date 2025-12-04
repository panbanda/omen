package fileproc

import (
	"runtime"
	"sync"

	"github.com/panbanda/omen/pkg/parser"
	"github.com/sourcegraph/conc/pool"
)

// ContentSource provides file content.
type ContentSource interface {
	Read(path string) ([]byte, error)
}

// MapSourceFiles processes files from a ContentSource in parallel.
// Unlike MapFiles, this reads content from the source before processing.
func MapSourceFiles[T any](
	files []string,
	src ContentSource,
	fn func(*parser.Parser, string, []byte) (T, error),
) []T {
	return MapSourceFilesWithProgress(files, src, fn, nil)
}

// fileWithContent holds a file path and its content.
type fileWithContent struct {
	path    string
	content []byte
}

// MapSourceFilesWithProgress processes files from a ContentSource with progress callback.
// Reads all file content first to avoid concurrent access issues with git trees.
func MapSourceFilesWithProgress[T any](
	files []string,
	src ContentSource,
	fn func(*parser.Parser, string, []byte) (T, error),
	onProgress ProgressFunc,
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
		filesWithContent = append(filesWithContent, fileWithContent{
			path:    path,
			content: content,
		})
	}

	// Now process in parallel with content already loaded
	maxWorkers := runtime.NumCPU() * DefaultWorkerMultiplier
	results := make([]T, 0, len(filesWithContent))
	var mu sync.Mutex

	p := pool.New().WithMaxGoroutines(maxWorkers)
	for _, fc := range filesWithContent {
		fc := fc // capture loop variable
		p.Go(func() {
			psr := parser.New()
			defer psr.Close()

			result, err := fn(psr, fc.path, fc.content)
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

	return results
}
