package progress

import (
	"errors"
	"sync"
	"testing"
)

func TestNewTracker(t *testing.T) {
	tests := []struct {
		name  string
		label string
		total int
	}{
		{
			name:  "standard tracker",
			label: "Processing files",
			total: 100,
		},
		{
			name:  "zero total",
			label: "Empty task",
			total: 0,
		},
		{
			name:  "single item",
			label: "One file",
			total: 1,
		},
		{
			name:  "large total",
			label: "Many files",
			total: 10000,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tracker := NewTracker(tt.label, tt.total)

			if tracker == nil {
				t.Fatal("NewTracker() returned nil")
			}

			if tracker.bar == nil {
				t.Error("tracker.bar should not be nil")
			}

			if tracker.label != tt.label {
				t.Errorf("tracker.label = %q, want %q", tracker.label, tt.label)
			}
		})
	}
}

func TestNewTrackerNegativeTotal(t *testing.T) {
	tracker := NewTracker("Negative test", -5)

	if tracker == nil {
		t.Fatal("NewTracker() returned nil")
	}

	if tracker.bar == nil {
		t.Error("tracker.bar should not be nil")
	}
}

func TestNewSpinner(t *testing.T) {
	tests := []struct {
		name  string
		label string
	}{
		{
			name:  "standard spinner",
			label: "Loading...",
		},
		{
			name:  "empty label",
			label: "",
		},
		{
			name:  "long label",
			label: "Processing a very long operation that may take some time",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tracker := NewSpinner(tt.label)

			if tracker == nil {
				t.Fatal("NewSpinner() returned nil")
			}

			if tracker.bar == nil {
				t.Error("tracker.bar should not be nil")
			}

			if tracker.label != tt.label {
				t.Errorf("tracker.label = %q, want %q", tracker.label, tt.label)
			}
		})
	}
}

func TestTrackerTick(t *testing.T) {
	tests := []struct {
		name  string
		total int
		ticks int
	}{
		{
			name:  "single tick",
			total: 10,
			ticks: 1,
		},
		{
			name:  "multiple ticks",
			total: 10,
			ticks: 5,
		},
		{
			name:  "ticks equal to total",
			total: 10,
			ticks: 10,
		},
		{
			name:  "ticks exceed total",
			total: 10,
			ticks: 15,
		},
		{
			name:  "zero ticks",
			total: 10,
			ticks: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tracker := NewTracker("Test", tt.total)

			for i := 0; i < tt.ticks; i++ {
				tracker.Tick()
			}

			tracker.FinishSuccess()
		})
	}
}

func TestTrackerTickConcurrent(t *testing.T) {
	tracker := NewTracker("Concurrent test", 1000)

	var wg sync.WaitGroup
	workers := 10
	ticksPerWorker := 100

	for i := 0; i < workers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < ticksPerWorker; j++ {
				tracker.Tick()
			}
		}()
	}

	wg.Wait()
	tracker.FinishSuccess()
}

func TestTrackerTickRapidUpdates(t *testing.T) {
	tracker := NewTracker("Rapid updates", 10000)

	for i := 0; i < 10000; i++ {
		tracker.Tick()
	}

	tracker.FinishSuccess()
}

func TestTrackerFinishSuccess(t *testing.T) {
	tracker := NewTracker("Success test", 10)

	for i := 0; i < 10; i++ {
		tracker.Tick()
	}

	tracker.FinishSuccess()
}

func TestTrackerFinishSuccessWithoutTicks(t *testing.T) {
	tracker := NewTracker("No ticks", 10)
	tracker.FinishSuccess()
}

func TestTrackerFinishSuccessMultipleCalls(t *testing.T) {
	tracker := NewTracker("Multiple finish", 10)
	tracker.Tick()

	tracker.FinishSuccess()
	tracker.FinishSuccess()
}

func TestTrackerFinishSkipped(t *testing.T) {
	tests := []struct {
		name   string
		reason string
	}{
		{
			name:   "cache hit",
			reason: "cached",
		},
		{
			name:   "already processed",
			reason: "already processed",
		},
		{
			name:   "empty reason",
			reason: "",
		},
		{
			name:   "long reason",
			reason: "this is a very long reason for skipping that contains many words",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tracker := NewTracker("Skip test", 10)
			tracker.Tick()
			tracker.FinishSkipped(tt.reason)
		})
	}
}

