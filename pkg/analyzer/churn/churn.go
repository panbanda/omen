package churn

import (
	"bufio"
	"bytes"
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/analyzer"
)

// DefaultGitTimeout is the default timeout for git operations.
const DefaultGitTimeout = 5 * time.Minute

// Analyzer analyzes git commit history for file churn.
type Analyzer struct {
	days      int
	spinner   *progress.Tracker
	opener    vcs.Opener
	useNative bool // use native git commands for better performance
}

// Compile-time check that Analyzer implements RepoAnalyzer.
var _ analyzer.RepoAnalyzer[*Analysis] = (*Analyzer)(nil)

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithDays sets the number of days to analyze git history.
func WithDays(days int) Option {
	return func(a *Analyzer) {
		if days > 0 {
			a.days = days
		}
	}
}

// WithSpinner sets a progress spinner for the analyzer.
func WithSpinner(spinner *progress.Tracker) Option {
	return func(a *Analyzer) {
		a.spinner = spinner
	}
}

// WithOpener sets the VCS opener (useful for testing).
// Using this option disables native git and falls back to go-git.
func WithOpener(opener vcs.Opener) Option {
	return func(a *Analyzer) {
		a.opener = opener
		a.useNative = false // custom opener means we're likely testing with mocks
	}
}

// WithNativeGit forces use of native git commands (default: true).
func WithNativeGit(use bool) Option {
	return func(a *Analyzer) {
		a.useNative = use
	}
}

// New creates a new churn analyzer.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		days:      30,
		spinner:   nil,
		opener:    vcs.DefaultOpener(),
		useNative: true, // default to fast native git
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// Analyze analyzes git history for a repository, optionally filtering to specific files.
func (a *Analyzer) Analyze(ctx context.Context, repoPath string, files []string) (*Analysis, error) {
	if ctx == nil {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(context.Background(), DefaultGitTimeout)
		defer cancel()
	}

	absPath, err := filepath.Abs(repoPath)
	if err != nil {
		absPath = repoPath
	}

	// Use native git for much better performance on large repos
	if a.useNative {
		return a.analyzeNative(ctx, repoPath, absPath, files)
	}

	return a.analyzeGoGit(ctx, repoPath, absPath, files)
}

// analyzeNative uses native git commands for better performance.
// git log --numstat is ~30x faster than go-git tree diffs.
func (a *Analyzer) analyzeNative(ctx context.Context, repoPath, absPath string, files []string) (*Analysis, error) {
	cutoff := time.Now().AddDate(0, 0, -a.days)
	sinceDate := cutoff.Format("2006-01-02")

	// Build git log command
	// Format: commit_hash|author_name|author_date
	// Followed by numstat lines: added\tdeleted\tfilepath
	args := []string{
		"log",
		"--numstat",
		"--since=" + sinceDate,
		"--format=%H|%aN|%aI",
	}

	cmd := exec.CommandContext(ctx, "git", args...)
	cmd.Dir = repoPath

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		// Check if it's not a git repo
		if strings.Contains(stderr.String(), "not a git repository") {
			return nil, err
		}
		return nil, err
	}

	fileMetrics := make(map[string]*FileMetrics)
	if err := a.parseGitLogNumstat(&stdout, fileMetrics); err != nil {
		return nil, err
	}

	fullAnalysis := buildAnalysis(fileMetrics, absPath, a.days)

	// If no files specified, return full analysis
	if len(files) == 0 {
		return fullAnalysis, nil
	}

	return a.filterAnalysis(fullAnalysis, repoPath, files)
}

