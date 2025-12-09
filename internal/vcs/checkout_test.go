package vcs

import (
	"sort"
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
	// Commits sorted newest-first by date (expected order)
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

func TestFindCommitAtOrAfterWithSorting(t *testing.T) {
	// Simulates go-git's graph traversal order where commits are NOT sorted by date.
	// go-git returns commits in graph traversal order, not chronological order.
	// In a repo with merge commits, older commits (by date) can appear BEFORE
	// newer commits in the iteration.
	//
	// FindCommitsAtIntervals must sort commits by date before calling
	// findCommitAtOrAfter, which expects newest-first order.
	unsortedCommits := []CommitInfo{
		{SHA: "head", Date: time.Date(2025, 12, 9, 12, 0, 0, 0, time.UTC)},       // index 0: newest
		{SHA: "oldest2016", Date: time.Date(2016, 9, 13, 12, 0, 0, 0, time.UTC)}, // index 1: oldest by DATE but in middle of iteration
		{SHA: "mid2023", Date: time.Date(2023, 6, 1, 12, 0, 0, 0, time.UTC)},     // index 2
		{SHA: "late2023", Date: time.Date(2023, 11, 1, 12, 0, 0, 0, time.UTC)},   // index 3
		{SHA: "early2024", Date: time.Date(2024, 1, 9, 12, 0, 0, 0, time.UTC)},   // index 4: LAST in iteration (but not oldest!)
	}

	// Before fix: findCommitAtOrAfter with unsorted commits returns wrong results
	t.Run("unsorted commits return wrong result", func(t *testing.T) {
		got := findCommitAtOrAfter(unsortedCommits, time.Date(2016, 1, 1, 0, 0, 0, 0, time.UTC))
		if got == nil || got.SHA != "early2024" {
			t.Skip("behavior changed - this documents the bug that existed before the fix")
		}
		// This is the bug: we asked for commit on/after 2016-01-01 and got 2024-01-09
	})

	// After fix: sort commits by date (newest first) before calling findCommitAtOrAfter
	sortedCommits := make([]CommitInfo, len(unsortedCommits))
	copy(sortedCommits, unsortedCommits)
	sort.Slice(sortedCommits, func(i, j int) bool {
		return sortedCommits[i].Date.After(sortedCommits[j].Date)
	})

	tests := []struct {
		name    string
		target  time.Time
		wantSHA string
	}{
		{
			name:    "finds oldest commit from 2016",
			target:  time.Date(2016, 1, 1, 0, 0, 0, 0, time.UTC),
			wantSHA: "oldest2016",
		},
		{
			name:    "finds 2023 commit",
			target:  time.Date(2023, 5, 1, 0, 0, 0, 0, time.UTC),
			wantSHA: "mid2023",
		},
		{
			name:    "finds late 2023 commit",
			target:  time.Date(2023, 10, 1, 0, 0, 0, 0, time.UTC),
			wantSHA: "late2023",
		},
		{
			name:    "finds early 2024 commit",
			target:  time.Date(2024, 1, 1, 0, 0, 0, 0, time.UTC),
			wantSHA: "early2024",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := findCommitAtOrAfter(sortedCommits, tt.target)
			if got == nil {
				t.Errorf("expected non-nil commit for target %v", tt.target)
				return
			}
			if got.SHA != tt.wantSHA {
				t.Errorf("got SHA %v, want %v (target: %v)", got.SHA, tt.wantSHA, tt.target)
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
