package analysis

import (
	"github.com/panbanda/omen/internal/analyzer"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/models"
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
	s := &Service{
		config: config.LoadOrDefault(),
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
func (s *Service) AnalyzeComplexity(files []string, opts ComplexityOptions) (*models.ComplexityAnalysis, error) {
	var analyzerOpts []analyzer.ComplexityOption
	if opts.MaxFileSize > 0 {
		analyzerOpts = append(analyzerOpts, analyzer.WithComplexityMaxFileSize(opts.MaxFileSize))
	} else if s.config.Analysis.MaxFileSize > 0 {
		analyzerOpts = append(analyzerOpts, analyzer.WithComplexityMaxFileSize(s.config.Analysis.MaxFileSize))
	}

	cxAnalyzer := analyzer.NewComplexityAnalyzer(analyzerOpts...)
	defer cxAnalyzer.Close()

	if opts.OnProgress != nil {
		return cxAnalyzer.AnalyzeProjectWithProgress(files, opts.OnProgress)
	}
	return cxAnalyzer.AnalyzeProject(files)
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
	Category models.DebtCategory
	Severity models.Severity
}

// AnalyzeSATD runs self-admitted technical debt analysis.
func (s *Service) AnalyzeSATD(files []string, opts SATDOptions) (*models.SATDAnalysis, error) {
	var analyzerOpts []analyzer.SATDOption
	if !opts.IncludeTests {
		analyzerOpts = append(analyzerOpts, analyzer.WithSATDSkipTests())
	}
	if opts.StrictMode {
		analyzerOpts = append(analyzerOpts, analyzer.WithSATDStrictMode())
	}

	satdAnalyzer := analyzer.NewSATDAnalyzer(analyzerOpts...)

	for _, p := range opts.CustomPatterns {
		if err := satdAnalyzer.AddPattern(p.Pattern, p.Category, p.Severity); err != nil {
			return nil, &PatternError{Pattern: p.Pattern, Err: err}
		}
	}

	if opts.OnProgress != nil {
		return satdAnalyzer.AnalyzeProjectWithProgress(files, opts.OnProgress)
	}
	return satdAnalyzer.AnalyzeProject(files)
}

// DeadCodeOptions configures dead code detection.
type DeadCodeOptions struct {
	Confidence float64
	OnProgress func()
}

// AnalyzeDeadCode detects potentially unused code.
func (s *Service) AnalyzeDeadCode(files []string, opts DeadCodeOptions) (*models.DeadCodeAnalysis, error) {
	confidence := opts.Confidence
	if confidence == 0 {
		confidence = s.config.Thresholds.DeadCodeConfidence
	}

	dcAnalyzer := analyzer.NewDeadCodeAnalyzer(analyzer.WithDeadCodeConfidence(confidence))
	defer dcAnalyzer.Close()

	if opts.OnProgress != nil {
		return dcAnalyzer.AnalyzeProjectWithProgress(files, opts.OnProgress)
	}
	return dcAnalyzer.AnalyzeProject(files)
}

// ChurnOptions configures churn analysis.
type ChurnOptions struct {
	Days    int
	Top     int
	Spinner *progress.Tracker
}

// AnalyzeChurn analyzes git commit history for file churn.
func (s *Service) AnalyzeChurn(repoPath string, opts ChurnOptions) (*models.ChurnAnalysis, error) {
	days := opts.Days
	if days <= 0 {
		days = s.config.Analysis.ChurnDays
	}

	var analyzerOpts []analyzer.ChurnOption
	analyzerOpts = append(analyzerOpts, analyzer.WithChurnDays(days))
	analyzerOpts = append(analyzerOpts, analyzer.WithChurnOpener(s.opener))
	if opts.Spinner != nil {
		analyzerOpts = append(analyzerOpts, analyzer.WithChurnSpinner(opts.Spinner))
	}

	churnAnalyzer := analyzer.NewChurnAnalyzer(analyzerOpts...)
	return churnAnalyzer.AnalyzeRepo(repoPath)
}

// DuplicatesOptions configures duplicate detection.
type DuplicatesOptions struct {
	MinLines            int
	SimilarityThreshold float64
	OnProgress          func()
}

// AnalyzeDuplicates detects code clones.
func (s *Service) AnalyzeDuplicates(files []string, opts DuplicatesOptions) (*models.CloneAnalysis, error) {
	minTokens := opts.MinLines * 8 // Convert lines to approximate tokens
	if opts.MinLines <= 0 {
		minTokens = s.config.Thresholds.DuplicateMinLines * 8
	}

	threshold := opts.SimilarityThreshold
	if threshold == 0 {
		threshold = s.config.Thresholds.DuplicateSimilarity
	}

	dupAnalyzer := analyzer.NewDuplicateAnalyzer(
		analyzer.WithDuplicateMinTokens(minTokens),
		analyzer.WithDuplicateSimilarityThreshold(threshold),
	)
	defer dupAnalyzer.Close()

	if opts.OnProgress != nil {
		return dupAnalyzer.AnalyzeProjectWithProgress(files, opts.OnProgress)
	}
	return dupAnalyzer.AnalyzeProject(files)
}

// DefectOptions configures defect prediction.
type DefectOptions struct {
	HighRiskOnly bool
	ChurnDays    int
	MaxFileSize  int64
}

// AnalyzeDefects predicts defect probability.
func (s *Service) AnalyzeDefects(repoPath string, files []string, opts DefectOptions) (*models.DefectAnalysis, error) {
	churnDays := opts.ChurnDays
	if churnDays <= 0 {
		churnDays = s.config.Analysis.ChurnDays
	}

	maxFileSize := opts.MaxFileSize
	if maxFileSize <= 0 {
		maxFileSize = s.config.Analysis.MaxFileSize
	}

	defectAnalyzer := analyzer.NewDefectAnalyzer(
		analyzer.WithDefectChurnDays(churnDays),
		analyzer.WithDefectMaxFileSize(maxFileSize),
	)
	defer defectAnalyzer.Close()

	return defectAnalyzer.AnalyzeProject(repoPath, files)
}

// TDGOptions configures TDG analysis.
type TDGOptions struct {
	Hotspots      int
	ShowPenalties bool
}

// AnalyzeTDG calculates Technical Debt Gradient scores.
func (s *Service) AnalyzeTDG(path string) (*models.ProjectScore, error) {
	tdgAnalyzer := analyzer.NewTdgAnalyzer()
	defer tdgAnalyzer.Close()

	project, err := tdgAnalyzer.AnalyzeProject(path)
	if err != nil {
		return nil, err
	}
	return &project, nil
}

// GraphOptions configures dependency graph analysis.
type GraphOptions struct {
	Scope          analyzer.GraphScope
	IncludeMetrics bool
	OnProgress     func()
}

// AnalyzeGraph builds a dependency graph.
func (s *Service) AnalyzeGraph(files []string, opts GraphOptions) (*models.DependencyGraph, *models.GraphMetrics, error) {
	scope := opts.Scope
	if scope == "" {
		scope = analyzer.ScopeModule
	}

	graphAnalyzer := analyzer.NewGraphAnalyzer(analyzer.WithGraphScope(scope))
	defer graphAnalyzer.Close()

	var graph *models.DependencyGraph
	var err error
	if opts.OnProgress != nil {
		graph, err = graphAnalyzer.AnalyzeProjectWithProgress(files, opts.OnProgress)
	} else {
		graph, err = graphAnalyzer.AnalyzeProject(files)
	}

	if err != nil {
		return nil, nil, err
	}

	var metrics *models.GraphMetrics
	if opts.IncludeMetrics {
		metrics = graphAnalyzer.CalculateMetrics(graph)
	}

	return graph, metrics, nil
}

// HotspotOptions configures hotspot analysis.
type HotspotOptions struct {
	Days        int
	Top         int
	MaxFileSize int64
	OnProgress  func()
}

// AnalyzeHotspots identifies code hotspots (high churn + high complexity).
func (s *Service) AnalyzeHotspots(repoPath string, files []string, opts HotspotOptions) (*models.HotspotAnalysis, error) {
	days := opts.Days
	if days <= 0 {
		days = s.config.Analysis.ChurnDays
	}

	maxFileSize := opts.MaxFileSize
	if maxFileSize <= 0 {
		maxFileSize = s.config.Analysis.MaxFileSize
	}

	hotspotAnalyzer := analyzer.NewHotspotAnalyzer(
		analyzer.WithHotspotChurnDays(days),
		analyzer.WithHotspotMaxFileSize(maxFileSize),
	)
	defer hotspotAnalyzer.Close()

	return hotspotAnalyzer.AnalyzeProjectWithProgress(repoPath, files, opts.OnProgress)
}

// TemporalCouplingOptions configures temporal coupling analysis.
type TemporalCouplingOptions struct {
	Days         int
	MinCochanges int
	Top          int
}

// AnalyzeTemporalCoupling identifies files that frequently change together.
func (s *Service) AnalyzeTemporalCoupling(repoPath string, opts TemporalCouplingOptions) (*models.TemporalCouplingAnalysis, error) {
	days := opts.Days
	if days <= 0 {
		days = 30
	}

	minCochanges := opts.MinCochanges
	if minCochanges <= 0 {
		minCochanges = 3
	}

	tcAnalyzer := analyzer.NewTemporalCouplingAnalyzer(days, minCochanges,
		analyzer.WithTemporalOpener(s.opener))
	defer tcAnalyzer.Close()

	return tcAnalyzer.AnalyzeRepo(repoPath)
}

// OwnershipOptions configures ownership analysis.
type OwnershipOptions struct {
	Top            int
	IncludeTrivial bool
	OnProgress     func()
}

// AnalyzeOwnership calculates code ownership and bus factor.
func (s *Service) AnalyzeOwnership(repoPath string, files []string, opts OwnershipOptions) (*models.OwnershipAnalysis, error) {
	var analyzerOpts []analyzer.OwnershipOption
	if opts.IncludeTrivial {
		analyzerOpts = append(analyzerOpts, analyzer.WithOwnershipIncludeTrivial())
	}
	analyzerOpts = append(analyzerOpts, analyzer.WithOwnershipOpener(s.opener))

	ownAnalyzer := analyzer.NewOwnershipAnalyzer(analyzerOpts...)
	defer ownAnalyzer.Close()

	return ownAnalyzer.AnalyzeRepoWithProgress(repoPath, files, opts.OnProgress)
}

// CohesionOptions configures cohesion analysis.
type CohesionOptions struct {
	IncludeTests bool
	Sort         string
	Top          int
	OnProgress   func()
}

// AnalyzeCohesion calculates CK (Chidamber-Kemerer) OO metrics.
func (s *Service) AnalyzeCohesion(files []string, opts CohesionOptions) (*models.CohesionAnalysis, error) {
	var analyzerOpts []analyzer.CohesionOption
	if opts.IncludeTests {
		analyzerOpts = append(analyzerOpts, analyzer.WithCohesionIncludeTestFiles())
	}

	ckAnalyzer := analyzer.NewCohesionAnalyzer(analyzerOpts...)
	defer ckAnalyzer.Close()

	return ckAnalyzer.AnalyzeProjectWithProgress(files, opts.OnProgress)
}

// RepoMapOptions configures repo map generation.
type RepoMapOptions struct {
	Top int
}

// AnalyzeRepoMap generates a PageRank-ranked symbol map.
func (s *Service) AnalyzeRepoMap(files []string, opts RepoMapOptions) (*models.RepoMap, error) {
	rmAnalyzer := analyzer.NewRepoMapAnalyzer()
	defer rmAnalyzer.Close()

	return rmAnalyzer.AnalyzeProject(files)
}

// ChangesOptions configures change-level defect prediction.
type ChangesOptions struct {
	Days    int
	Weights models.ChangesWeights
}

// AnalyzeChanges performs change-level defect prediction on recent commits.
func (s *Service) AnalyzeChanges(repoPath string, opts ChangesOptions) (*models.ChangesAnalysis, error) {
	days := opts.Days
	if days <= 0 {
		days = s.config.Analysis.ChurnDays
	}

	var analyzerOpts []analyzer.ChangesOption
	analyzerOpts = append(analyzerOpts, analyzer.WithChangesDays(days))
	analyzerOpts = append(analyzerOpts, analyzer.WithChangesOpener(s.opener))

	// Only set weights if they're not zero-valued
	if opts.Weights.FIX != 0 || opts.Weights.Entropy != 0 {
		analyzerOpts = append(analyzerOpts, analyzer.WithChangesWeights(opts.Weights))
	}

	changesAnalyzer := analyzer.NewChangesAnalyzer(analyzerOpts...)
	defer changesAnalyzer.Close()

	return changesAnalyzer.AnalyzeRepo(repoPath)
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
func (s *Service) AnalyzeSmells(files []string, opts SmellOptions) (*models.SmellAnalysis, error) {
	// First build the dependency graph
	graphAnalyzer := analyzer.NewGraphAnalyzer(analyzer.WithGraphScope(analyzer.ScopeFile))
	defer graphAnalyzer.Close()

	graph, err := graphAnalyzer.AnalyzeProjectWithProgress(files, opts.OnProgress)
	if err != nil {
		return nil, err
	}

	// Configure smell analyzer
	var smellOpts []analyzer.SmellOption
	if opts.HubThreshold > 0 {
		smellOpts = append(smellOpts, analyzer.WithHubThreshold(opts.HubThreshold))
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
		smellOpts = append(smellOpts, analyzer.WithGodThresholds(fanIn, fanOut))
	}
	if opts.InstabilityDifference > 0 {
		smellOpts = append(smellOpts, analyzer.WithInstabilityDifference(opts.InstabilityDifference))
	}

	smellAnalyzer := analyzer.NewSmellAnalyzer(smellOpts...)
	defer smellAnalyzer.Close()

	return smellAnalyzer.AnalyzeGraph(graph), nil
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
