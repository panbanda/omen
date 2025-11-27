package models

// CloneType represents the type of code clone detected.
type CloneType string

const (
	CloneType1 CloneType = "type1" // Exact (whitespace only differs)
	CloneType2 CloneType = "type2" // Parametric (identifiers/literals differ)
	CloneType3 CloneType = "type3" // Structural (statements added/removed)
)

// CodeClone represents a detected duplicate code fragment.
type CodeClone struct {
	Type       CloneType `json:"type"`
	Similarity float64   `json:"similarity"`
	FileA      string    `json:"file_a"`
	FileB      string    `json:"file_b"`
	StartLineA uint32    `json:"start_line_a"`
	EndLineA   uint32    `json:"end_line_a"`
	StartLineB uint32    `json:"start_line_b"`
	EndLineB   uint32    `json:"end_line_b"`
	LinesA     int       `json:"lines_a"`
	LinesB     int       `json:"lines_b"`
	TokenCount int       `json:"token_count,omitempty"`
	GroupID    uint64    `json:"group_id,omitempty"`
}

// CloneInstance represents a single occurrence within a clone group.
type CloneInstance struct {
	File           string  `json:"file"`
	StartLine      uint32  `json:"start_line"`
	EndLine        uint32  `json:"end_line"`
	Lines          int     `json:"lines"`
	NormalizedHash uint64  `json:"normalized_hash"`
	Similarity     float64 `json:"similarity"`
}

// CloneGroup represents a group of similar code fragments.
type CloneGroup struct {
	ID                uint64          `json:"id"`
	Type              CloneType       `json:"type"`
	Instances         []CloneInstance `json:"instances"`
	TotalLines        int             `json:"total_lines"`
	TotalTokens       int             `json:"total_tokens"`
	AverageSimilarity float64         `json:"average_similarity"`
}

// CloneAnalysis represents the full duplicate detection result.
type CloneAnalysis struct {
	Clones            []CodeClone  `json:"clones"`
	Groups            []CloneGroup `json:"groups,omitempty"`
	Summary           CloneSummary `json:"summary"`
	TotalFilesScanned int          `json:"total_files_scanned"`
	MinLines          int          `json:"min_lines"`
	Threshold         float64      `json:"threshold"`
}

// CloneSummary provides aggregate statistics.
type CloneSummary struct {
	TotalClones      int                  `json:"total_clones"`
	TotalGroups      int                  `json:"total_groups"`
	Type1Count       int                  `json:"type1_count"`
	Type2Count       int                  `json:"type2_count"`
	Type3Count       int                  `json:"type3_count"`
	DuplicatedLines  int                  `json:"duplicated_lines"`
	TotalLines       int                  `json:"total_lines"`
	DuplicationRatio float64              `json:"duplication_ratio"`
	FileOccurrences  map[string]int       `json:"file_occurrences"`
	AvgSimilarity    float64              `json:"avg_similarity"`
	P50Similarity    float64              `json:"p50_similarity"`
	P95Similarity    float64              `json:"p95_similarity"`
	Hotspots         []DuplicationHotspot `json:"hotspots,omitempty"`
}

// DuplicationHotspot represents a file with high duplication.
type DuplicationHotspot struct {
	File            string  `json:"file"`
	DuplicateLines  int     `json:"duplicate_lines"`
	CloneGroupCount int     `json:"clone_group_count"`
	Severity        float64 `json:"severity"`
}

// NewCloneSummary creates an initialized summary.
func NewCloneSummary() CloneSummary {
	return CloneSummary{
		FileOccurrences: make(map[string]int),
	}
}

// AddClone updates the summary with a new clone.
func (s *CloneSummary) AddClone(c CodeClone) {
	s.TotalClones++
	s.FileOccurrences[c.FileA]++
	if c.FileA != c.FileB {
		s.FileOccurrences[c.FileB]++
	}
	s.DuplicatedLines += c.LinesA + c.LinesB

	switch c.Type {
	case CloneType1:
		s.Type1Count++
	case CloneType2:
		s.Type2Count++
	case CloneType3:
		s.Type3Count++
	}
}

// MinHashSignature represents a MinHash signature for similarity estimation.
type MinHashSignature struct {
	Values []uint64 `json:"values"`
}

// JaccardSimilarity computes similarity between two MinHash signatures.
func (s *MinHashSignature) JaccardSimilarity(other *MinHashSignature) float64 {
	if len(s.Values) != len(other.Values) || len(s.Values) == 0 {
		return 0.0
	}

	matches := 0
	for i := range s.Values {
		if s.Values[i] == other.Values[i] {
			matches++
		}
	}

	return float64(matches) / float64(len(s.Values))
}
