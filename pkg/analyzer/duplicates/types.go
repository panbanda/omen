package duplicates

// Type represents the type of code clone detected.
type Type string

const (
	Type1 Type = "type1" // Exact (whitespace only differs)
	Type2 Type = "type2" // Parametric (identifiers/literals differ)
	Type3 Type = "type3" // Structural (statements added/removed)
)

// String returns the string representation.
func (t Type) String() string {
	return string(t)
}

// Clone represents a detected duplicate code fragment.
type Clone struct {
	Type       Type    `json:"type"`
	Similarity float64 `json:"similarity"`
	FileA      string  `json:"file_a"`
	FileB      string  `json:"file_b"`
	StartLineA uint32  `json:"start_line_a"`
	EndLineA   uint32  `json:"end_line_a"`
	StartLineB uint32  `json:"start_line_b"`
	EndLineB   uint32  `json:"end_line_b"`
	LinesA     int     `json:"lines_a"`
	LinesB     int     `json:"lines_b"`
	TokenCount int     `json:"token_count,omitempty"`
	GroupID    uint64  `json:"group_id,omitempty"`
}

// Instance represents a single occurrence within a clone group.
type Instance struct {
	File           string  `json:"file"`
	StartLine      uint32  `json:"start_line"`
	EndLine        uint32  `json:"end_line"`
	Lines          int     `json:"lines"`
	NormalizedHash uint64  `json:"normalized_hash"`
	Similarity     float64 `json:"similarity"`
}

// Group represents a group of similar code fragments.
type Group struct {
	ID                uint64     `json:"id"`
	Type              Type       `json:"type"`
	Instances         []Instance `json:"instances"`
	TotalLines        int        `json:"total_lines"`
	TotalTokens       int        `json:"total_tokens"`
	AverageSimilarity float64    `json:"average_similarity"`
}

// Analysis represents the full duplicate detection result.
type Analysis struct {
	Clones            []Clone `json:"clones"`
	Groups            []Group `json:"groups,omitempty"`
	Summary           Summary `json:"summary"`
	TotalFilesScanned int     `json:"total_files_scanned"`
	MinLines          int     `json:"min_lines"`
	Threshold         float64 `json:"threshold"`
}

// Summary provides aggregate statistics.
type Summary struct {
	TotalClones      int            `json:"total_clones"`
	TotalGroups      int            `json:"total_groups"`
	Type1Count       int            `json:"type1_count"`
	Type2Count       int            `json:"type2_count"`
	Type3Count       int            `json:"type3_count"`
	DuplicatedLines  int            `json:"duplicated_lines"`
	TotalLines       int            `json:"total_lines"`
	DuplicationRatio float64        `json:"duplication_ratio"`
	FileOccurrences  map[string]int `json:"file_occurrences"`
	AvgSimilarity    float64        `json:"avg_similarity"`
	P50Similarity    float64        `json:"p50_similarity"`
	P95Similarity    float64        `json:"p95_similarity"`
	Hotspots         []Hotspot      `json:"hotspots,omitempty"`
}

// Hotspot represents a file with high duplication.
type Hotspot struct {
	File            string  `json:"file"`
	DuplicateLines  int     `json:"duplicate_lines"`
	CloneGroupCount int     `json:"clone_group_count"`
	Severity        float64 `json:"severity"`
}

// NewSummary creates an initialized summary.
func NewSummary() Summary {
	return Summary{
		FileOccurrences: make(map[string]int),
	}
}

// AddClone updates the summary with a new clone.
func (s *Summary) AddClone(c Clone) {
	s.TotalClones++
	s.FileOccurrences[c.FileA]++
	if c.FileA != c.FileB {
		s.FileOccurrences[c.FileB]++
	}
	s.DuplicatedLines += c.LinesA + c.LinesB

	switch c.Type {
	case Type1:
		s.Type1Count++
	case Type2:
		s.Type2Count++
	case Type3:
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

// ReportSummary is the pmat-compatible summary format.
type ReportSummary struct {
	TotalFiles       int     `json:"total_files"`
	TotalFragments   int     `json:"total_fragments"`
	DuplicateLines   int     `json:"duplicate_lines"`
	TotalLines       int     `json:"total_lines"`
	DuplicationRatio float64 `json:"duplication_ratio"`
	CloneGroups      int     `json:"clone_groups"`
	LargestGroupSize int     `json:"largest_group_size"`
}

// Report is the pmat-compatible output format.
type Report struct {
	Summary  ReportSummary `json:"summary"`
	Groups   []Group       `json:"groups"`
	Hotspots []Hotspot     `json:"hotspots"`
}

// ToReport converts Analysis to pmat-compatible format.
func (a *Analysis) ToReport() *Report {
	largestGroupSize := 0
	for _, g := range a.Groups {
		if len(g.Instances) > largestGroupSize {
			largestGroupSize = len(g.Instances)
		}
	}

	return &Report{
		Summary: ReportSummary{
			TotalFiles:       a.TotalFilesScanned,
			TotalFragments:   a.Summary.TotalClones,
			DuplicateLines:   a.Summary.DuplicatedLines,
			TotalLines:       a.Summary.TotalLines,
			DuplicationRatio: a.Summary.DuplicationRatio,
			CloneGroups:      len(a.Groups),
			LargestGroupSize: largestGroupSize,
		},
		Groups:   a.Groups,
		Hotspots: a.Summary.Hotspots,
	}
}

// Config holds duplicate detection configuration.
type Config struct {
	MinTokens            int
	SimilarityThreshold  float64
	ShingleSize          int
	NumHashFunctions     int
	NumBands             int
	RowsPerBand          int
	NormalizeIdentifiers bool
	NormalizeLiterals    bool
	IgnoreComments       bool
	MinGroupSize         int
}

// DefaultConfig returns pmat-compatible defaults.
func DefaultConfig() Config {
	return Config{
		MinTokens:            50,
		SimilarityThreshold:  0.70,
		ShingleSize:          5,
		NumHashFunctions:     200,
		NumBands:             20,
		RowsPerBand:          10,
		NormalizeIdentifiers: true,
		NormalizeLiterals:    true,
		IgnoreComments:       true,
		MinGroupSize:         2,
	}
}
