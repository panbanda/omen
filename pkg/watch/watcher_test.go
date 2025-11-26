package watch

import (
	"context"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/fsnotify/fsnotify"
	"github.com/panbanda/omen/pkg/config"
)

func TestNewWatcher(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	tests := []struct {
		name      string
		path      string
		debounce  time.Duration
		analyzers []string
	}{
		{
			name:      "default debounce",
			path:      tmpDir,
			debounce:  0,
			analyzers: []string{"complexity"},
		},
		{
			name:      "custom debounce",
			path:      tmpDir,
			debounce:  time.Second,
			analyzers: []string{"satd", "deadcode"},
		},
		{
			name:      "negative debounce defaults",
			path:      tmpDir,
			debounce:  -time.Second,
			analyzers: nil,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			w, err := NewWatcher(tt.path, cfg, tt.debounce, tt.analyzers)
			if err != nil {
				t.Fatalf("NewWatcher() error = %v", err)
			}
			defer w.Stop()

			if w.fsWatcher == nil {
				t.Error("fsWatcher should not be nil")
			}
			if w.config != cfg {
				t.Error("config should match")
			}
			if w.path != tt.path {
				t.Errorf("path = %v, want %v", w.path, tt.path)
			}
			if w.pending == nil {
				t.Error("pending map should be initialized")
			}
			if tt.debounce <= 0 && w.debounce != 500*time.Millisecond {
				t.Errorf("debounce should default to 500ms, got %v", w.debounce)
			}
			if tt.debounce > 0 && w.debounce != tt.debounce {
				t.Errorf("debounce = %v, want %v", w.debounce, tt.debounce)
			}
		})
	}
}

func TestWatcher_SetCallback(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, time.Second, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	if w.callback != nil {
		t.Error("callback should be nil initially")
	}

	w.SetCallback(func(path string) {
		// callback set
	})

	if w.callback == nil {
		t.Error("callback should be set")
	}
}

func TestWatcher_Stop(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, time.Second, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}

	err = w.Stop()
	if err != nil {
		t.Errorf("Stop() error = %v", err)
	}
}

func TestWatcher_WatchedFiles(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, time.Second, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	// Initially no watched directories
	files := w.WatchedFiles()
	if files == nil {
		files = []string{}
	}

	// Add a directory to watch
	if err := w.fsWatcher.Add(tmpDir); err != nil {
		t.Fatalf("Add() error = %v", err)
	}

	files = w.WatchedFiles()
	if len(files) == 0 {
		t.Error("WatchedFiles() should return at least one directory after Add()")
	}

	found := false
	for _, f := range files {
		if f == tmpDir {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("WatchedFiles() should contain %v", tmpDir)
	}
}

func TestWatcher_handleEvent(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, time.Second, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	tests := []struct {
		name        string
		event       fsnotify.Event
		wantPending bool
	}{
		{
			name: "write event for go file",
			event: fsnotify.Event{
				Name: filepath.Join(tmpDir, "test.go"),
				Op:   fsnotify.Write,
			},
			wantPending: true,
		},
		{
			name: "create event for go file",
			event: fsnotify.Event{
				Name: filepath.Join(tmpDir, "new.go"),
				Op:   fsnotify.Create,
			},
			wantPending: true,
		},
		{
			name: "remove event ignored",
			event: fsnotify.Event{
				Name: filepath.Join(tmpDir, "removed.go"),
				Op:   fsnotify.Remove,
			},
			wantPending: false,
		},
		{
			name: "chmod event ignored",
			event: fsnotify.Event{
				Name: filepath.Join(tmpDir, "changed.go"),
				Op:   fsnotify.Chmod,
			},
			wantPending: false,
		},
		{
			name: "unsupported file type ignored",
			event: fsnotify.Event{
				Name: filepath.Join(tmpDir, "readme.txt"),
				Op:   fsnotify.Write,
			},
			wantPending: false,
		},
		{
			name: "python file supported",
			event: fsnotify.Event{
				Name: filepath.Join(tmpDir, "script.py"),
				Op:   fsnotify.Write,
			},
			wantPending: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Clear pending
			w.mu.Lock()
			w.pending = make(map[string]time.Time)
			w.mu.Unlock()

			w.handleEvent(tt.event)

			w.mu.Lock()
			_, found := w.pending[tt.event.Name]
			w.mu.Unlock()

			if found != tt.wantPending {
				t.Errorf("pending[%v] = %v, want %v", tt.event.Name, found, tt.wantPending)
			}
		})
	}
}

