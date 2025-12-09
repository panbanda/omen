package analysis

import (
	"context"
	"path/filepath"

	"github.com/panbanda/omen/internal/locator"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/analyzer/repomap"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/panbanda/omen/pkg/source"
)

// FocusedContextOptions configures focused context generation.
type FocusedContextOptions struct {
	Focus   string
	BaseDir string
	RepoMap *repomap.Map
}

// FocusedContextResult contains the focused context for a file or symbol.
type FocusedContextResult struct {
	Target     FocusedTarget
	Complexity *ComplexityInfo
	SATD       []SATDItem
	Candidates []FocusedCandidate // For ambiguous matches
}

// FocusedTarget identifies the resolved target.
type FocusedTarget struct {
	Type   string         // "file" or "symbol"
	Path   string         // For files
	Symbol *FocusedSymbol // For symbols
}

// FocusedSymbol contains symbol details.
type FocusedSymbol struct {
	Name string
	Kind string
	File string
	Line int
}

// FocusedCandidate represents an ambiguous match option.
type FocusedCandidate struct {
	Path string
	Name string
	File string
	Line int
	Kind string
}

// ComplexityInfo contains complexity metrics.
type ComplexityInfo struct {
	CyclomaticTotal int
	CognitiveTotal  int
	TopFunctions    []FunctionComplexity
}

// FunctionComplexity contains per-function complexity.
type FunctionComplexity struct {
	Name       string
	Line       int
	Cyclomatic int
	Cognitive  int
}

// SATDItem contains a technical debt marker.
type SATDItem struct {
	Line     int
	Type     string
	Content  string
	Severity string
}

// FocusedContext generates deep context for a specific file or symbol.
func (s *Service) FocusedContext(ctx context.Context, opts FocusedContextOptions) (*FocusedContextResult, error) {
	baseDir := opts.BaseDir
	if baseDir == "" {
		baseDir = "."
	}

	// Resolve the focus target
	locatorResult, err := locator.Locate(opts.Focus, opts.RepoMap, locator.WithBaseDir(baseDir))
	if err != nil {
		// For ambiguous matches, return result with candidates
		if err == locator.ErrAmbiguousMatch && locatorResult != nil {
			candidates := make([]FocusedCandidate, len(locatorResult.Candidates))
			for i, c := range locatorResult.Candidates {
				candidates[i] = FocusedCandidate{
					Path: c.Path,
					Name: c.Name,
					File: c.File,
					Line: c.Line,
					Kind: c.Kind,
				}
			}
			return &FocusedContextResult{Candidates: candidates}, err
		}
		return nil, err
	}

	result := &FocusedContextResult{}

	switch locatorResult.Type {
	case locator.TargetFile:
		return s.focusedContextForFile(ctx, locatorResult.Path, opts)
	case locator.TargetSymbol:
		return s.focusedContextForSymbol(ctx, locatorResult.Symbol, opts)
	}

	return result, nil
}

func (s *Service) focusedContextForFile(ctx context.Context, path string, opts FocusedContextOptions) (*FocusedContextResult, error) {
	result := &FocusedContextResult{
		Target: FocusedTarget{
			Type: "file",
			Path: path,
		},
	}

	// Get complexity metrics
	cxAnalyzer := complexity.New()
	defer cxAnalyzer.Close()

	cxResult, err := cxAnalyzer.Analyze(ctx, []string{path}, source.NewFilesystem())
	if err == nil && cxResult != nil {
		cxInfo := &ComplexityInfo{}
		for _, f := range cxResult.Files {
			if f.Path == path {
				for _, fn := range f.Functions {
					cxInfo.CyclomaticTotal += int(fn.Metrics.Cyclomatic)
					cxInfo.CognitiveTotal += int(fn.Metrics.Cognitive)
					cxInfo.TopFunctions = append(cxInfo.TopFunctions, FunctionComplexity{
						Name:       fn.Name,
						Line:       int(fn.StartLine),
						Cyclomatic: int(fn.Metrics.Cyclomatic),
						Cognitive:  int(fn.Metrics.Cognitive),
					})
				}
				break
			}
		}
		result.Complexity = cxInfo
	}

	// Get SATD markers
	satdAnalyzer := satd.New()
	defer satdAnalyzer.Close()

	satdResult, err := satdAnalyzer.Analyze(ctx, []string{path}, source.NewFilesystem())
	if err == nil && satdResult != nil {
		for _, item := range satdResult.Items {
			if item.File == path || filepath.Base(item.File) == filepath.Base(path) {
				result.SATD = append(result.SATD, SATDItem{
					Line:     int(item.Line),
					Type:     item.Marker,
					Content:  item.Description,
					Severity: string(item.Severity),
				})
			}
		}
	}

	return result, nil
}

func (s *Service) focusedContextForSymbol(ctx context.Context, sym *locator.Symbol, opts FocusedContextOptions) (*FocusedContextResult, error) {
	result := &FocusedContextResult{
		Target: FocusedTarget{
			Type: "symbol",
			Symbol: &FocusedSymbol{
				Name: sym.Name,
				Kind: sym.Kind,
				File: sym.File,
				Line: sym.Line,
			},
		},
	}

	// Get complexity for the symbol's file
	cxAnalyzer := complexity.New()
	defer cxAnalyzer.Close()

	cxResult, err := cxAnalyzer.Analyze(ctx, []string{sym.File}, source.NewFilesystem())
	if err == nil && cxResult != nil {
		cxInfo := &ComplexityInfo{}
		for _, f := range cxResult.Files {
			for _, fn := range f.Functions {
				// Find the specific function
				if fn.Name == sym.Name && int(fn.StartLine) == sym.Line {
					cxInfo.CyclomaticTotal = int(fn.Metrics.Cyclomatic)
					cxInfo.CognitiveTotal = int(fn.Metrics.Cognitive)
					cxInfo.TopFunctions = []FunctionComplexity{{
						Name:       fn.Name,
						Line:       int(fn.StartLine),
						Cyclomatic: int(fn.Metrics.Cyclomatic),
						Cognitive:  int(fn.Metrics.Cognitive),
					}}
					break
				}
			}
		}
		result.Complexity = cxInfo
	}

	return result, nil
}
