package analyzer

import (
	"context"
	"sync/atomic"
)

// ProgressFunc is called to report analysis progress.
// current is the number of items processed, total is the total count,
// and path is the current item being processed.
type ProgressFunc func(current, total int, path string)

// Tracker tracks progress for analysis operations.
// It is safe for concurrent use from multiple goroutines.
type Tracker struct {
	total    atomic.Int32
	current  atomic.Int32
	callback ProgressFunc
}

// NewTracker creates a new progress tracker with the given callback.
// The callback is invoked on each Tick with (current, total, path).
func NewTracker(callback ProgressFunc) *Tracker {
	return &Tracker{callback: callback}
}

// Add increments the total count by n. Call this when you discover
// how many items will be processed.
func (t *Tracker) Add(n int) {
	t.total.Add(int32(n))
}

// SetTotal sets the total count. This replaces any previous total.
func (t *Tracker) SetTotal(n int) {
	t.total.Store(int32(n))
}

// Tick marks one item as completed. The path identifies the completed item.
// This increments the current count and invokes the callback if set.
func (t *Tracker) Tick(path string) {
	current := int(t.current.Add(1))
	total := int(t.total.Load())
	if t.callback != nil {
		t.callback(current, total, path)
	}
}

// Current returns the current progress count.
func (t *Tracker) Current() int {
	return int(t.current.Load())
}

// Total returns the total count.
func (t *Tracker) Total() int {
	return int(t.total.Load())
}

type trackerKey struct{}

// WithTracker returns a context that carries a progress tracker.
// Use TrackerFromContext to extract it in the processing layer.
func WithTracker(ctx context.Context, t *Tracker) context.Context {
	return context.WithValue(ctx, trackerKey{}, t)
}

// TrackerFromContext extracts the progress tracker from the context.
// Returns nil if no tracker was set.
func TrackerFromContext(ctx context.Context) *Tracker {
	if t, ok := ctx.Value(trackerKey{}).(*Tracker); ok {
		return t
	}
	return nil
}