func TestWatcher_handleEvent_Excluded(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, time.Second, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	tests := []struct {
		name        string
		path        string
		wantPending bool
	}{
		{
			name:        "test file excluded",
			path:        filepath.Join(tmpDir, "main_test.go"),
			wantPending: false,
		},
		{
			name:        "vendor file excluded",
			path:        filepath.Join(tmpDir, "vendor", "lib.go"),
			wantPending: false,
		},
		{
			name:        "normal file not excluded",
			path:        filepath.Join(tmpDir, "main.go"),
			wantPending: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			w.mu.Lock()
			w.pending = make(map[string]time.Time)
			w.mu.Unlock()

			event := fsnotify.Event{
				Name: tt.path,
				Op:   fsnotify.Write,
			}

			w.handleEvent(event)

			w.mu.Lock()
			_, found := w.pending[tt.path]
			w.mu.Unlock()

			if found != tt.wantPending {
				t.Errorf("pending[%v] = %v, want %v", tt.path, found, tt.wantPending)
			}
		})
	}
}

func TestWatcher_processPending(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, 50*time.Millisecond, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	var callbackPath string
	var callbackMu sync.Mutex

	w.SetCallback(func(path string) {
		callbackMu.Lock()
		callbackPath = path
		callbackMu.Unlock()
	})

	testFile := filepath.Join(tmpDir, "test.go")

	// Add a pending file with old timestamp
	w.mu.Lock()
	w.pending[testFile] = time.Now().Add(-100 * time.Millisecond)
	w.mu.Unlock()

	// Process pending
	w.processPending()

	// Wait for callback
	time.Sleep(50 * time.Millisecond)

	callbackMu.Lock()
	gotPath := callbackPath
	callbackMu.Unlock()

	if gotPath != testFile {
		t.Errorf("callback path = %v, want %v", gotPath, testFile)
	}

	// Verify pending is cleared
	w.mu.Lock()
	_, stillPending := w.pending[testFile]
	w.mu.Unlock()

	if stillPending {
		t.Error("file should be removed from pending after processing")
	}
}

func TestWatcher_processPending_NotReady(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, time.Hour, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	callbackCalled := false
	w.SetCallback(func(path string) {
		callbackCalled = true
	})

	testFile := filepath.Join(tmpDir, "test.go")

	// Add with current timestamp (not ready yet due to long debounce)
	w.mu.Lock()
	w.pending[testFile] = time.Now()
	w.mu.Unlock()

	w.processPending()

	time.Sleep(10 * time.Millisecond)

	if callbackCalled {
		t.Error("callback should not be called for file not past debounce period")
	}

	w.mu.Lock()
	_, stillPending := w.pending[testFile]
	w.mu.Unlock()

	if !stillPending {
		t.Error("file should still be in pending")
	}
}

func TestWatcher_processPending_NoCallback(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, 50*time.Millisecond, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	testFile := filepath.Join(tmpDir, "test.go")

	w.mu.Lock()
	w.pending[testFile] = time.Now().Add(-100 * time.Millisecond)
	w.mu.Unlock()

	// Should not panic without callback
	w.processPending()

	w.mu.Lock()
	_, stillPending := w.pending[testFile]
	w.mu.Unlock()

	if stillPending {
		t.Error("file should be removed from pending even without callback")
	}
}

func TestWatcher_Start_Context(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, 50*time.Millisecond, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	ctx, cancel := context.WithCancel(context.Background())

	errCh := make(chan error, 1)
	go func() {
		errCh <- w.Start(ctx)
	}()

	// Give it time to start
	time.Sleep(100 * time.Millisecond)

	// Cancel context
	cancel()

	select {
	case err := <-errCh:
		if err != context.Canceled {
			t.Errorf("Start() error = %v, want context.Canceled", err)
		}
	case <-time.After(time.Second):
		t.Error("Start() did not return after context cancellation")
	}
}

