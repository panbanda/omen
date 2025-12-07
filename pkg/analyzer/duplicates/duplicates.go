package duplicates

import (
	"encoding/binary"
	"math"
	"os"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/cespare/xxhash/v2"
	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/parser"
	"github.com/panbanda/omen/pkg/stats"
	"github.com/zeebo/blake3"
)

// Analyzer detects code clones using MinHash with LSH for efficient candidate filtering.
type Analyzer struct {
	parser      *parser.Parser
	config      Config
	maxFileSize int64

	// Identifier normalization state
	identifierCounter uint32
	identifierMap     sync.Map
}

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithMinTokens sets the minimum number of tokens for a code fragment.
func WithMinTokens(minTokens int) Option {
	return func(a *Analyzer) {
		a.config.MinTokens = minTokens
	}
}

// WithSimilarityThreshold sets the similarity threshold for clone detection.
func WithSimilarityThreshold(threshold float64) Option {
	return func(a *Analyzer) {
		a.config.SimilarityThreshold = threshold
	}
}

// WithConfig sets all duplicate configuration from a config struct.
func WithConfig(cfg config.DuplicateConfig) Option {
	return func(a *Analyzer) {
		a.config = Config{
			MinTokens:            cfg.MinTokens,
			SimilarityThreshold:  cfg.SimilarityThreshold,
			ShingleSize:          cfg.ShingleSize,
			NumHashFunctions:     cfg.NumHashFunctions,
			NumBands:             cfg.NumBands,
			RowsPerBand:          cfg.RowsPerBand,
			NormalizeIdentifiers: cfg.NormalizeIdentifiers,
			NormalizeLiterals:    cfg.NormalizeLiterals,
			IgnoreComments:       cfg.IgnoreComments,
			MinGroupSize:         cfg.MinGroupSize,
		}
	}
}

// WithMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithMaxFileSize(maxSize int64) Option {
	return func(a *Analyzer) {
		a.maxFileSize = maxSize
	}
}

// New creates a new duplicate analyzer with default config.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		parser:      parser.New(),
		config:      DefaultConfig(),
		maxFileSize: 0,
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// codeFragment represents a code fragment for clone detection.
type codeFragment struct {
	id             uint64
	file           string
	startLine      uint32
	endLine        uint32
	content        string
	normalizedHash uint64
	signature      *MinHashSignature
	tokens         []string
}

// AnalyzeProject detects code clones across a project.
func (a *Analyzer) AnalyzeProject(files []string) (*Analysis, error) {
	return a.AnalyzeProjectWithProgress(files, nil)
}

