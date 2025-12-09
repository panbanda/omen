package analysis

import (
	"context"
	"sort"

	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/analyzer/changes"
	"github.com/panbanda/omen/pkg/analyzer/churn"
	"github.com/panbanda/omen/pkg/analyzer/cohesion"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/analyzer/deadcode"
	"github.com/panbanda/omen/pkg/analyzer/defect"
	"github.com/panbanda/omen/pkg/analyzer/duplicates"
	"github.com/panbanda/omen/pkg/analyzer/featureflags"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/panbanda/omen/pkg/analyzer/hotspot"
	"github.com/panbanda/omen/pkg/analyzer/ownership"
	"github.com/panbanda/omen/pkg/analyzer/repomap"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/panbanda/omen/pkg/analyzer/smells"
	"github.com/panbanda/omen/pkg/analyzer/tdg"
	"github.com/panbanda/omen/pkg/analyzer/temporal"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/source"
)

// Service orchestrates code analysis operations.
type Service struct {
	config *config.Config
	opener vcs.Opener
}

// Option configures a Service.
type Option func(*Service)

// WithConfig sets the configuration.
func WithConfig(cfg *config.Config) Option {
	return func(s *Service) {
		s.config = cfg
	}
}

// WithOpener sets the VCS opener (for testing).
func WithOpener(opener vcs.Opener) Option {
	return func(s *Service) {
		s.opener = opener
	}
}

// New creates a new analysis service.
func New(opts ...Option) *Service {
	cfg, _ := config.LoadOrDefault()
	if cfg == nil {
		cfg = config.DefaultConfig()
	}
	s := &Service{
		config: cfg,
		opener: vcs.DefaultOpener(),
	}
	for _, opt := range opts {
		opt(s)
	}
	return s
}

// ComplexityOptions configures complexity analysis.
type ComplexityOptions struct {
	CyclomaticThreshold int
	CognitiveThreshold  int
	FunctionsOnly       bool
	MaxFileSize         int64
	OnProgress          func()
}

// AnalyzeComplexity runs complexity analysis on the given files.
func (s *Service) AnalyzeComplexity(ctx context.Context, files []string, opts ComplexityOptions) (*complexity.Analysis, error) {
	var analyzerOpts []complexity.Option
	if opts.MaxFileSize > 0 {
		analyzerOpts = append(analyzerOpts, complexity.WithMaxFileSize(opts.MaxFileSize))
	} else if s.config.Analysis.MaxFileSize > 0 {
		analyzerOpts = append(analyzerOpts, complexity.WithMaxFileSize(s.config.Analysis.MaxFileSize))
	}

	cxAnalyzer := complexity.New(analyzerOpts...)
	defer cxAnalyzer.Close()

	if opts.OnProgress != nil {
		tracker := analyzer.NewTracker(func(_, _ int, _ string) {
			opts.OnProgress()
		})
		ctx = analyzer.WithTracker(ctx, tracker)
	}
	return cxAnalyzer.Analyze(ctx, files, source.NewFilesystem())
}

// SATDOptions configures SATD analysis.
type SATDOptions struct {
	IncludeTests   bool
	StrictMode     bool
	CustomPatterns []PatternConfig
	OnProgress     func()
}

// PatternConfig defines a custom SATD pattern.
type PatternConfig struct {
	Pattern  string
	Category satd.Category
	Severity satd.Severity
}

// AnalyzeSATD runs self-admitted technical debt analysis.
func (s *Service) AnalyzeSATD(ctx context.Context, files []string, opts SATDOptions) (*satd.Analysis, error) {
	var analyzerOpts []satd.Option
	if !opts.IncludeTests {
		analyzerOpts = append(analyzerOpts, satd.WithSkipTests())
	}
	if opts.StrictMode {
		analyzerOpts = append(analyzerOpts, satd.WithStrictMode())
	}

	satdAnalyzer := satd.New(analyzerOpts...)
	defer satdAnalyzer.Close()

	for _, p := range opts.CustomPatterns {
		if err := satdAnalyzer.AddPattern(p.Pattern, p.Category, p.Severity); err != nil {
			return nil, &PatternError{Pattern: p.Pattern, Err: err}
		}
	}

	if opts.OnProgress != nil {
		tracker := analyzer.NewTracker(func(_, _ int, _ string) {
			opts.OnProgress()
		})
		ctx = analyzer.WithTracker(ctx, tracker)
	}
	return satdAnalyzer.Analyze(ctx, files, source.NewFilesystem())
}