// parseGitLogNumstat parses output from git log --numstat --format="%H|%aN|%aI".
func (a *Analyzer) parseGitLogNumstat(r *bytes.Buffer, fileMetrics map[string]*FileMetrics) error {
	scanner := bufio.NewScanner(r)

	var currentHash, currentAuthor string
	var currentTime time.Time

	for scanner.Scan() {
		line := scanner.Text()

		// Skip empty lines
		if line == "" {
			continue
		}

		// Check if this is a commit line (contains |)
		if strings.Contains(line, "|") {
			parts := strings.SplitN(line, "|", 3)
			if len(parts) == 3 {
				currentHash = parts[0]
				currentAuthor = parts[1]
				currentTime, _ = time.Parse(time.RFC3339, parts[2])
				_ = currentHash // unused but kept for potential future use
				continue
			}
		}

		// This is a numstat line: added\tdeleted\tfilepath
		parts := strings.Split(line, "\t")
		if len(parts) != 3 {
			continue
		}

		addedStr, deletedStr, relativePath := parts[0], parts[1], parts[2]

		// Skip binary files (shown as "-")
		if addedStr == "-" || deletedStr == "-" {
			continue
		}

		added, _ := strconv.Atoi(addedStr)
		deleted, _ := strconv.Atoi(deletedStr)

		if _, exists := fileMetrics[relativePath]; !exists {
			fileMetrics[relativePath] = &FileMetrics{
				Path:         "./" + relativePath,
				RelativePath: relativePath,
				AuthorCounts: make(map[string]int),
				FirstCommit:  currentTime,
				LastCommit:   currentTime,
			}
		}

		fm := fileMetrics[relativePath]
		fm.Commits++
		fm.AuthorCounts[currentAuthor]++
		fm.LinesAdded += added
		fm.LinesDeleted += deleted

		if currentTime.Before(fm.FirstCommit) {
			fm.FirstCommit = currentTime
		}
		if currentTime.After(fm.LastCommit) {
			fm.LastCommit = currentTime
		}

		if a.spinner != nil {
			a.spinner.Tick()
		}
	}

	return scanner.Err()
}

// analyzeGoGit uses go-git for analysis (slower but works with mocked repos).
func (a *Analyzer) analyzeGoGit(ctx context.Context, repoPath, absPath string, files []string) (*Analysis, error) {
	repo, err := a.opener.PlainOpen(repoPath)
	if err != nil {
		return nil, err
	}

	cutoff := time.Now().AddDate(0, 0, -a.days)

	logIter, err := repo.Log(&vcs.LogOptions{
		Since: &cutoff,
	})
	if err != nil {
		return nil, err
	}
	defer logIter.Close()

	fileMetrics := make(map[string]*FileMetrics)

	err = logIter.ForEach(func(commit vcs.Commit) error {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		if a.spinner != nil {
			a.spinner.Tick()
		}

		return a.processCommit(commit, fileMetrics)
	})

	if err != nil {
		return nil, err
	}

	fullAnalysis := buildAnalysis(fileMetrics, absPath, a.days)

	// If no files specified, return full analysis
	if len(files) == 0 {
		return fullAnalysis, nil
	}

	return a.filterAnalysis(fullAnalysis, repoPath, files)
}

// filterAnalysis filters the analysis to only include specified files.
func (a *Analyzer) filterAnalysis(fullAnalysis *Analysis, repoPath string, files []string) (*Analysis, error) {
	fileSet := make(map[string]bool)
	for _, f := range files {
		rel, err := filepath.Rel(repoPath, f)
		if err != nil {
			rel = f
		}
		fileSet[rel] = true
		fileSet[f] = true
		fileSet["./"+rel] = true
	}

	filtered := &Analysis{
		GeneratedAt:    time.Now().UTC(),
		PeriodDays:     a.days,
		RepositoryRoot: fullAnalysis.RepositoryRoot,
		Files:          make([]FileMetrics, 0),
		Summary:        NewSummary(),
	}

	for _, fm := range fullAnalysis.Files {
		if fileSet[fm.Path] || fileSet[fm.RelativePath] {
			filtered.Files = append(filtered.Files, fm)
		}
	}

	var totalCommits, totalAdded, totalDeleted int
	for _, fm := range filtered.Files {
		totalCommits += fm.Commits
		totalAdded += fm.LinesAdded
		totalDeleted += fm.LinesDeleted
		for author, count := range fm.AuthorCounts {
			filtered.Summary.AuthorContributions[author] += count
		}
	}

	filtered.Summary.TotalFilesChanged = len(filtered.Files)
	filtered.Summary.TotalFileChanges = totalCommits
	filtered.Summary.TotalAdditions = totalAdded
	filtered.Summary.TotalDeletions = totalDeleted

	if len(filtered.Files) > 0 {
		filtered.Summary.AvgCommitsPerFile = float64(totalCommits) / float64(len(filtered.Files))

		sort.Slice(filtered.Files, func(i, j int) bool {
			return filtered.Files[i].ChurnScore > filtered.Files[j].ChurnScore
		})
		filtered.Summary.MaxChurnScore = filtered.Files[0].ChurnScore
	}

	return filtered, nil
}

// Close releases any resources held by the analyzer.
func (a *Analyzer) Close() {
}

