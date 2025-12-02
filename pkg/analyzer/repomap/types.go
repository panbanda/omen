package repomap

import (
	"sort"
	"time"
)

// Symbol represents a code symbol in the repository map.
type Symbol struct {
	Name      string  `json:"name"`
	Kind      string  `json:"kind"` // function, class, method, etc.
	File      string  `json:"file"`
	Line      int     `json:"line"`
	Signature string  `json:"signature"` // Full signature or summary
	PageRank  float64 `json:"pagerank"`
	InDegree  int     `json:"in_degree"`  // How many symbols call/use this
	OutDegree int     `json:"out_degree"` // How many symbols this calls/uses
}

// Summary provides aggregate statistics for the repo map.
type Summary struct {
	TotalSymbols   int     `json:"total_symbols"`
	TotalFiles     int     `json:"total_files"`
	AvgPageRank    float64 `json:"avg_pagerank"`
	MaxPageRank    float64 `json:"max_pagerank"`
	AvgConnections float64 `json:"avg_connections"`
}

// Map represents a PageRank-ranked summary of repository symbols.
type Map struct {
	GeneratedAt time.Time `json:"generated_at"`
	Symbols     []Symbol  `json:"symbols"`
	Summary     Summary   `json:"summary"`
}

// SortByPageRank sorts symbols by PageRank in descending order.
func (r *Map) SortByPageRank() {
	sort.Slice(r.Symbols, func(i, j int) bool {
		return r.Symbols[i].PageRank > r.Symbols[j].PageRank
	})
}

// CalculateSummary computes summary statistics for the repo map.
func (r *Map) CalculateSummary() {
	if len(r.Symbols) == 0 {
		return
	}

	files := make(map[string]bool)
	var totalPR float64
	var maxPR float64
	var totalConnections int

	for _, s := range r.Symbols {
		files[s.File] = true
		totalPR += s.PageRank
		if s.PageRank > maxPR {
			maxPR = s.PageRank
		}
		totalConnections += s.InDegree + s.OutDegree
	}

	r.Summary = Summary{
		TotalSymbols:   len(r.Symbols),
		TotalFiles:     len(files),
		AvgPageRank:    totalPR / float64(len(r.Symbols)),
		MaxPageRank:    maxPR,
		AvgConnections: float64(totalConnections) / float64(len(r.Symbols)),
	}
}

// TopN returns the top N symbols by PageRank.
func (r *Map) TopN(n int) []Symbol {
	r.SortByPageRank()
	if n > len(r.Symbols) {
		n = len(r.Symbols)
	}
	return r.Symbols[:n]
}
