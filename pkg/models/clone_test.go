package models

import (
	"testing"
)

func TestMinHashSignature_JaccardSimilarity(t *testing.T) {
	tests := []struct {
		name     string
		sig1     MinHashSignature
		sig2     MinHashSignature
		expected float64
	}{
		{
			name: "identical signatures",
			sig1: MinHashSignature{Values: []uint64{1, 2, 3, 4, 5}},
			sig2: MinHashSignature{Values: []uint64{1, 2, 3, 4, 5}},
			expected: 1.0,
		},
		{
			name: "completely different signatures",
			sig1: MinHashSignature{Values: []uint64{1, 2, 3, 4, 5}},
			sig2: MinHashSignature{Values: []uint64{6, 7, 8, 9, 10}},
			expected: 0.0,
		},
		{
			name: "50% similar",
			sig1: MinHashSignature{Values: []uint64{1, 2, 3, 4}},
			sig2: MinHashSignature{Values: []uint64{1, 2, 5, 6}},
			expected: 0.5,
		},
		{
			name: "25% similar",
			sig1: MinHashSignature{Values: []uint64{1, 2, 3, 4}},
			sig2: MinHashSignature{Values: []uint64{1, 5, 6, 7}},
			expected: 0.25,
		},
		{
			name: "empty signatures",
			sig1: MinHashSignature{Values: []uint64{}},
			sig2: MinHashSignature{Values: []uint64{}},
			expected: 0.0,
		},
		{
			name: "different length signatures",
			sig1: MinHashSignature{Values: []uint64{1, 2, 3}},
			sig2: MinHashSignature{Values: []uint64{1, 2}},
			expected: 0.0,
		},
		{
			name: "nil values",
			sig1: MinHashSignature{Values: nil},
			sig2: MinHashSignature{Values: []uint64{1, 2, 3}},
			expected: 0.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.sig1.JaccardSimilarity(&tt.sig2)
			if got != tt.expected {
				t.Errorf("JaccardSimilarity() = %v, expected %v", got, tt.expected)
			}
		})
	}
}

func TestCloneSummary_AddClone(t *testing.T) {
	tests := []struct {
		name               string
		clones             []CodeClone
		expectedTotal      int
		expectedType1      int
		expectedType2      int
		expectedType3      int
		expectedDupLines   int
		expectedFileCount  int
	}{
		{
			name: "single type1 clone",
			clones: []CodeClone{
				{
					Type:   CloneType1,
					FileA:  "file1.go",
					FileB:  "file2.go",
					LinesA: 10,
					LinesB: 10,
				},
			},
			expectedTotal:     1,
			expectedType1:     1,
			expectedType2:     0,
			expectedType3:     0,
			expectedDupLines:  20,
			expectedFileCount: 2,
		},
		{
			name: "multiple clone types",
			clones: []CodeClone{
				{
					Type:   CloneType1,
					FileA:  "file1.go",
					FileB:  "file2.go",
					LinesA: 5,
					LinesB: 5,
				},
				{
					Type:   CloneType2,
					FileA:  "file1.go",
					FileB:  "file3.go",
					LinesA: 8,
					LinesB: 8,
				},
				{
					Type:   CloneType3,
					FileA:  "file2.go",
					FileB:  "file3.go",
					LinesA: 12,
					LinesB: 12,
				},
			},
			expectedTotal:     3,
			expectedType1:     1,
			expectedType2:     1,
			expectedType3:     1,
			expectedDupLines:  50,
			expectedFileCount: 3,
		},
		{
			name: "same file clone",
			clones: []CodeClone{
				{
					Type:   CloneType1,
					FileA:  "file1.go",
					FileB:  "file1.go",
					LinesA: 10,
					LinesB: 10,
				},
			},
			expectedTotal:     1,
			expectedType1:     1,
			expectedDupLines:  20,
			expectedFileCount: 1,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := NewCloneSummary()

			for _, clone := range tt.clones {
				s.AddClone(clone)
			}

			if s.TotalClones != tt.expectedTotal {
				t.Errorf("TotalClones = %d, expected %d", s.TotalClones, tt.expectedTotal)
			}
			if s.Type1Count != tt.expectedType1 {
				t.Errorf("Type1Count = %d, expected %d", s.Type1Count, tt.expectedType1)
			}
			if s.Type2Count != tt.expectedType2 {
				t.Errorf("Type2Count = %d, expected %d", s.Type2Count, tt.expectedType2)
			}
			if s.Type3Count != tt.expectedType3 {
				t.Errorf("Type3Count = %d, expected %d", s.Type3Count, tt.expectedType3)
			}
			if s.DuplicatedLines != tt.expectedDupLines {
				t.Errorf("DuplicatedLines = %d, expected %d", s.DuplicatedLines, tt.expectedDupLines)
			}
			if len(s.FileOccurrences) != tt.expectedFileCount {
				t.Errorf("FileOccurrences count = %d, expected %d", len(s.FileOccurrences), tt.expectedFileCount)
			}
		})
	}
}

func TestNewCloneSummary(t *testing.T) {
	s := NewCloneSummary()

	if s.FileOccurrences == nil {
		t.Error("FileOccurrences should be initialized")
	}

	if len(s.FileOccurrences) != 0 {
		t.Errorf("FileOccurrences should be empty, got %d items", len(s.FileOccurrences))
	}

	if s.TotalClones != 0 {
		t.Error("TotalClones should be 0")
	}
}

func TestCloneType_Constants(t *testing.T) {
	if CloneType1 != "type1" {
		t.Errorf("CloneType1 = %s, expected type1", CloneType1)
	}
	if CloneType2 != "type2" {
		t.Errorf("CloneType2 = %s, expected type2", CloneType2)
	}
	if CloneType3 != "type3" {
		t.Errorf("CloneType3 = %s, expected type3", CloneType3)
	}
}

func TestCloneSummary_FileOccurrenceTracking(t *testing.T) {
	s := NewCloneSummary()

	clone1 := CodeClone{
		Type:   CloneType1,
		FileA:  "file1.go",
		FileB:  "file2.go",
		LinesA: 5,
		LinesB: 5,
	}

	clone2 := CodeClone{
		Type:   CloneType1,
		FileA:  "file1.go",
		FileB:  "file3.go",
		LinesA: 3,
		LinesB: 3,
	}

	s.AddClone(clone1)
	s.AddClone(clone2)

	if s.FileOccurrences["file1.go"] != 2 {
		t.Errorf("file1.go occurrences = %d, expected 2", s.FileOccurrences["file1.go"])
	}
	if s.FileOccurrences["file2.go"] != 1 {
		t.Errorf("file2.go occurrences = %d, expected 1", s.FileOccurrences["file2.go"])
	}
	if s.FileOccurrences["file3.go"] != 1 {
		t.Errorf("file3.go occurrences = %d, expected 1", s.FileOccurrences["file3.go"])
	}
}
