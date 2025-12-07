package analyzer

import (
	"context"
	"sync"
	"testing"
)

func TestTracker_AddAndTick(t *testing.T) {
	var calls []struct {
		current, total int
		path           string
	}
	var mu sync.Mutex

	tracker := NewTracker(func(current, total int, path string) {
		mu.Lock()
		calls = append(calls, struct {
			current, total int
			path           string
		}{current, total, path})
		mu.Unlock()
	})

	tracker.Add(3)
	tracker.Tick("file1.go")
	tracker.Tick("file2.go")
	tracker.Tick("file3.go")

	if got := tracker.Total(); got != 3 {
		t.Errorf("Total() = %d, want 3", got)
	}
	if got := tracker.Current(); got != 3 {
		t.Errorf("Current() = %d, want 3", got)
	}

	if len(calls) != 3 {
		t.Fatalf("expected 3 callback calls, got %d", len(calls))
	}

	// Verify callback values
	if calls[0].current != 1 || calls[0].total != 3 || calls[0].path != "file1.go" {
		t.Errorf("call 1: got (%d, %d, %q), want (1, 3, \"file1.go\")",
			calls[0].current, calls[0].total, calls[0].path)
	}
	if calls[2].current != 3 || calls[2].total != 3 || calls[2].path != "file3.go" {
		t.Errorf("call 3: got (%d, %d, %q), want (3, 3, \"file3.go\")",
			calls[2].current, calls[2].total, calls[2].path)
	}
}

func TestTracker_SetTotal(t *testing.T) {
	tracker := NewTracker(nil)
	tracker.Add(5)
	if got := tracker.Total(); got != 5 {
		t.Errorf("after Add(5): Total() = %d, want 5", got)
	}

	tracker.SetTotal(10)
	if got := tracker.Total(); got != 10 {
		t.Errorf("after SetTotal(10): Total() = %d, want 10", got)
	}
}

func TestTracker_ConcurrentTicks(t *testing.T) {
	tracker := NewTracker(nil)
	tracker.Add(100)

	var wg sync.WaitGroup
	for i := 0; i < 100; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			tracker.Tick("file.go")
		}()
	}
	wg.Wait()

	if got := tracker.Current(); got != 100 {
		t.Errorf("Current() = %d, want 100", got)
	}
}

func TestTracker_NilCallback(t *testing.T) {
	tracker := NewTracker(nil)
	tracker.Add(1)
	tracker.Tick("file.go") // Should not panic
}

func TestWithTracker(t *testing.T) {
	tracker := NewTracker(nil)
	ctx := WithTracker(context.Background(), tracker)

	got := TrackerFromContext(ctx)
	if got != tracker {
		t.Error("TrackerFromContext should return the same tracker")
	}
}

func TestTrackerFromContext_Nil(t *testing.T) {
	got := TrackerFromContext(context.Background())
	if got != nil {
		t.Error("TrackerFromContext should return nil for context without tracker")
	}
}
