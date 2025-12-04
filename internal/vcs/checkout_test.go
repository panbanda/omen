package vcs

import (
	"testing"
	"time"
)

func TestStartOfWeek(t *testing.T) {
	tests := []struct {
		name string
		in   time.Time
		want time.Time
	}{
		{
			name: "monday stays monday",
			in:   time.Date(2024, 12, 2, 15, 30, 0, 0, time.UTC), // Monday
			want: time.Date(2024, 12, 2, 0, 0, 0, 0, time.UTC),
		},
		{
			name: "wednesday goes to monday",
			in:   time.Date(2024, 12, 4, 10, 0, 0, 0, time.UTC), // Wednesday
			want: time.Date(2024, 12, 2, 0, 0, 0, 0, time.UTC),
		},
		{
			name: "sunday goes to previous monday",
			in:   time.Date(2024, 12, 8, 23, 59, 0, 0, time.UTC), // Sunday
			want: time.Date(2024, 12, 2, 0, 0, 0, 0, time.UTC),
		},
		{
			name: "saturday goes to monday",
			in:   time.Date(2024, 12, 7, 12, 0, 0, 0, time.UTC), // Saturday
			want: time.Date(2024, 12, 2, 0, 0, 0, 0, time.UTC),
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := startOfWeek(tt.in)
			if !got.Equal(tt.want) {
				t.Errorf("startOfWeek(%v) = %v, want %v", tt.in, got, tt.want)
			}
		})
	}
}

func TestStartOfMonth(t *testing.T) {
	tests := []struct {
		name string
		in   time.Time
		want time.Time
	}{
		{
			name: "first day stays",
			in:   time.Date(2024, 12, 1, 15, 30, 0, 0, time.UTC),
			want: time.Date(2024, 12, 1, 0, 0, 0, 0, time.UTC),
		},
		{
			name: "mid month goes to first",
			in:   time.Date(2024, 12, 15, 10, 0, 0, 0, time.UTC),
			want: time.Date(2024, 12, 1, 0, 0, 0, 0, time.UTC),
		},
		{
			name: "end of month goes to first",
			in:   time.Date(2024, 12, 31, 23, 59, 0, 0, time.UTC),
			want: time.Date(2024, 12, 1, 0, 0, 0, 0, time.UTC),
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := startOfMonth(tt.in)
			if !got.Equal(tt.want) {
				t.Errorf("startOfMonth(%v) = %v, want %v", tt.in, got, tt.want)
			}
		})
	}
}

func TestFindCommitAtOrAfter(t *testing.T) {
	commits := []CommitInfo{
		{SHA: "newer", Date: time.Date(2024, 12, 15, 12, 0, 0, 0, time.UTC)},
		{SHA: "older", Date: time.Date(2024, 12, 1, 12, 0, 0, 0, time.UTC)},
	}

	tests := []struct {
		name    string
		target  time.Time
		wantSHA string
		wantNil bool
	}{
		{
			name:    "exact match on older",
			target:  time.Date(2024, 12, 1, 12, 0, 0, 0, time.UTC),
			wantSHA: "older",
		},
		{
			name:    "between commits finds newer",
			target:  time.Date(2024, 12, 10, 0, 0, 0, 0, time.UTC),
			wantSHA: "newer",
		},
		{
			name:    "before all commits finds oldest",
			target:  time.Date(2024, 11, 1, 0, 0, 0, 0, time.UTC),
			wantSHA: "older",
		},
		{
			name:    "after all commits returns nil",
			target:  time.Date(2024, 12, 20, 0, 0, 0, 0, time.UTC),
			wantNil: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := findCommitAtOrAfter(commits, tt.target)
			if tt.wantNil {
				if got != nil {
					t.Errorf("expected nil, got %v", got)
				}
				return
			}
			if got == nil {
				t.Error("expected non-nil commit")
				return
			}
			if got.SHA != tt.wantSHA {
				t.Errorf("got SHA %v, want %v", got.SHA, tt.wantSHA)
			}
		})
	}
}

func TestGenerateBoundaries(t *testing.T) {
	tests := []struct {
		name   string
		period string
		since  time.Duration
		snap   bool
		minLen int
	}{
		{
			name:   "monthly snapped 1y",
			period: "monthly",
			since:  365 * 24 * time.Hour,
			snap:   true,
			minLen: 10, // at least 10 months
		},
		{
			name:   "weekly snapped 3m",
			period: "weekly",
			since:  90 * 24 * time.Hour,
			snap:   true,
			minLen: 10, // at least 10 weeks
		},
		{
			name:   "monthly unsnapped 6m",
			period: "monthly",
			since:  180 * 24 * time.Hour,
			snap:   false,
			minLen: 5,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := generateBoundaries(tt.period, tt.since, tt.snap)
			if len(got) < tt.minLen {
				t.Errorf("got %d boundaries, want at least %d", len(got), tt.minLen)
			}

			// Verify boundaries are in order
			for i := 1; i < len(got); i++ {
				if !got[i].After(got[i-1]) {
					t.Errorf("boundary %d (%v) not after boundary %d (%v)",
						i, got[i], i-1, got[i-1])
				}
			}
		})
	}
}