func TestWatcher_Start_FileChange(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, 50*time.Millisecond, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	var callbackCount int32
	var lastPath string
	var mu sync.Mutex

	w.SetCallback(func(path string) {
		atomic.AddInt32(&callbackCount, 1)
		mu.Lock()
		lastPath = path
		mu.Unlock()
	})

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	go w.Start(ctx)

	// Wait for watcher to start
	time.Sleep(100 * time.Millisecond)

	// Create a Go file
	testFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(testFile, []byte("package main\n"), 0644); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}

	// Wait for debounce + processing
	time.Sleep(200 * time.Millisecond)

	count := atomic.LoadInt32(&callbackCount)
	if count == 0 {
		t.Error("callback should be called when file is created")
	}

	mu.Lock()
	gotPath := lastPath
	mu.Unlock()

	if gotPath != testFile {
		t.Errorf("callback path = %v, want %v", gotPath, testFile)
	}
}

func TestWatcher_Start_ExcludedDirectory(t *testing.T) {
	tmpDir := t.TempDir()

	// Create excluded directories
	vendorDir := filepath.Join(tmpDir, "vendor")
	if err := os.MkdirAll(vendorDir, 0755); err != nil {
		t.Fatalf("MkdirAll() error = %v", err)
	}

	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, 50*time.Millisecond, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	go w.Start(ctx)

	time.Sleep(100 * time.Millisecond)

	// Verify vendor is not in watched list
	watched := w.WatchedFiles()
	for _, path := range watched {
		if filepath.Base(path) == "vendor" {
			t.Error("vendor directory should not be watched")
		}
	}
}

func TestWatcher_Debounce(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, 200*time.Millisecond, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	var callbackCount int32

	w.SetCallback(func(path string) {
		atomic.AddInt32(&callbackCount, 1)
	})

	testFile := filepath.Join(tmpDir, "test.go")

	// Simulate rapid changes
	for i := 0; i < 5; i++ {
		event := fsnotify.Event{
			Name: testFile,
			Op:   fsnotify.Write,
		}
		w.handleEvent(event)
		time.Sleep(10 * time.Millisecond)
	}

	// Wait for debounce
	time.Sleep(300 * time.Millisecond)

	// Process pending
	w.processPending()

	// Wait for callback
	time.Sleep(50 * time.Millisecond)

	count := atomic.LoadInt32(&callbackCount)
	if count != 1 {
		t.Errorf("callback count = %d, want 1 (debounced)", count)
	}
}

func TestWatcher_ConcurrentHandleEvent(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, time.Hour, nil)
	if err != nil {
		t.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	var wg sync.WaitGroup
	numGoroutines := 10
	eventsPerGoroutine := 100

	for i := 0; i < numGoroutines; i++ {
		wg.Add(1)
		go func(id int) {
			defer wg.Done()
			for j := 0; j < eventsPerGoroutine; j++ {
				event := fsnotify.Event{
					Name: filepath.Join(tmpDir, "test.go"),
					Op:   fsnotify.Write,
				}
				w.handleEvent(event)
			}
		}(i)
	}

	wg.Wait()

	// Should not panic and pending should have the file
	w.mu.Lock()
	_, found := w.pending[filepath.Join(tmpDir, "test.go")]
	w.mu.Unlock()

	if !found {
		t.Error("file should be in pending after concurrent events")
	}
}

func BenchmarkHandleEvent(b *testing.B) {
	tmpDir := b.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, time.Hour, nil)
	if err != nil {
		b.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	event := fsnotify.Event{
		Name: filepath.Join(tmpDir, "test.go"),
		Op:   fsnotify.Write,
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		w.handleEvent(event)
	}
}

func BenchmarkProcessPending(b *testing.B) {
	tmpDir := b.TempDir()
	cfg := config.DefaultConfig()

	w, err := NewWatcher(tmpDir, cfg, 0, nil)
	if err != nil {
		b.Fatalf("NewWatcher() error = %v", err)
	}
	defer w.Stop()

	w.SetCallback(func(path string) {})

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		b.StopTimer()
		w.mu.Lock()
		for j := 0; j < 100; j++ {
			w.pending[filepath.Join(tmpDir, "test.go")] = time.Now().Add(-time.Hour)
		}
		w.mu.Unlock()
		b.StartTimer()

		w.processPending()
	}
}