// AnalyzeProjectWithProgress detects code clones with optional progress callback.
func (a *Analyzer) AnalyzeProjectWithProgress(files []string, onProgress fileproc.ProgressFunc) (*Analysis, error) {
	analysis := &Analysis{
		Clones:            make([]Clone, 0),
		Groups:            make([]Group, 0),
		Summary:           NewSummary(),
		TotalFilesScanned: len(files),
		MinLines:          a.config.MinTokens / 8, // Approximate lines from tokens
		Threshold:         a.config.SimilarityThreshold,
	}

	// Extract fragments from all files in parallel
	fileFragments := fileproc.ForEachFileWithProgress(files, func(path string) ([]codeFragment, error) {
		// Check file size before reading
		if a.maxFileSize > 0 {
			info, err := os.Stat(path)
			if err != nil {
				return nil, err
			}
			if info.Size() > a.maxFileSize {
				return nil, nil // Skip file if too large
			}
		}
		return a.extractFragments(path)
	}, onProgress)

	var allFragments []codeFragment
	var fragmentID uint64
	totalLines := 0
	for _, fragments := range fileFragments {
		for _, f := range fragments {
			f.id = fragmentID
			fragmentID++
			allFragments = append(allFragments, f)
			totalLines += int(f.endLine - f.startLine + 1)
		}
	}

	// Compute MinHash signatures for all fragments
	for i := range allFragments {
		allFragments[i].signature = a.computeMinHash(allFragments[i].tokens)
		allFragments[i].normalizedHash = a.computeNormalizedHash(allFragments[i].tokens)
	}

	// Find clone pairs using LSH for O(n) average-case candidate filtering
	clonePairs := a.findClonePairsLSH(allFragments)

	// Group clones using Union-Find
	groups := a.groupClones(allFragments, clonePairs)
	analysis.Groups = groups
	analysis.Summary.TotalGroups = len(groups)

	// Convert groups to pairwise clones for backward compatibility
	for _, group := range groups {
		for i := 0; i < len(group.Instances); i++ {
			for j := i + 1; j < len(group.Instances); j++ {
				instA := group.Instances[i]
				instB := group.Instances[j]
				clone := Clone{
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
		analysis.Summary.P50Similarity = stats.Percentile(similarities, 50)
		analysis.Summary.P95Similarity = stats.Percentile(similarities, 95)
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

// findClonePairsLSH uses Locality-Sensitive Hashing for O(n) average-case candidate filtering.
func (a *Analyzer) findClonePairsLSH(fragments []codeFragment) []clonePair {
	bands := a.config.NumBands
	rowsPerBand := a.config.RowsPerBand

	// Create LSH buckets for each band
	lshBuckets := make([]map[uint64][]int, bands)
	for i := range lshBuckets {
		lshBuckets[i] = make(map[uint64][]int)
	}

	// Hash each fragment into buckets
	for idx, fragment := range fragments {
		if fragment.signature == nil || len(fragment.signature.Values) == 0 {
			continue
		}
		for band := 0; band < bands; band++ {
			start := band * rowsPerBand
			end := start + rowsPerBand
			if end > len(fragment.signature.Values) {
				end = len(fragment.signature.Values)
			}
			if start >= end {
				continue
			}

			// Hash the band portion of the signature
			bandHash := hashBand(fragment.signature.Values[start:end], uint64(band))
			lshBuckets[band][bandHash] = append(lshBuckets[band][bandHash], idx)
		}
	}

	// Find candidate pairs from buckets (pairs that hash to the same bucket in any band)
	candidatePairs := make(map[uint64]struct{})
	for _, bandBuckets := range lshBuckets {
		for _, bucket := range bandBuckets {
			if len(bucket) < 2 {
				continue
			}
			// Add all pairs from this bucket as candidates
			for i := 0; i < len(bucket); i++ {
				for j := i + 1; j < len(bucket); j++ {
					idxA, idxB := bucket[i], bucket[j]
					if idxA > idxB {
						idxA, idxB = idxB, idxA
					}
					pairKey := uint64(idxA)<<32 | uint64(idxB)
					candidatePairs[pairKey] = struct{}{}
				}
			}
		}
	}

	// Verify candidate pairs with actual Jaccard similarity calculation
	var pairs []clonePair
	for pairKey := range candidatePairs {
		idxA := int(pairKey >> 32)
		idxB := int(pairKey & 0xFFFFFFFF)
		fragA := fragments[idxA]
		fragB := fragments[idxB]

		// Skip if same file and overlapping
		if fragA.file == fragB.file {
			if fragA.startLine <= fragB.endLine && fragB.startLine <= fragA.endLine {
				continue
			}
		}

		// Calculate actual similarity
		similarity := fragA.signature.JaccardSimilarity(fragB.signature)
		if similarity >= a.config.SimilarityThreshold {
			pairs = append(pairs, clonePair{
				idxA:       idxA,
				idxB:       idxB,
				similarity: similarity,
			})
		}
	}

	return pairs
}

// hashBand computes a hash for a band portion of the signature.
// Uses FNV-1a style combining without allocations.
func hashBand(values []uint64, seed uint64) uint64 {
	const fnvPrime = 0x00000100000001B3
	h := seed ^ 0xcbf29ce484222325 // FNV offset basis
	for _, v := range values {
		h ^= v
		h *= fnvPrime
	}
	return h
}

type clonePair struct {
	idxA       int
	idxB       int
	similarity float64
}

// groupClones groups clone pairs using Union-Find algorithm.
func (a *Analyzer) groupClones(fragments []codeFragment, pairs []clonePair) []Group {
	if len(pairs) == 0 {
		return nil
	}

	// Initialize Union-Find
	parent := make([]int, len(fragments))
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

	// Group fragments by their root
	groupMap := make(map[int][]int)
	for i := range fragments {
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

	// Convert to Group
	var groups []Group
	var groupID uint64

	for _, memberIndices := range groupMap {
		if len(memberIndices) < a.config.MinGroupSize {
			continue
		}

		groupID++
		var instances []Instance
		var totalLines, totalTokens int
		var similaritySum float64
		var similarityCount int

		for _, idx := range memberIndices {
			frag := fragments[idx]
			lines := int(frag.endLine - frag.startLine + 1)
			instances = append(instances, Instance{
				File:           frag.file,
				StartLine:      frag.startLine,
				EndLine:        frag.endLine,
				Lines:          lines,
				NormalizedHash: frag.normalizedHash,
				Similarity:     1.0,
			})
			totalLines += lines
			totalTokens += len(frag.tokens)
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

		groups = append(groups, Group{
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
func (a *Analyzer) computeHotspots(groups []Group) []Hotspot {
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

	var hotspots []Hotspot
	for file, stats := range fileStats {
		severity := math.Log(float64(stats.lines)+1) * math.Sqrt(float64(len(stats.groupsSet)))
		hotspots = append(hotspots, Hotspot{
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

// extractFragments extracts code fragments from a file for clone detection.
// Uses function-level extraction when possible, falling back to whole file (pmat-compatible).
func (a *Analyzer) extractFragments(path string) ([]codeFragment, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	return a.extractFragmentsFromContent(path, content), nil
}

// extractFragmentsFromContent extracts fragments from in-memory content.
func (a *Analyzer) extractFragmentsFromContent(path string, content []byte) []codeFragment {
	lines := strings.Split(string(content), "\n")
	var fragments []codeFragment

	// Try function-level extraction first
	funcFragments := a.extractFunctionFragments(path, lines)
	if len(funcFragments) > 0 {
		fragments = append(fragments, funcFragments...)
	}

	// If no function fragments, fall back to whole file as single fragment (pmat-compatible)
	if len(fragments) == 0 {
		frag := a.createFragment(path, 0, len(lines)-1, lines)
		if frag != nil {
			fragments = append(fragments, *frag)
		}
	}

	return fragments
}

// extractFunctionFragments extracts function-level code fragments.
func (a *Analyzer) extractFunctionFragments(path string, lines []string) []codeFragment {
	var fragments []codeFragment
	lang := detectLanguage(path)

	inFunction := false
	var funcStartLine int
	var funcLines []string
	braceDepth := 0

	for i, line := range lines {
		trimmed := strings.TrimSpace(line)

		if !inFunction {
			if isFunctionStart(trimmed, lang) {
				inFunction = true
				funcStartLine = i
				funcLines = []string{line}
				braceDepth = strings.Count(line, "{") - strings.Count(line, "}")
				if lang == "python" {
					braceDepth = 1 // Python uses indentation, not braces
				}
			}
		} else {
			funcLines = append(funcLines, line)
			if lang == "python" {
				// Python function ends at dedent or new def/class
				if trimmed != "" && !strings.HasPrefix(line, " ") && !strings.HasPrefix(line, "\t") {
					if strings.HasPrefix(trimmed, "def ") || strings.HasPrefix(trimmed, "class ") || i == len(lines)-1 {
						// End of function
						frag := a.createFragment(path, funcStartLine, i-1, funcLines[:len(funcLines)-1])
						if frag != nil {
							fragments = append(fragments, *frag)
						}
						// Check if this line starts a new function
						if isFunctionStart(trimmed, lang) {
							funcStartLine = i
							funcLines = []string{line}
						} else {
							inFunction = false
						}
					}
				}
			} else {
				braceDepth += strings.Count(line, "{") - strings.Count(line, "}")
				if braceDepth <= 0 {
					// End of function
					frag := a.createFragment(path, funcStartLine, i, funcLines)
					if frag != nil {
						fragments = append(fragments, *frag)
					}
					inFunction = false
					funcLines = nil
				}
			}
		}
	}

	// Handle unclosed function at end of file
	if inFunction && len(funcLines) > 0 {
		frag := a.createFragment(path, funcStartLine, len(lines)-1, funcLines)
		if frag != nil {
			fragments = append(fragments, *frag)
		}
	}

	return fragments
}

// createFragment creates a code fragment from lines if it meets minimum token requirements.
func (a *Analyzer) createFragment(path string, startLine, endLine int, lines []string) *codeFragment {
	content := strings.Join(lines, "\n")

	// Normalize and tokenize
	normalizedContent := a.normalizeCode(content)
	tokens := tokenize(normalizedContent)
	normalizedTokens := a.normalizeTokens(tokens)

	// Check minimum token count
	if len(normalizedTokens) < a.config.MinTokens {
		return nil
	}

	return &codeFragment{
		file:      path,
		startLine: uint32(startLine + 1),
		endLine:   uint32(endLine + 1),
		content:   strings.Join(normalizedTokens, " "),
		tokens:    normalizedTokens,
	}
}

// normalizeCode normalizes code for comparison.
func (a *Analyzer) normalizeCode(code string) string {
	lines := strings.Split(code, "\n")
	var normalized []string
	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		if trimmed == "" {
			continue
		}
		if a.config.IgnoreComments && isComment(trimmed) {
			continue
		}
		normalized = append(normalized, trimmed)
	}
	return strings.Join(normalized, "\n")
}

// normalizeTokens applies identifier and literal normalization.
func (a *Analyzer) normalizeTokens(tokens []string) []string {
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
func (a *Analyzer) normalizeToken(token string) string {
	if token == "" {
		return ""
	}

	// Check if it's a keyword (don't normalize)
	if isKeyword(token) {
		return token
	}

	// Check if it's a literal
	if isLiteral(token) {
		if a.config.NormalizeLiterals {
			return "LITERAL"
		}
		return token
	}

	// Check if it's an operator or delimiter (don't normalize)
	if isOperatorOrDelimiter(token) {
		return token
	}

	// It's an identifier
	if a.config.NormalizeIdentifiers {
		return a.canonicalizeIdentifier(token)
	}
	return token
}

// canonicalizeIdentifier maps identifiers to canonical names (VAR_N).
func (a *Analyzer) canonicalizeIdentifier(name string) string {
	// Use LoadOrStore to avoid race where two goroutines both see the key
	// as missing and assign different IDs to the same identifier.
	id := atomic.AddUint32(&a.identifierCounter, 1)
	canonical := "VAR_" + strconv.FormatUint(uint64(id), 10)
	if actual, loaded := a.identifierMap.LoadOrStore(name, canonical); loaded {
		return actual.(string)
	}
	return canonical
}

// keywords is a pre-allocated set of programming language keywords.
var keywords = map[string]bool{
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
	// JavaScript/TypeScript
	"function": true, "new": true, "this": true, "super": true,
	"extends": true, "implements": true, "export": true, "throw": true,
	"catch": true, "instanceof": true, "typeof": true, "void": true,
	"delete": true, "debugger": true,
	// Common
	"null": true, "undefined": true,
}

// isKeyword checks if a token is a programming language keyword.
func isKeyword(token string) bool {
	return keywords[token]
}

// isLiteral checks if a token is a literal value.
func isLiteral(token string) bool {
	if len(token) == 0 {
		return false
	}

	// String literal
	if token[0] == '"' || token[0] == '\'' || token[0] == '`' {
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

// operators is a pre-allocated set of operators and delimiters.
var operators = map[string]bool{
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

// isOperatorOrDelimiter checks if a token is an operator or delimiter.
func isOperatorOrDelimiter(token string) bool {
	return operators[token]
}

// isComment checks if a line is a comment.
func isComment(line string) bool {
	return strings.HasPrefix(line, "//") ||
		strings.HasPrefix(line, "#") ||
		strings.HasPrefix(line, "/*") ||
		strings.HasPrefix(line, "*") ||
		strings.HasPrefix(line, "*/")
}

// computeMinHash computes a MinHash signature using k-shingles (pmat-compatible).
// Uses blake3-hashed shingles and xxhash with seeds for MinHash.
func (a *Analyzer) computeMinHash(tokens []string) *MinHashSignature {
	// Generate k-shingles as uint64 hashes (blake3)
	shingles := generateKShingles(tokens, a.config.ShingleSize)

	// Initialize signature with max values
	signature := &MinHashSignature{
		Values: make([]uint64, a.config.NumHashFunctions),
	}
	for i := range signature.Values {
		signature.Values[i] = ^uint64(0)
	}

	// For each shingle hash, compute MinHash values with different seeds
	for _, shingleHash := range shingles {
		for i := 0; i < a.config.NumHashFunctions; i++ {
			h := hashUint64WithSeed(shingleHash, uint64(i))
			if h < signature.Values[i] {
				signature.Values[i] = h
			}
		}
	}

	return signature
}

// hashUint64WithSeed computes a hash of a uint64 value with a seed.
// Uses bit mixing instead of xxhash to avoid allocations.
func hashUint64WithSeed(value uint64, seed uint64) uint64 {
	// Combine value and seed using murmur-style mixing
	h := value ^ seed
	h ^= h >> 33
	h *= 0xff51afd7ed558ccd
	h ^= h >> 33
	h *= 0xc4ceb9fe1a85ec53
	h ^= h >> 33
	return h
}

// generateKShingles creates k-shingles from tokens using blake3 hashing (pmat-compatible).
// Returns a set of uint64 hashes instead of strings for efficiency.
func generateKShingles(tokens []string, k int) []uint64 {
	if len(tokens) < k {
		if len(tokens) > 0 {
			// Hash the entire token sequence if fewer than k tokens
			h := blake3.New()
			for _, t := range tokens {
				h.Write([]byte(t))
			}
			sum := h.Sum(nil)
			return []uint64{binary.LittleEndian.Uint64(sum[:8])}
		}
		return nil
	}

	shingleSet := make(map[uint64]struct{})
	h := blake3.New()

	for i := 0; i <= len(tokens)-k; i++ {
		h.Reset()
		for j := i; j < i+k; j++ {
			h.Write([]byte(tokens[j]))
		}
		sum := h.Sum(nil)
		hash := binary.LittleEndian.Uint64(sum[:8])
		shingleSet[hash] = struct{}{}
	}

	shingles := make([]uint64, 0, len(shingleSet))
	for hash := range shingleSet {
		shingles = append(shingles, hash)
	}

	return shingles
}

// computeNormalizedHash computes a hash of the normalized token sequence.
func (a *Analyzer) computeNormalizedHash(tokens []string) uint64 {
	content := strings.Join(tokens, " ")
	return xxhash.Sum64String(content)
}

// tokenize splits code into tokens (pmat-compatible).
// Handles string literals, numbers, identifiers, operators, and delimiters.
func tokenize(content string) []string {
	var tokens []string
	runes := []rune(content)
	i := 0

	for i < len(runes) {
		c := runes[i]

		// Skip whitespace
		if isWhitespace(c) {
			i++
			continue
		}

		// String literals (double quotes)
		if c == '"' {
			literal := collectStringLiteral(runes, &i, '"')
			tokens = append(tokens, literal)
			continue
		}

		// String literals (single quotes)
		if c == '\'' {
			literal := collectStringLiteral(runes, &i, '\'')
			tokens = append(tokens, literal)
			continue
		}

		// Template literals (backtick)
		if c == '`' {
			literal := collectStringLiteral(runes, &i, '`')
			tokens = append(tokens, literal)
			continue
		}

		// Numbers
		if isDigit(c) || (c == '-' && i+1 < len(runes) && isDigit(runes[i+1])) {
			number := collectNumber(runes, &i)
			tokens = append(tokens, number)
			continue
		}

		// Identifiers and keywords
		if isIdentifierStart(c) {
			ident := collectIdentifier(runes, &i)
			tokens = append(tokens, ident)
			continue
		}

		// Multi-character operators
		if op := collectOperator(runes, &i); op != "" {
			tokens = append(tokens, op)
			continue
		}

		// Single character (delimiter or unknown)
		tokens = append(tokens, string(c))
		i++
	}

	return tokens
}

// collectStringLiteral collects a string literal including quotes.
func collectStringLiteral(runes []rune, i *int, quote rune) string {
	var sb strings.Builder
	sb.WriteRune(runes[*i])
	*i++

	for *i < len(runes) {
		c := runes[*i]
		sb.WriteRune(c)
		*i++

		if c == quote {
			break
		}
		// Handle escape sequences
		if c == '\\' && *i < len(runes) {
			sb.WriteRune(runes[*i])
			*i++
		}
	}

	return sb.String()
}

// collectNumber collects a numeric literal.
func collectNumber(runes []rune, i *int) string {
	var sb strings.Builder

	// Handle negative sign
	if runes[*i] == '-' {
		sb.WriteRune('-')
		*i++
	}

	for *i < len(runes) {
		c := runes[*i]
		if isDigit(c) || c == '.' || c == '_' || c == 'x' || c == 'X' ||
			c == 'b' || c == 'B' || c == 'o' || c == 'O' ||
			(c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F') ||
			c == 'e' || c == 'E' {
			sb.WriteRune(c)
			*i++
		} else {
			break
		}
	}

	return sb.String()
}

// collectIdentifier collects an identifier.
func collectIdentifier(runes []rune, i *int) string {
	var sb strings.Builder

	for *i < len(runes) {
		c := runes[*i]
		if isIdentifierChar(c) {
			sb.WriteRune(c)
			*i++
		} else {
			break
		}
	}

	return sb.String()
}

// collectOperator collects multi-character operators.
func collectOperator(runes []rune, i *int) string {
	if *i >= len(runes) {
		return ""
	}

	// Try 3-character operators
	if *i+2 < len(runes) {
		op3 := string(runes[*i : *i+3])
		if op3 == "<<=" || op3 == ">>=" || op3 == "..." || op3 == "===" || op3 == "!==" {
			*i += 3
			return op3
		}
	}

	// Try 2-character operators
	if *i+1 < len(runes) {
		op2 := string(runes[*i : *i+2])
		switch op2 {
		case "==", "!=", "<=", ">=", "&&", "||", "<<", ">>",
			"+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
			"++", "--", "->", "=>", "::", "..", "??":
			*i += 2
			return op2
		}
	}

	return ""
}

func isDigit(c rune) bool {
	return c >= '0' && c <= '9'
}

func isIdentifierStart(c rune) bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_'
}

func isIdentifierChar(c rune) bool {
	return isIdentifierStart(c) || isDigit(c)
}

func isWhitespace(c rune) bool {
	return c == ' ' || c == '\t' || c == '\n' || c == '\r'
}

// determineCloneType classifies a clone based on similarity.
func determineCloneType(similarity float64) Type {
	if similarity >= 0.95 {
		return Type1 // Exact (whitespace only)
	} else if similarity >= 0.85 {
		return Type2 // Parametric (identifiers differ)
	}
	return Type3 // Structural (statements differ)
}

// detectLanguage detects programming language from file extension.
func detectLanguage(path string) string {
	path = strings.ToLower(path)
	switch {
	case strings.HasSuffix(path, ".go"):
		return "go"
	case strings.HasSuffix(path, ".rs"):
		return "rust"
	case strings.HasSuffix(path, ".py"):
		return "python"
	case strings.HasSuffix(path, ".ts"), strings.HasSuffix(path, ".tsx"):
		return "typescript"
	case strings.HasSuffix(path, ".js"), strings.HasSuffix(path, ".jsx"):
		return "javascript"
	case strings.HasSuffix(path, ".c"), strings.HasSuffix(path, ".h"):
		return "c"
	case strings.HasSuffix(path, ".cpp"), strings.HasSuffix(path, ".hpp"),
		strings.HasSuffix(path, ".cc"), strings.HasSuffix(path, ".cxx"):
		return "cpp"
	case strings.HasSuffix(path, ".java"):
		return "java"
	case strings.HasSuffix(path, ".kt"), strings.HasSuffix(path, ".kts"):
		return "kotlin"
	case strings.HasSuffix(path, ".rb"):
		return "ruby"
	case strings.HasSuffix(path, ".php"):
		return "php"
	default:
		return "unknown"
	}
}

// isFunctionStart checks if a line starts a function definition.
func isFunctionStart(line, lang string) bool {
	switch lang {
	case "go":
		return strings.HasPrefix(line, "func ") && strings.Contains(line, "(")
	case "rust":
		return strings.Contains(line, "fn ") && strings.Contains(line, "(")
	case "python":
		return strings.HasPrefix(line, "def ") && strings.Contains(line, "(")
	case "typescript", "javascript":
		return strings.Contains(line, "function ") ||
			strings.Contains(line, "=> {") ||
			(strings.Contains(line, "(") && strings.Contains(line, ") {"))
	case "c", "cpp":
		return strings.Contains(line, "(") &&
			(strings.Contains(line, ") {") || strings.HasSuffix(line, "{"))
	case "java", "kotlin":
		return (strings.Contains(line, "void ") ||
			strings.Contains(line, "int ") ||
			strings.Contains(line, "String ") ||
			strings.Contains(line, "fun ") ||
			strings.Contains(line, "public ") ||
			strings.Contains(line, "private ") ||
			strings.Contains(line, "protected ")) &&
			strings.Contains(line, "(")
	default:
		return false
	}
}

// ContentSource provides file content.
type ContentSource interface {
	Read(path string) ([]byte, error)
}

// AnalyzeProjectFromSource detects clones from files read via ContentSource.
func (a *Analyzer) AnalyzeProjectFromSource(files []string, src ContentSource) (*Analysis, error) {
	analysis := &Analysis{
		Clones:            make([]Clone, 0),
		Groups:            make([]Group, 0),
		Summary:           NewSummary(),
		TotalFilesScanned: len(files),
		MinLines:          a.config.MinTokens / 8,
		Threshold:         a.config.SimilarityThreshold,
	}

	// Read all files and extract fragments
	var allFragments []codeFragment
	var fragmentID uint64
	totalLines := 0

	for _, path := range files {
		content, err := src.Read(path)
		if err != nil {
			continue
		}
		if a.maxFileSize > 0 && int64(len(content)) > a.maxFileSize {
			continue
		}

		fragments := a.extractFragmentsFromContent(path, content)
		for _, f := range fragments {
			f.id = fragmentID
			fragmentID++
			allFragments = append(allFragments, f)
			totalLines += int(f.endLine - f.startLine + 1)
		}
	}

	// Compute MinHash signatures for all fragments
	for i := range allFragments {
		allFragments[i].signature = a.computeMinHash(allFragments[i].tokens)
		allFragments[i].normalizedHash = a.computeNormalizedHash(allFragments[i].tokens)
	}

	// Find clone pairs using LSH
	clonePairs := a.findClonePairsLSH(allFragments)

	// Group clones using Union-Find
	groups := a.groupClones(allFragments, clonePairs)
	analysis.Groups = groups
	analysis.Summary.TotalGroups = len(groups)

	// Convert groups to pairwise clones for backward compatibility
	for _, group := range groups {
		for i := 0; i < len(group.Instances); i++ {
			for j := i + 1; j < len(group.Instances); j++ {
				instA := group.Instances[i]
				instB := group.Instances[j]
				clone := Clone{
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
		analysis.Summary.P50Similarity = stats.Percentile(similarities, 50)
		analysis.Summary.P95Similarity = stats.Percentile(similarities, 95)
	}

	// Calculate duplication ratio
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

// Close releases analyzer resources.
func (a *Analyzer) Close() {
	a.parser.Close()
}
