package watch

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/fatih/color"
	"github.com/fsnotify/fsnotify"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/parser"
)

// Watcher monitors files for changes and triggers analysis.
type Watcher struct {
	fsWatcher *fsnotify.Watcher
	config    *config.Config
	debounce  time.Duration
	analyzers []string
	path      string
	callback  func(path string)
	mu        sync.Mutex
	pending   map[string]time.Time
}

// NewWatcher creates a new file watcher.
func NewWatcher(path string, cfg *config.Config, debounce time.Duration, analyzers []string) (*Watcher, error) {
	fsWatcher, err := fsnotify.NewWatcher()
	if err != nil {
		return nil, err
	}

	if debounce <= 0 {
		debounce = 500 * time.Millisecond
	}

	return &Watcher{
		fsWatcher: fsWatcher,
		config:    cfg,
		debounce:  debounce,
		analyzers: analyzers,
		path:      path,
		pending:   make(map[string]time.Time),
	}, nil
}

// SetCallback sets the function to call when a file changes.
func (w *Watcher) SetCallback(cb func(path string)) {
	w.callback = cb
}

// Start begins watching for file changes.
func (w *Watcher) Start(ctx context.Context) error {
	// Add directories recursively
	err := filepath.Walk(w.path, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil // Skip errors
		}

		// Skip excluded directories
		if info.IsDir() {
			for _, excluded := range w.config.Exclude.Dirs {
				if info.Name() == excluded {
					return filepath.SkipDir
				}
			}
			return w.fsWatcher.Add(path)
		}

		return nil
	})

	if err != nil {
		return err
	}

	color.Cyan("Watching for changes in %s...", w.path)
	color.Cyan("Press Ctrl+C to stop")
	fmt.Println()

	// Start debounce processor
	go w.processDebounced(ctx)

	// Process events
	for {
		select {
		case <-ctx.Done():
			return ctx.Err()

		case event, ok := <-w.fsWatcher.Events:
			if !ok {
				return nil
			}
			w.handleEvent(event)

		case err, ok := <-w.fsWatcher.Errors:
			if !ok {
				return nil
			}
			color.Red("Watch error: %v", err)
		}
	}
}

// handleEvent processes a filesystem event.
func (w *Watcher) handleEvent(event fsnotify.Event) {
	// Only care about writes and creates
	if event.Op&(fsnotify.Write|fsnotify.Create) == 0 {
		return
	}

	path := event.Name

	// Check if file should be excluded
	if w.config.ShouldExclude(path) {
		return
	}

	// Check if file is a supported language
	lang := parser.DetectLanguage(path)
	if lang == parser.LangUnknown {
		return
	}

	// Add to pending with current time
	w.mu.Lock()
	w.pending[path] = time.Now()
	w.mu.Unlock()
}

// processDebounced processes pending changes after debounce period.
func (w *Watcher) processDebounced(ctx context.Context) {
	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			w.processPending()
		}
	}
}

// processPending processes files that have been stable for the debounce period.
func (w *Watcher) processPending() {
	w.mu.Lock()
	defer w.mu.Unlock()

	now := time.Now()
	var ready []string

	for path, lastMod := range w.pending {
		if now.Sub(lastMod) >= w.debounce {
			ready = append(ready, path)
		}
	}

	for _, path := range ready {
		delete(w.pending, path)
		if w.callback != nil {
			go w.runCallback(path)
		}
	}
}

// runCallback executes the callback for a changed file.
func (w *Watcher) runCallback(path string) {
	relPath, err := filepath.Rel(w.path, path)
	if err != nil {
		relPath = path
	}

	color.Yellow("\nFile changed: %s", relPath)
	fmt.Println(strings.Repeat("-", 40))

	w.callback(path)

	fmt.Println()
}

// Stop stops the watcher.
func (w *Watcher) Stop() error {
	return w.fsWatcher.Close()
}

// WatchedFiles returns the list of watched files.
func (w *Watcher) WatchedFiles() []string {
	return w.fsWatcher.WatchList()
}
