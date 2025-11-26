package analyzer

import (
	"hash/fnv"
	"os"
	"sort"
	"strings"

	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
)

// DuplicateAnalyzer detects code clones using MinHash.
type DuplicateAnalyzer struct {
	parser    *parser.Parser
	minLines  int
	threshold float64
	numHashes int
}

// NewDuplicateAnalyzer creates a new duplicate analyzer.
func NewDuplicateAnalyzer(minLines int, threshold float64) *DuplicateAnalyzer {
	if minLines <= 0 {
		minLines = 6
	}
	if threshold <= 0 || threshold > 1 {
		threshold = 0.8
	}
	return &DuplicateAnalyzer{
		parser:    parser.New(),
		minLines:  minLines,
		threshold: threshold,
		numHashes: 128, // Number of hash functions for MinHash
	}
}

// fileBlock represents a chunk of code for clone detection.
type fileBlock struct {
	file      string
	startLine uint32
	endLine   uint32
	content   string
	signature *models.MinHashSignature
}

// AnalyzeProject detects code clones across a project.
func (a *DuplicateAnalyzer) AnalyzeProject(files []string) (*models.CloneAnalysis, error) {
	return a.AnalyzeProjectWithProgress(files, nil)
}

// AnalyzeProjectWithProgress detects code clones with optional progress callback.
func (a *DuplicateAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress ProgressFunc) (*models.CloneAnalysis, error) {
	analysis := &models.CloneAnalysis{
		Clones:            make([]models.CodeClone, 0),
		Summary:           models.NewCloneSummary(),
		TotalFilesScanned: len(files),
		MinLines:          a.minLines,
		Threshold:         a.threshold,
	}

	// Extract blocks from all files in parallel
	fileBlocks := ForEachFileWithProgress(files, func(path string) ([]fileBlock, error) {
		return a.extractBlocks(path)
	}, onProgress)

	var allBlocks []fileBlock
	for _, blocks := range fileBlocks {
		allBlocks = append(allBlocks, blocks...)
	}

	// Compute MinHash signatures for all blocks
	for i := range allBlocks {
		allBlocks[i].signature = a.computeMinHash(allBlocks[i].content)
	}

	// Compare all pairs of blocks
	for i := 0; i < len(allBlocks); i++ {
		for j := i + 1; j < len(allBlocks); j++ {
			blockA := allBlocks[i]
			blockB := allBlocks[j]

			// Skip if same file and overlapping
			if blockA.file == blockB.file {
				if blockA.startLine <= blockB.endLine && blockB.startLine <= blockA.endLine {
					continue
				}
			}

			// Calculate similarity
			similarity := blockA.signature.JaccardSimilarity(blockB.signature)
			if similarity >= a.threshold {
				clone := models.CodeClone{
					Type:       determineCloneType(similarity),
					Similarity: similarity,
					FileA:      blockA.file,
					FileB:      blockB.file,
					StartLineA: blockA.startLine,
					EndLineA:   blockA.endLine,
					StartLineB: blockB.startLine,
					EndLineB:   blockB.endLine,
					LinesA:     int(blockA.endLine - blockA.startLine + 1),
					LinesB:     int(blockB.endLine - blockB.startLine + 1),
				}
				analysis.Clones = append(analysis.Clones, clone)
				analysis.Summary.AddClone(clone)
			}
		}
	}

	// Calculate average similarity and percentiles
	if len(analysis.Clones) > 0 {
		similarities := make([]float64, len(analysis.Clones))
		var totalSim float64
		for i, c := range analysis.Clones {
			similarities[i] = c.Similarity
			totalSim += c.Similarity
		}
		analysis.Summary.AvgSimilarity = totalSim / float64(len(analysis.Clones))

		sort.Float64s(similarities)
		analysis.Summary.P50Similarity = percentileFloat64Dup(similarities, 50)
		analysis.Summary.P95Similarity = percentileFloat64Dup(similarities, 95)
	}

	return analysis, nil
}

// percentileFloat64Dup calculates the p-th percentile of a sorted slice.
func percentileFloat64Dup(sorted []float64, p int) float64 {
	if len(sorted) == 0 {
		return 0
	}
	idx := (p * len(sorted)) / 100
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}