// DeadCodeOptions configures dead code detection.
type DeadCodeOptions struct {
	Confidence float64
	OnProgress func()
}

// AnalyzeDeadCode detects potentially unused code.
func (s *Service) AnalyzeDeadCode(ctx context.Context, files []string, opts DeadCodeOptions) (*deadcode.Analysis, error) {
	confidence := opts.Confidence
	if confidence == 0 {
		confidence = s.config.Thresholds.DeadCodeConfidence
	}

	dcAnalyzer := deadcode.New(deadcode.WithConfidence(confidence))
	defer dcAnalyzer.Close()

	if opts.OnProgress != nil {
		tracker := analyzer.NewTracker(func(_, _ int, _ string) {
			opts.OnProgress()
		})
		ctx = analyzer.WithTracker(ctx, tracker)
	}
	return dcAnalyzer.Analyze(ctx, files)
}

// ChurnOptions configures churn analysis.
type ChurnOptions struct {
	Days    int
	Top     int
	Spinner *progress.Tracker
}

// AnalyzeChurn analyzes git commit history for file churn.
func (s *Service) AnalyzeChurn(ctx context.Context, repoPath string, opts ChurnOptions) (*churn.Analysis, error) {
	days := opts.Days
	if days <= 0 {
		days = s.config.Analysis.ChurnDays
	}

	var analyzerOpts []churn.Option
	analyzerOpts = append(analyzerOpts, churn.WithDays(days))
	analyzerOpts = append(analyzerOpts, churn.WithOpener(s.opener))
	if opts.Spinner != nil {
		analyzerOpts = append(analyzerOpts, churn.WithSpinner(opts.Spinner))
	}

	churnAnalyzer := churn.New(analyzerOpts...)
	defer churnAnalyzer.Close()

	return churnAnalyzer.Analyze(ctx, repoPath, nil)
}

// DuplicatesOptions configures duplicate detection.
type DuplicatesOptions struct {
	MinLines            int
	SimilarityThreshold float64
	OnProgress          func()
}

// AnalyzeDuplicates detects code clones.
func (s *Service) AnalyzeDuplicates(ctx context.Context, files []string, opts DuplicatesOptions) (*duplicates.Analysis, error) {
	minTokens := opts.MinLines * 8 // Convert lines to approximate tokens
	if opts.MinLines <= 0 {
		minTokens = s.config.Thresholds.DuplicateMinLines * 8
	}

	threshold := opts.SimilarityThreshold
	if threshold == 0 {
		threshold = s.config.Thresholds.DuplicateSimilarity
	}

	dupAnalyzer := duplicates.New(
		duplicates.WithMinTokens(minTokens),
		duplicates.WithSimilarityThreshold(threshold),
	)
	defer dupAnalyzer.Close()

	if opts.OnProgress != nil {
		tracker := analyzer.NewTracker(func(_, _ int, _ string) {
			opts.OnProgress()
		})
		ctx = analyzer.WithTracker(ctx, tracker)
	}
	return dupAnalyzer.Analyze(ctx, files, source.NewFilesystem())
}

// DefectOptions configures defect prediction.
type DefectOptions struct {
	HighRiskOnly bool
	ChurnDays    int
	MaxFileSize  int64
}

// AnalyzeDefects predicts defect probability.
func (s *Service) AnalyzeDefects(ctx context.Context, repoPath string, files []string, opts DefectOptions) (*defect.Analysis, error) {
	churnDays := opts.ChurnDays
	if churnDays <= 0 {
		churnDays = s.config.Analysis.ChurnDays
	}

	maxFileSize := opts.MaxFileSize
	if maxFileSize <= 0 {
		maxFileSize = s.config.Analysis.MaxFileSize
	}

	defectAnalyzer := defect.New(
		defect.WithChurnDays(churnDays),
		defect.WithMaxFileSize(maxFileSize),
	)
	defer defectAnalyzer.Close()

	return defectAnalyzer.Analyze(ctx, repoPath, files)
}

// TDGOptions configures TDG analysis.
type TDGOptions struct {
	Hotspots      int
	ShowPenalties bool
}