// processCommit extracts churn data from a single commit.
func (a *Analyzer) processCommit(commit vcs.Commit, fileMetrics map[string]*FileMetrics) error {
	if commit.NumParents() == 0 {
		return nil
	}

	parent, err := commit.Parent(0)
	if err != nil {
		return nil
	}

	parentTree, err := parent.Tree()
	if err != nil {
		return nil
	}

	commitTree, err := commit.Tree()
	if err != nil {
		return nil
	}

	changes, err := parentTree.Diff(commitTree)
	if err != nil {
		return nil
	}

	for _, change := range changes {
		relativePath := change.ToName()
		if relativePath == "" {
			relativePath = change.FromName() // Deleted file
		}

		if _, exists := fileMetrics[relativePath]; !exists {
			fileMetrics[relativePath] = &FileMetrics{
				Path:         "./" + relativePath, // pmat prefixes with ./
				RelativePath: relativePath,
				AuthorCounts: make(map[string]int),
				FirstCommit:  commit.Author().When,
				LastCommit:   commit.Author().When,
			}
		}

		fm := fileMetrics[relativePath]
		fm.Commits++
		fm.AuthorCounts[commit.Author().Name]++

		if commit.Author().When.Before(fm.FirstCommit) {
			fm.FirstCommit = commit.Author().When
		}
		if commit.Author().When.After(fm.LastCommit) {
			fm.LastCommit = commit.Author().When
		}

		patch, err := change.Patch()
		if err == nil {
			for _, filePatch := range patch.FilePatches() {
				for _, chunk := range filePatch.Chunks() {
					content := chunk.Content()
					switch chunk.Type() {
					case vcs.ChunkAdd:
						fm.LinesAdded += countLines(content)
					case vcs.ChunkDelete:
						fm.LinesDeleted += countLines(content)
					}
				}
			}
		}
	}

	return nil
}

// buildAnalysis constructs the final analysis from collected metrics.
func buildAnalysis(fileMetrics map[string]*FileMetrics, absPath string, days int) *Analysis {
	analysis := &Analysis{
		GeneratedAt:    time.Now().UTC(),
		PeriodDays:     days,
		RepositoryRoot: absPath,
		Files:          make([]FileMetrics, 0, len(fileMetrics)),
		Summary:        NewSummary(),
	}

	// Find max values for normalization
	var maxCommits, maxChanges int
	for _, fm := range fileMetrics {
		if fm.Commits > maxCommits {
			maxCommits = fm.Commits
		}
		changes := fm.LinesAdded + fm.LinesDeleted
		if changes > maxChanges {
			maxChanges = changes
		}
	}

	// Calculate scores and collect stats
	var totalCommits, totalAdded, totalDeleted int
	now := time.Now()

	for _, fm := range fileMetrics {
		fm.UniqueAuthors = make([]string, 0, len(fm.AuthorCounts))
		for author := range fm.AuthorCounts {
			fm.UniqueAuthors = append(fm.UniqueAuthors, author)
		}

		fm.CalculateChurnScoreWithMax(maxCommits, maxChanges)

		filePath := absPath + "/" + fm.RelativePath
		fm.TotalLOC, fm.LOCReadError = countFileLOC(filePath)
		fm.CalculateRelativeChurn(now)

		analysis.Files = append(analysis.Files, *fm)

		totalCommits += fm.Commits
		totalAdded += fm.LinesAdded
		totalDeleted += fm.LinesDeleted

		for author, count := range fm.AuthorCounts {
			analysis.Summary.AuthorContributions[author] += count
		}
	}

	// Sort by churn score (highest first)
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].ChurnScore > analysis.Files[j].ChurnScore
	})

	// Build summary
	analysis.Summary.TotalFilesChanged = len(analysis.Files)
	analysis.Summary.TotalFileChanges = totalCommits
	analysis.Summary.TotalAdditions = totalAdded
	analysis.Summary.TotalDeletions = totalDeleted

	if len(analysis.Files) > 0 {
		analysis.Summary.AvgCommitsPerFile = float64(totalCommits) / float64(len(analysis.Files))
		analysis.Summary.MaxChurnScore = analysis.Files[0].ChurnScore
	}

	analysis.Summary.CalculateStatistics(analysis.Files)
	analysis.Summary.IdentifyHotspotAndStableFiles(analysis.Files)

	return analysis
}

// countLines counts the number of newlines in content.
func countLines(content string) int {
	return strings.Count(content, "\n")
}

// countFileLOC counts the number of lines in a file on disk.
// Returns the line count and whether an error occurred.
func countFileLOC(path string) (int, bool) {
	f, err := os.Open(path)
	if err != nil {
		return 0, true
	}
	defer f.Close()

	count := 0
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		count++
	}
	if scanner.Err() != nil {
		return count, true
	}
	return count, false
}
