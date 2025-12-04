package progress

import (
	"fmt"
	"os"

	"github.com/schollz/progressbar/v3"
)

// Tracker wraps a progress bar for file processing.
type Tracker struct {
	bar   *progressbar.ProgressBar
	label string
}

// NewSpinner creates a spinner for operations with unknown total count.
func NewSpinner(label string) *Tracker {
	bar := progressbar.NewOptions(-1,
		progressbar.OptionSetWriter(os.Stderr),
		progressbar.OptionSetWidth(20),
		progressbar.OptionSetDescription(label),
		progressbar.OptionSpinnerType(14),
		progressbar.OptionClearOnFinish(),
	)
	return &Tracker{bar: bar, label: label}
}

// NewTracker creates a progress bar with the given label and total count.
func NewTracker(label string, total int) *Tracker {
	bar := progressbar.NewOptions(total,
		progressbar.OptionSetWriter(os.Stderr),
		progressbar.OptionShowCount(),
		progressbar.OptionSetWidth(30),
		progressbar.OptionSetDescription(label),
		progressbar.OptionUseANSICodes(true),
		progressbar.OptionSetElapsedTime(false),
		progressbar.OptionSetPredictTime(false),
		progressbar.OptionSetTheme(progressbar.Theme{
			Saucer:        "=",
			SaucerHead:    ">",
			SaucerPadding: " ",
			BarStart:      "[",
			BarEnd:        "]",
		}),
	)
	return &Tracker{bar: bar, label: label}
}

// Tick increments the progress by 1. Safe for concurrent use.
func (t *Tracker) Tick() {
	t.bar.Add(1)
}

// FinishSuccess clears the bar completely (no output).
func (t *Tracker) FinishSuccess() {
	t.bar.Finish()
	t.bar.Clear()
}

// FinishSkipped clears the bar and prints a skip message to stderr.
func (t *Tracker) FinishSkipped(reason string) {
	t.bar.Finish()
	t.bar.Clear()
	fmt.Fprintf(os.Stderr, "  %s skipped (%s)\n", t.label, reason)
}

// FinishError clears the bar and prints an error message to stderr.
func (t *Tracker) FinishError(err error) {
	t.bar.Finish()
	t.bar.Clear()
	fmt.Fprintf(os.Stderr, "  %s error: %v\n", t.label, err)
}