// AnalyzeTDG calculates Technical Debt Gradient scores.
func (s *Service) AnalyzeTDG(ctx context.Context, files []string) (*tdg.Analysis, error) {
	tdgAnalyzer := tdg.New()
	defer tdgAnalyzer.Close()

	return tdgAnalyzer.Analyze(ctx, files, source.NewFilesystem())
}

// GraphOptions configures dependency graph analysis.
type GraphOptions struct {
	Scope          graph.Scope
	IncludeMetrics bool
	OnProgress     func()
}

// AnalyzeGraph builds a dependency graph.
func (s *Service) AnalyzeGraph(ctx context.Context, files []string, opts GraphOptions) (*graph.DependencyGraph, *graph.Metrics, error) {
	scope := opts.Scope
	if scope == "" {
		scope = graph.ScopeModule
	}

	graphAnalyzer := graph.New(graph.WithScope(scope))
	defer graphAnalyzer.Close()

	if opts.OnProgress != nil {
		tracker := analyzer.NewTracker(func(_, _ int, _ string) {
			opts.OnProgress()
		})
		ctx = analyzer.WithTracker(ctx, tracker)
	}

	depGraph, err := graphAnalyzer.Analyze(ctx, files, source.NewFilesystem())
	if err != nil {
		return nil, nil, err
	}

	var metrics *graph.Metrics
	if opts.IncludeMetrics {
		metrics = graphAnalyzer.CalculateMetrics(depGraph)
	}

	return depGraph, metrics, nil
}

// HotspotOptions configures hotspot analysis.
type HotspotOptions struct {
	Days        int
	Top         int
	MaxFileSize int64
	OnProgress  func()
}

// AnalyzeHotspots identifies code hotspots (high churn + high complexity).
func (s *Service) AnalyzeHotspots(ctx context.Context, repoPath string, files []string, opts HotspotOptions) (*hotspot.Analysis, error) {
	days := opts.Days
	if days <= 0 {
		days = s.config.Analysis.ChurnDays
	}

	maxFileSize := opts.MaxFileSize
	if maxFileSize <= 0 {
		maxFileSize = s.config.Analysis.MaxFileSize
	}

	hotspotAnalyzer := hotspot.New(
		hotspot.WithChurnDays(days),
		hotspot.WithMaxFileSize(maxFileSize),
	)
	defer hotspotAnalyzer.Close()

	if opts.OnProgress != nil {
		tracker := analyzer.NewTracker(func(_, _ int, _ string) {
			opts.OnProgress()
		})
		ctx = analyzer.WithTracker(ctx, tracker)
	}
	return hotspotAnalyzer.Analyze(ctx, repoPath, files)
}

// RankedFile represents a file with its hotspot score for sorting.
type RankedFile struct {
	Path  string
	Score float64
}

// SortFilesByHotspot returns files sorted by hotspot score (highest first).
// This combines churn and complexity to surface the most problematic files.
func (s *Service) SortFilesByHotspot(ctx context.Context, repoPath string, files []string, opts HotspotOptions) ([]RankedFile, error) {
	analysis, err := s.AnalyzeHotspots(ctx, repoPath, files, opts)
	if err != nil {
		return nil, err
	}

	// Build lookup from hotspot results
	scoreMap := make(map[string]float64)
	for _, hs := range analysis.Files {
		scoreMap[hs.Path] = hs.HotspotScore
	}

	// Create ranked list
	ranked := make([]RankedFile, 0, len(files))
	for _, f := range files {
		ranked = append(ranked, RankedFile{
			Path:  f,
			Score: scoreMap[f], // 0 if not in hotspots
		})
	}

	// Sort by score descending
	sort.Slice(ranked, func(i, j int) bool {
		return ranked[i].Score > ranked[j].Score
	})

	return ranked, nil
}

// TemporalCouplingOptions configures temporal coupling analysis.
type TemporalCouplingOptions struct {
	Days         int
	MinCochanges int
	Top          int
}

// AnalyzeTemporalCoupling identifies files that frequently change together.
func (s *Service) AnalyzeTemporalCoupling(ctx context.Context, repoPath string, opts TemporalCouplingOptions) (*temporal.Analysis, error) {
	days := opts.Days
	if days <= 0 {
		days = 30
	}

	minCochanges := opts.MinCochanges
	if minCochanges <= 0 {
		minCochanges = 3
	}

	tcAnalyzer := temporal.New(days, minCochanges,
		temporal.WithOpener(s.opener))
	defer tcAnalyzer.Close()

	return tcAnalyzer.Analyze(ctx, repoPath, nil)
}

