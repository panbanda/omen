package analyzer

import (
	"math"
	"os"
	"sort"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/cespare/xxhash/v2"
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
)

// DuplicateAnalyzer detects code clones using MinHash with identifier normalization.
type DuplicateAnalyzer struct {
	parser    *parser.Parser
	minLines  int
	threshold float64
	numHashes int

	// Identifier normalization state
	identifierCounter uint32
	identifierMap     sync.Map
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
		numHashes: 128,
	}
}

// fileBlock represents a chunk of code for clone detection.
type fileBlock struct {
	file           string
	startLine      uint32
	endLine        uint32
	content        string
	normalizedHash uint64
	signature      *models.MinHashSignature
	tokens         []string
}

// AnalyzeProject detects code clones across a project.
func (a *DuplicateAnalyzer) AnalyzeProject(files []string) (*models.CloneAnalysis, error) {
	return a.AnalyzeProjectWithProgress(files, nil)
}

// AnalyzeProjectWithProgress detects code clones with optional progress callback.
func (a *DuplicateAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress ProgressFunc) (*models.CloneAnalysis, error) {
	analysis := &models.CloneAnalysis{
		Clones:            make([]models.CodeClone, 0),
		Groups:            make([]models.CloneGroup, 0),
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
	totalLines := 0
	for _, blocks := range fileBlocks {
		allBlocks = append(allBlocks, blocks...)
		for _, b := range blocks {
			totalLines += int(b.endLine - b.startLine + 1)
		}
	}

	// Compute MinHash signatures for all blocks
	for i := range allBlocks {
		allBlocks[i].signature = a.computeMinHash(allBlocks[i].content)
		allBlocks[i].normalizedHash = a.computeNormalizedHash(allBlocks[i].tokens)
	}

	// Find clone pairs using threshold comparison
	clonePairs := a.findClonePairs(allBlocks)

	// Group clones using Union-Find
	groups := a.groupClones(allBlocks, clonePairs)
	analysis.Groups = groups
	analysis.Summary.TotalGroups = len(groups)

	// Convert groups to pairwise clones for backward compatibility
	for _, group := range groups {
		for i := 0; i < len(group.Instances); i++ {
			for j := i + 1; j < len(group.Instances); j++ {
				instA := group.Instances[i]
				instB := group.Instances[j]
				clone := models.CodeClone{
					Type:       group.Type,
					Similarity: group.AverageSimilarity,
					FileA:      instA.File,
					FileB:      instB.File,
					StartLineA: instA.StartLine,
					EndLineA:   instA.EndLine,
					StartLineB: instB.StartLine,
					EndLineB:   instB.EndLine,
					LinesA:     instA.Lines,
					LinesB:     instB.Lines,
					GroupID:    group.ID,
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

	// Calculate duplication ratio (capped at 1.0 since overlapping blocks can inflate the count)
	analysis.Summary.TotalLines = totalLines
	if totalLines > 0 {
		ratio := float64(analysis.Summary.DuplicatedLines) / float64(totalLines)
		if ratio > 1.0 {
			ratio = 1.0
		}
		analysis.Summary.DuplicationRatio = ratio
	}

	// Compute hotspots
	analysis.Summary.Hotspots = a.computeHotspots(groups)

	return analysis, nil
}

// findClonePairs finds pairs of blocks that exceed the similarity threshold.
func (a *DuplicateAnalyzer) findClonePairs(blocks []fileBlock) []clonePair {
	var pairs []clonePair

	for i := 0; i < len(blocks); i++ {
		for j := i + 1; j < len(blocks); j++ {
			blockA := blocks[i]
			blockB := blocks[j]

			// Skip if same file and overlapping
			if blockA.file == blockB.file {
				if blockA.startLine <= blockB.endLine && blockB.startLine <= blockA.endLine {
					continue
				}
			}

			// Calculate similarity
			similarity := blockA.signature.JaccardSimilarity(blockB.signature)
			if similarity >= a.threshold {
				pairs = append(pairs, clonePair{
					idxA:       i,
					idxB:       j,
					similarity: similarity,
				})
			}
		}
	}

	return pairs
}

type clonePair struct {
	idxA       int
	idxB       int
	similarity float64
}

// groupClones groups clone pairs using Union-Find algorithm.
func (a *DuplicateAnalyzer) groupClones(blocks []fileBlock, pairs []clonePair) []models.CloneGroup {
	if len(pairs) == 0 {
		return nil
	}

	// Initialize Union-Find
	parent := make([]int, len(blocks))
	for i := range parent {
		parent[i] = i
	}

	var find func(int) int
	find = func(x int) int {
		if parent[x] != x {
			parent[x] = find(parent[x])
		}
		return parent[x]
	}

	union := func(x, y int) {
		px, py := find(x), find(y)
		if px != py {
			parent[px] = py
		}
	}

	// Union all clone pairs
	for _, pair := range pairs {
		union(pair.idxA, pair.idxB)
	}

	// Group blocks by their root
	groupMap := make(map[int][]int)
	for i := range blocks {
		root := find(i)
		groupMap[root] = append(groupMap[root], i)
	}

	// Build similarity map for pairs
	similarityMap := make(map[[2]int]float64)
	for _, pair := range pairs {
		key := [2]int{pair.idxA, pair.idxB}
		if pair.idxA > pair.idxB {
			key = [2]int{pair.idxB, pair.idxA}
		}
		similarityMap[key] = pair.similarity
	}

	// Convert to CloneGroup
	var groups []models.CloneGroup
	var groupID uint64

	for _, memberIndices := range groupMap {
		if len(memberIndices) < 2 {
			continue
		}

		groupID++
		var instances []models.CloneInstance
		var totalLines, totalTokens int
		var similaritySum float64
		var similarityCount int

		for _, idx := range memberIndices {
			block := blocks[idx]
			lines := int(block.endLine - block.startLine + 1)
			instances = append(instances, models.CloneInstance{
				File:           block.file,
				StartLine:      block.startLine,
				EndLine:        block.endLine,
				Lines:          lines,
				NormalizedHash: block.normalizedHash,
				Similarity:     1.0,
			})
			totalLines += lines
			totalTokens += len(block.tokens)
		}

		// Calculate average similarity
		for i := 0; i < len(memberIndices); i++ {
			for j := i + 1; j < len(memberIndices); j++ {
				key := [2]int{memberIndices[i], memberIndices[j]}
				if memberIndices[i] > memberIndices[j] {
					key = [2]int{memberIndices[j], memberIndices[i]}
				}
				if sim, ok := similarityMap[key]; ok {
					similaritySum += sim
					similarityCount++
				}
			}
		}

		avgSimilarity := 1.0
		if similarityCount > 0 {
			avgSimilarity = similaritySum / float64(similarityCount)
		}

		groups = append(groups, models.CloneGroup{
			ID:                groupID,
			Type:              determineCloneType(avgSimilarity),
			Instances:         instances,
			TotalLines:        totalLines,
			TotalTokens:       totalTokens,
			AverageSimilarity: avgSimilarity,
		})
	}

	return groups
}

// computeHotspots identifies files with high duplication.
func (a *DuplicateAnalyzer) computeHotspots(groups []models.CloneGroup) []models.DuplicationHotspot {
	fileStats := make(map[string]struct {
		lines     int
		groupsSet map[uint64]bool
	})

	for _, group := range groups {
		for _, inst := range group.Instances {
			stats, ok := fileStats[inst.File]
			if !ok {
				stats = struct {
					lines     int
					groupsSet map[uint64]bool
				}{groupsSet: make(map[uint64]bool)}
			}
			stats.lines += inst.Lines
			stats.groupsSet[group.ID] = true
			fileStats[inst.File] = stats
		}
	}

	var hotspots []models.DuplicationHotspot
	for file, stats := range fileStats {
		severity := math.Log(float64(stats.lines)+1) * math.Sqrt(float64(len(stats.groupsSet)))
		hotspots = append(hotspots, models.DuplicationHotspot{
			File:            file,
			DuplicateLines:  stats.lines,
			CloneGroupCount: len(stats.groupsSet),
			Severity:        severity,
		})
	}

	sort.Slice(hotspots, func(i, j int) bool {
		return hotspots[i].Severity > hotspots[j].Severity
	})

	if len(hotspots) > 10 {
		hotspots = hotspots[:10]
	}

	return hotspots
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

			normalizedContent := normalizeCode(blockContent)
			tokens := tokenize(normalizedContent)
			normalizedTokens := a.normalizeTokens(tokens)

			blocks = append(blocks, fileBlock{
				file:      path,
				startLine: uint32(start + 1),
				endLine:   uint32(end + 1),
				content:   strings.Join(normalizedTokens, " "),
				tokens:    normalizedTokens,
			})
		}
	}

	return blocks, nil
}

// normalizeTokens applies identifier and literal normalization.
func (a *DuplicateAnalyzer) normalizeTokens(tokens []string) []string {
	result := make([]string, 0, len(tokens))
	for _, token := range tokens {
		normalized := a.normalizeToken(token)
		if normalized != "" {
			result = append(result, normalized)
		}
	}
	return result
}

// normalizeToken normalizes a single token.
func (a *DuplicateAnalyzer) normalizeToken(token string) string {
	// Skip empty tokens
	if token == "" {
		return ""
	}

	// Check if it's a keyword (don't normalize)
	if isKeyword(token) {
		return token
	}

	// Check if it's a literal (number or string)
	if isLiteral(token) {
		return "LITERAL"
	}

	// Check if it's an operator or delimiter (don't normalize)
	if isOperatorOrDelimiter(token) {
		return token
	}

	// It's an identifier - canonicalize it
	return a.canonicalizeIdentifier(token)
}

// canonicalizeIdentifier maps identifiers to canonical names (VAR_N).
func (a *DuplicateAnalyzer) canonicalizeIdentifier(name string) string {
	if canonical, ok := a.identifierMap.Load(name); ok {
		return canonical.(string)
	}

	id := atomic.AddUint32(&a.identifierCounter, 1)
	canonical := "VAR_" + itoa(int(id))
	a.identifierMap.Store(name, canonical)
	return canonical
}

// itoa converts an int to string without fmt dependency.
func itoa(i int) string {
	if i == 0 {
		return "0"
	}
	var buf [20]byte
	pos := len(buf)
	for i > 0 {
		pos--
		buf[pos] = byte('0' + i%10)
		i /= 10
	}
	return string(buf[pos:])
}

// isKeyword checks if a token is a programming language keyword.
func isKeyword(token string) bool {
	keywords := map[string]bool{
		// Go
		"func": true, "return": true, "if": true, "else": true, "for": true,
		"range": true, "switch": true, "case": true, "default": true, "break": true,
		"continue": true, "goto": true, "fallthrough": true, "defer": true,
		"go": true, "select": true, "chan": true, "map": true, "struct": true,
		"interface": true, "type": true, "var": true, "const": true, "package": true,
		"import": true, "nil": true, "true": true, "false": true,
		// Rust
		"fn": true, "let": true, "mut": true, "match": true, "loop": true,
		"while": true, "impl": true, "trait": true, "mod": true, "use": true,
		"pub": true, "crate": true, "self": true, "Self": true, "where": true,
		"async": true, "await": true, "static": true, "extern": true, "unsafe": true,
		"enum": true, "move": true, "ref": true, "as": true, "in": true,
		// Python
		"def": true, "class": true, "elif": true, "try": true, "except": true,
		"finally": true, "with": true, "lambda": true, "yield": true, "assert": true,
		"raise": true, "pass": true, "del": true, "global": true, "nonlocal": true,
		"and": true, "or": true, "not": true, "is": true, "from": true,
		// JavaScript/TypeScript (only unique keywords not in Go/Rust/Python)
		"function": true, "new": true, "this": true, "super": true,
		"extends": true, "implements": true, "export": true, "throw": true,
		"catch": true, "instanceof": true, "typeof": true, "void": true,
		"delete": true, "debugger": true,
		// Common
		"null": true, "undefined": true,
	}
	return keywords[token]
}

// isLiteral checks if a token is a literal value.
func isLiteral(token string) bool {
	if len(token) == 0 {
		return false
	}

	// String literal
	if (token[0] == '"' || token[0] == '\'' || token[0] == '`') {
		return true
	}

	// Number literal
	if token[0] >= '0' && token[0] <= '9' {
		return true
	}

	// Negative number
	if len(token) > 1 && token[0] == '-' && token[1] >= '0' && token[1] <= '9' {
		return true
	}

	return false
}

// isOperatorOrDelimiter checks if a token is an operator or delimiter.
func isOperatorOrDelimiter(token string) bool {
	operators := map[string]bool{
		"+": true, "-": true, "*": true, "/": true, "%": true,
		"=": true, "==": true, "!=": true, "<": true, ">": true,
		"<=": true, ">=": true, "&&": true, "||": true, "!": true,
		"&": true, "|": true, "^": true, "<<": true, ">>": true,
		"+=": true, "-=": true, "*=": true, "/=": true, "%=": true,
		"&=": true, "|=": true, "^=": true, "<<=": true, ">>=": true,
		"++": true, "--": true, "->": true, "=>": true, "::": true,
		"..": true, "...": true, "?": true, ":": true,
		"(": true, ")": true, "[": true, "]": true, "{": true, "}": true,
		",": true, ";": true, ".": true,
	}
	return operators[token]
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

// computeMinHash computes a MinHash signature for a code block using xxhash.
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

// computeNormalizedHash computes a hash of the normalized token sequence.
func (a *DuplicateAnalyzer) computeNormalizedHash(tokens []string) uint64 {
	content := strings.Join(tokens, " ")
	return xxhash.Sum64String(content)
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

// hashWithSeed computes a hash with a seed for MinHash using xxhash.
func hashWithSeed(s string, seed uint64) uint64 {
	// Combine seed with string using xxhash
	h := xxhash.New()
	seedBytes := []byte{byte(seed), byte(seed >> 8), byte(seed >> 16), byte(seed >> 24),
		byte(seed >> 32), byte(seed >> 40), byte(seed >> 48), byte(seed >> 56)}
	h.Write(seedBytes)
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