// extractBlocks extracts code blocks from a file for clone detection.
func (a *DuplicateAnalyzer) extractBlocks(path string) ([]fileBlock, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	lines := strings.Split(string(content), "\n")
	var blocks []fileBlock

	// Use sliding window to create blocks
	windowSize := a.minLines
	for start := 0; start <= len(lines)-windowSize; start++ {
		// Also try larger blocks
		for end := start + windowSize - 1; end < len(lines) && end < start+windowSize*3; end++ {
			blockLines := lines[start : end+1]
			blockContent := strings.Join(blockLines, "\n")

			// Skip mostly empty blocks
			if countNonEmptyLines(blockLines) < a.minLines {
				continue
			}

			blocks = append(blocks, fileBlock{
				file:      path,
				startLine: uint32(start + 1),
				endLine:   uint32(end + 1),
				content:   normalizeCode(blockContent),
			})
		}
	}

	return blocks, nil
}

// countNonEmptyLines counts lines that aren't empty or just whitespace.
func countNonEmptyLines(lines []string) int {
	count := 0
	for _, line := range lines {
		if strings.TrimSpace(line) != "" {
			count++
		}
	}
	return count
}

// normalizeCode normalizes code for comparison by removing whitespace variations.
func normalizeCode(code string) string {
	lines := strings.Split(code, "\n")
	var normalized []string
	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		if trimmed != "" && !isComment(trimmed) {
			normalized = append(normalized, trimmed)
		}
	}
	return strings.Join(normalized, "\n")
}

// isComment checks if a line is a comment (basic heuristic).
func isComment(line string) bool {
	return strings.HasPrefix(line, "//") ||
		strings.HasPrefix(line, "#") ||
		strings.HasPrefix(line, "/*") ||
		strings.HasPrefix(line, "*") ||
		strings.HasPrefix(line, "*/")
}

// computeMinHash computes a MinHash signature for a code block.
func (a *DuplicateAnalyzer) computeMinHash(content string) *models.MinHashSignature {
	// Generate shingles (n-grams of tokens)
	shingles := generateShingles(content, 3)

	// Initialize signature with max values
	signature := &models.MinHashSignature{
		Values: make([]uint64, a.numHashes),
	}
	for i := range signature.Values {
		signature.Values[i] = ^uint64(0)
	}

	// For each shingle, compute hashes and keep minimum
	for shingle := range shingles {
		for i := 0; i < a.numHashes; i++ {
			h := hashWithSeed(shingle, uint64(i))
			if h < signature.Values[i] {
				signature.Values[i] = h
			}
		}
	}

	return signature
}

// generateShingles creates n-grams from content.
func generateShingles(content string, n int) map[string]bool {
	tokens := tokenize(content)
	shingles := make(map[string]bool)

	if len(tokens) < n {
		if len(tokens) > 0 {
			shingles[strings.Join(tokens, " ")] = true
		}
		return shingles
	}

	for i := 0; i <= len(tokens)-n; i++ {
		shingle := strings.Join(tokens[i:i+n], " ")
		shingles[shingle] = true
	}

	return shingles
}

// tokenize splits code into tokens.
func tokenize(content string) []string {
	// Simple tokenization: split on whitespace and punctuation
	var tokens []string
	var current strings.Builder

	for _, c := range content {
		if isTokenChar(c) {
			current.WriteRune(c)
		} else {
			if current.Len() > 0 {
				tokens = append(tokens, current.String())
				current.Reset()
			}
			// Include punctuation as tokens
			if !isWhitespace(c) {
				tokens = append(tokens, string(c))
			}
		}
	}

	if current.Len() > 0 {
		tokens = append(tokens, current.String())
	}

	return tokens
}

func isTokenChar(c rune) bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
		(c >= '0' && c <= '9') || c == '_'
}

func isWhitespace(c rune) bool {
	return c == ' ' || c == '\t' || c == '\n' || c == '\r'
}

// hashWithSeed computes a hash with a seed for MinHash.
func hashWithSeed(s string, seed uint64) uint64 {
	h := fnv.New64a()
	h.Write([]byte{byte(seed), byte(seed >> 8), byte(seed >> 16), byte(seed >> 24)})
	h.Write([]byte(s))
	return h.Sum64()
}

// determineCloneType classifies a clone based on similarity.
func determineCloneType(similarity float64) models.CloneType {
	if similarity >= 0.95 {
		return models.CloneType1 // Exact (whitespace only)
	} else if similarity >= 0.85 {
		return models.CloneType2 // Parametric (identifiers differ)
	}
	return models.CloneType3 // Structural (statements differ)
}

// Close releases analyzer resources.
func (a *DuplicateAnalyzer) Close() {
	a.parser.Close()
}