// OwnershipOptions configures ownership analysis.
type OwnershipOptions struct {
	Top            int
	IncludeTrivial bool
	OnProgress     func()
}

// AnalyzeOwnership calculates code ownership and bus factor.
func (s *Service) AnalyzeOwnership(ctx context.Context, repoPath string, files []string, opts OwnershipOptions) (*ownership.Analysis, error) {
	var analyzerOpts []ownership.Option
	if opts.IncludeTrivial {
		analyzerOpts = append(analyzerOpts, ownership.WithIncludeTrivial())
	}
	analyzerOpts = append(analyzerOpts, ownership.WithOpener(s.opener))

	ownAnalyzer := ownership.New(analyzerOpts...)
	defer ownAnalyzer.Close()

	if opts.OnProgress != nil {
		tracker := analyzer.NewTracker(func(_, _ int, _ string) {
			opts.OnProgress()
		})
		ctx = analyzer.WithTracker(ctx, tracker)
	}
	return ownAnalyzer.Analyze(ctx, repoPath, files)
}

// CohesionOptions configures cohesion analysis.
type CohesionOptions struct {
	IncludeTests bool
	Sort         string
	Top          int
	OnProgress   func()
}

// AnalyzeCohesion calculates CK (Chidamber-Kemerer) OO metrics.
func (s *Service) AnalyzeCohesion(ctx context.Context, files []string, opts CohesionOptions) (*cohesion.Analysis, error) {
	var analyzerOpts []cohesion.Option
	if opts.IncludeTests {
		analyzerOpts = append(analyzerOpts, cohesion.WithIncludeTestFiles())
	}

	ckAnalyzer := cohesion.New(analyzerOpts...)
	defer ckAnalyzer.Close()

	if opts.OnProgress != nil {
		tracker := analyzer.NewTracker(func(_, _ int, _ string) {
			opts.OnProgress()
		})
		ctx = analyzer.WithTracker(ctx, tracker)
	}
	return ckAnalyzer.Analyze(ctx, files, source.NewFilesystem())
}

// RepoMapOptions configures repo map generation.
type RepoMapOptions struct {
	Top int
}

// AnalyzeRepoMap generates a PageRank-ranked symbol map.
func (s *Service) AnalyzeRepoMap(ctx context.Context, files []string, opts RepoMapOptions) (*repomap.Map, error) {
	rmAnalyzer := repomap.New()
	defer rmAnalyzer.Close()

	return rmAnalyzer.Analyze(ctx, files)
}

// ChangesOptions configures change-level defect prediction.
type ChangesOptions struct {
	Days    int
	Weights changes.Weights
}

// AnalyzeChanges performs change-level defect prediction on recent commits.
func (s *Service) AnalyzeChanges(ctx context.Context, repoPath string, opts ChangesOptions) (*changes.Analysis, error) {
	days := opts.Days
	if days <= 0 {
		days = s.config.Analysis.ChurnDays
	}

	var analyzerOpts []changes.Option
	analyzerOpts = append(analyzerOpts, changes.WithDays(days))
	analyzerOpts = append(analyzerOpts, changes.WithOpener(s.opener))

	// Only set weights if they're not zero-valued
	if opts.Weights.FIX != 0 || opts.Weights.Entropy != 0 {
		analyzerOpts = append(analyzerOpts, changes.WithWeights(opts.Weights))
	}

	changesAnalyzer := changes.New(analyzerOpts...)
	defer changesAnalyzer.Close()

	return changesAnalyzer.Analyze(ctx, repoPath, nil)
}

// SmellOptions configures architectural smell detection.
type SmellOptions struct {
	HubThreshold          int
	GodFanInThreshold     int
	GodFanOutThreshold    int
	InstabilityDifference float64
	OnProgress            func()
}