func TestTrackerFinishError(t *testing.T) {
	tests := []struct {
		name string
		err  error
	}{
		{
			name: "simple error",
			err:  errors.New("something went wrong"),
		},
		{
			name: "file error",
			err:  errors.New("file not found"),
		},
		{
			name: "wrapped error",
			err:  errors.New("wrapped: original error"),
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tracker := NewTracker("Error test", 10)
			tracker.Tick()
			tracker.FinishError(tt.err)
		})
	}
}

func TestTrackerLifecycle(t *testing.T) {
	tests := []struct {
		name       string
		total      int
		ticks      int
		finishFunc func(*Tracker)
	}{
		{
			name:  "complete success",
			total: 5,
			ticks: 5,
			finishFunc: func(tr *Tracker) {
				tr.FinishSuccess()
			},
		},
		{
			name:  "partial with skip",
			total: 10,
			ticks: 3,
			finishFunc: func(tr *Tracker) {
				tr.FinishSkipped("not needed")
			},
		},
		{
			name:  "partial with error",
			total: 10,
			ticks: 7,
			finishFunc: func(tr *Tracker) {
				tr.FinishError(errors.New("failed"))
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tracker := NewTracker(tt.name, tt.total)

			for i := 0; i < tt.ticks; i++ {
				tracker.Tick()
			}

			tt.finishFunc(tracker)
		})
	}
}

func TestSpinnerLifecycle(t *testing.T) {
	tests := []struct {
		name       string
		ticks      int
		finishFunc func(*Tracker)
	}{
		{
			name:  "spinner success",
			ticks: 10,
			finishFunc: func(tr *Tracker) {
				tr.FinishSuccess()
			},
		},
		{
			name:  "spinner skip",
			ticks: 5,
			finishFunc: func(tr *Tracker) {
				tr.FinishSkipped("cached")
			},
		},
		{
			name:  "spinner error",
			ticks: 3,
			finishFunc: func(tr *Tracker) {
				tr.FinishError(errors.New("timeout"))
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tracker := NewSpinner(tt.name)

			for i := 0; i < tt.ticks; i++ {
				tracker.Tick()
			}

			tt.finishFunc(tracker)
		})
	}
}

func TestTrackerZeroTotal(t *testing.T) {
	tracker := NewTracker("Zero total", 0)

	tracker.Tick()
	tracker.Tick()

	tracker.FinishSuccess()
}

func TestTrackerNilSafety(t *testing.T) {
	tracker := NewTracker("Nil safety", 10)

	if tracker.bar == nil {
		t.Fatal("bar should not be nil after creation")
	}

	tracker.Tick()
	tracker.FinishSuccess()
}

func TestMultipleTrackers(t *testing.T) {
	tracker1 := NewTracker("Task 1", 10)
	tracker2 := NewTracker("Task 2", 20)
	tracker3 := NewSpinner("Task 3")

	for i := 0; i < 10; i++ {
		tracker1.Tick()
	}
	tracker1.FinishSuccess()

	for i := 0; i < 20; i++ {
		tracker2.Tick()
	}
	tracker2.FinishSuccess()

	for i := 0; i < 5; i++ {
		tracker3.Tick()
	}
	tracker3.FinishSuccess()
}

func TestTrackerInterleaved(t *testing.T) {
	tracker1 := NewTracker("Interleaved 1", 10)
	tracker2 := NewTracker("Interleaved 2", 10)

	tracker1.Tick()
	tracker2.Tick()
	tracker1.Tick()
	tracker2.Tick()

	tracker1.FinishSuccess()
	tracker2.FinishSuccess()
}

func BenchmarkTrackerCreation(b *testing.B) {
	for i := 0; i < b.N; i++ {
		tracker := NewTracker("Benchmark", 100)
		tracker.FinishSuccess()
	}
}

func BenchmarkTrackerTick(b *testing.B) {
	tracker := NewTracker("Benchmark", b.N)
	b.ResetTimer()

	for i := 0; i < b.N; i++ {
		tracker.Tick()
	}

	tracker.FinishSuccess()
}

func BenchmarkSpinnerCreation(b *testing.B) {
	for i := 0; i < b.N; i++ {
		tracker := NewSpinner("Benchmark")
		tracker.FinishSuccess()
	}
}

func BenchmarkTrackerConcurrent(b *testing.B) {
	tracker := NewTracker("Concurrent benchmark", b.N)
	b.ResetTimer()

	var wg sync.WaitGroup
	workers := 10
	ticksPerWorker := b.N / workers

	for i := 0; i < workers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < ticksPerWorker; j++ {
				tracker.Tick()
			}
		}()
	}

	wg.Wait()
	tracker.FinishSuccess()
}