// AnalyzeSmells detects architectural smells in a dependency graph.
func (s *Service) AnalyzeSmells(ctx context.Context, files []string, opts SmellOptions) (*smells.Analysis, error) {
	// First build the dependency graph
	graphAnalyzer := graph.New(graph.WithScope(graph.ScopeFile))
	defer graphAnalyzer.Close()

	if opts.OnProgress != nil {
		tracker := analyzer.NewTracker(func(_, _ int, _ string) {
			opts.OnProgress()
		})
		ctx = analyzer.WithTracker(ctx, tracker)
	}

	depGraph, err := graphAnalyzer.Analyze(ctx, files, source.NewFilesystem())
	if err != nil {
		return nil, err
	}

	// Configure smell analyzer
	var smellOpts []smells.Option
	if opts.HubThreshold > 0 {
		smellOpts = append(smellOpts, smells.WithHubThreshold(opts.HubThreshold))
	}
	if opts.GodFanInThreshold > 0 || opts.GodFanOutThreshold > 0 {
		fanIn := opts.GodFanInThreshold
		fanOut := opts.GodFanOutThreshold
		if fanIn == 0 {
			fanIn = 10
		}
		if fanOut == 0 {
			fanOut = 10
		}
		smellOpts = append(smellOpts, smells.WithGodThresholds(fanIn, fanOut))
	}
	if opts.InstabilityDifference > 0 {
		smellOpts = append(smellOpts, smells.WithInstabilityDifference(opts.InstabilityDifference))
	}

	smellAnalyzer := smells.New(smellOpts...)
	defer smellAnalyzer.Close()

	return smellAnalyzer.AnalyzeGraph(depGraph), nil
}

// FeatureFlagOptions configures feature flag analysis.
type FeatureFlagOptions struct {
	Providers   []string
	IncludeGit  bool
	ExpectedTTL int
	OnProgress  func()
}

// AnalyzeFeatureFlags detects and analyzes feature flags in the codebase.
func (s *Service) AnalyzeFeatureFlags(ctx context.Context, files []string, opts FeatureFlagOptions) (*featureflags.Analysis, error) {
	var analyzerOpts []featureflags.Option

	if len(opts.Providers) > 0 {
		analyzerOpts = append(analyzerOpts, featureflags.WithProviders(opts.Providers))
	} else if len(s.config.FeatureFlags.Providers) > 0 {
		analyzerOpts = append(analyzerOpts, featureflags.WithProviders(s.config.FeatureFlags.Providers))
	}

	if s.config.Analysis.MaxFileSize > 0 {
		analyzerOpts = append(analyzerOpts, featureflags.WithMaxFileSize(s.config.Analysis.MaxFileSize))
	}

	analyzerOpts = append(analyzerOpts, featureflags.WithGitHistory(opts.IncludeGit))
	analyzerOpts = append(analyzerOpts, featureflags.WithVCSOpener(s.opener))

	ttl := opts.ExpectedTTL
	if ttl == 0 {
		ttl = s.config.FeatureFlags.ExpectedTTL.Release
	}
	if ttl == 0 {
		ttl = 14 // default
	}
	analyzerOpts = append(analyzerOpts, featureflags.WithExpectedTTL(ttl))

	// Load custom providers from config
	if len(s.config.FeatureFlags.CustomProviders) > 0 {
		customProviders := make([]featureflags.CustomProvider, len(s.config.FeatureFlags.CustomProviders))
		for i, cp := range s.config.FeatureFlags.CustomProviders {
			customProviders[i] = featureflags.CustomProvider{
				Name:      cp.Name,
				Languages: cp.Languages,
				Query:     cp.Query,
			}
		}
		analyzerOpts = append(analyzerOpts, featureflags.WithCustomProviders(customProviders))
	}

	flagAnalyzer, err := featureflags.New(analyzerOpts...)
	if err != nil {
		return nil, err
	}
	defer flagAnalyzer.Close()

	if opts.OnProgress != nil {
		tracker := analyzer.NewTracker(func(_, _ int, _ string) {
			opts.OnProgress()
		})
		ctx = analyzer.WithTracker(ctx, tracker)
	}
	return flagAnalyzer.Analyze(ctx, files)
}

// PatternError indicates an invalid pattern.
type PatternError struct {
	Pattern string
	Err     error
}

func (e *PatternError) Error() string {
	return "invalid pattern " + e.Pattern + ": " + e.Err.Error()
}

func (e *PatternError) Unwrap() error {
	return e.Err
}
