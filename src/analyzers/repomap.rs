//! Repository map (PageRank-ranked symbols) analyzer.
//!
//! Generates a PageRank-ranked index of repository symbols (functions and methods)
//! optimized for LLM context. Higher-ranked symbols are more "central" in the codebase
//! based on call relationships. The calls come from the shared index of the repository.

use std::collections::HashSet;
use std::path::Path;

use chrono::Utc;
use ignore::WalkBuilder;
use petgraph::graph::DiGraph;
use petgraph::Direction;
use serde::{Deserialize, Serialize};

use crate::core::{is_test_file, AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::index::build::CodeIndex;
use crate::index::graph::{pagerank, PageRankOptions};
use crate::index::model::SymbolKind as IndexKind;
use crate::index::view::{CallView, DEFAULT_MIN_BASIS};

/// Repomap analyzer configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// PageRank damping factor (default: 0.85).
    pub damping: f64,
    /// PageRank max iterations (default: 100).
    pub max_iterations: usize,
    /// PageRank convergence tolerance (default: 1e-6).
    pub tolerance: f64,
    /// Maximum symbols to return (0 = all).
    pub max_symbols: usize,
    /// Skip test files.
    pub skip_test_files: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            damping: 0.85,
            max_iterations: 100,
            tolerance: 1e-6,
            max_symbols: 0,
            skip_test_files: true,
        }
    }
}

/// Repomap analyzer.
pub struct Analyzer {
    config: Config,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }

    pub fn with_config(config: Config) -> Self {
        Self { config }
    }

    pub fn with_damping(mut self, damping: f64) -> Self {
        self.config.damping = damping;
        self
    }

    pub fn with_max_symbols(mut self, max: usize) -> Self {
        self.config.max_symbols = max;
        self
    }

    pub fn with_skip_test_files(mut self, skip: bool) -> Self {
        self.config.skip_test_files = skip;
        self
    }

    /// Analyze a repository and generate a PageRank-ranked symbol map.
    pub fn analyze_repo(&self, repo_path: &Path) -> Result<Analysis> {
        let files: Vec<_> = WalkBuilder::new(repo_path)
            .hidden(false)
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
            .filter(|e| Language::detect(e.path()).is_some())
            .map(|e| e.into_path())
            .collect();

        Ok(self.analyze_index(&CodeIndex::from_files(repo_path, &files)))
    }

    /// Rank the functions and the methods of an index.
    ///
    /// An edge of the graph is a call that the index resolved with the basis
    /// `DEFAULT_MIN_BASIS` or a stronger basis.
    pub fn analyze_index(&self, index: &CodeIndex) -> Analysis {
        let view = CallView::from_index_where(index, |file| {
            !self.config.skip_test_files || !is_test_file(Path::new(file))
        });

        let mut graph: DiGraph<usize, ()> = DiGraph::new();
        let mut node_of = vec![None; view.symbols.len()];
        for (position, symbol) in view.symbols.iter().enumerate() {
            if is_callable(symbol.kind) {
                node_of[position] = Some(graph.add_node(position));
            }
        }
        for (caller, callee) in view.edges(DEFAULT_MIN_BASIS) {
            if let (Some(from), Some(to)) = (node_of[caller], node_of[callee]) {
                graph.add_edge(from, to, ());
            }
        }

        let scores = pagerank(
            &graph,
            &PageRankOptions {
                damping: self.config.damping,
                max_iterations: self.config.max_iterations,
                tolerance: self.config.tolerance,
            },
        );

        let mut output_symbols: Vec<SymbolEntry> = graph
            .node_indices()
            .map(|node| {
                let symbol = &view.symbols[graph[node]];
                SymbolEntry {
                    name: symbol.name.clone(),
                    qualified_name: symbol.qualified_name.clone(),
                    kind: if symbol.kind == IndexKind::Method {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    },
                    file: symbol.file.clone(),
                    line: symbol.line,
                    signature: symbol.signature.clone(),
                    pagerank: scores[node.index()],
                    in_degree: graph.edges_directed(node, Direction::Incoming).count(),
                    out_degree: graph.edges_directed(node, Direction::Outgoing).count(),
                }
            })
            .collect();

        // The view lists the symbols in file and position order, and the sort
        // keeps that order for symbols with equal rank.
        output_symbols.sort_by(|a, b| b.pagerank.total_cmp(&a.pagerank));

        if self.config.max_symbols > 0 && output_symbols.len() > self.config.max_symbols {
            output_symbols.truncate(self.config.max_symbols);
        }

        let summary = calculate_summary(&output_symbols);

        Analysis {
            generated_at: Utc::now().to_rfc3339(),
            symbols: output_symbols,
            summary,
        }
    }
}

fn is_callable(kind: IndexKind) -> bool {
    matches!(
        kind,
        IndexKind::Function | IndexKind::Method | IndexKind::Constructor
    )
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "repomap"
    }

    fn description(&self) -> &'static str {
        "Generate PageRank-ranked symbol index for LLM context"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        Ok(self.analyze_index(&*ctx.code_index()?))
    }
}

/// Repomap analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub generated_at: String,
    pub symbols: Vec<SymbolEntry>,
    pub summary: Summary,
}

/// A symbol entry in the repo map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolEntry {
    pub name: String,
    /// The key of the symbol in the shared index: `rel/path#Container.name`.
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: u32,
    pub signature: String,
    pub pagerank: f64,
    pub in_degree: usize,
    pub out_degree: usize,
}

/// Symbol kinds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Enum,
    Constant,
}

/// Summary statistics for the repo map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub total_symbols: usize,
    pub total_files: usize,
    pub avg_pagerank: f64,
    pub max_pagerank: f64,
    pub avg_connections: f64,
}

/// Calculate summary statistics from symbols.
fn calculate_summary(symbols: &[SymbolEntry]) -> Summary {
    if symbols.is_empty() {
        return Summary {
            total_symbols: 0,
            total_files: 0,
            avg_pagerank: 0.0,
            max_pagerank: 0.0,
            avg_connections: 0.0,
        };
    }

    let mut files = HashSet::new();
    let mut total_pr = 0.0;
    let mut max_pr = 0.0_f64;
    let mut total_connections = 0;

    for sym in symbols {
        files.insert(&sym.file);
        total_pr += sym.pagerank;
        max_pr = max_pr.max(sym.pagerank);
        total_connections += sym.in_degree + sym.out_degree;
    }

    Summary {
        total_symbols: symbols.len(),
        total_files: files.len(),
        avg_pagerank: total_pr / symbols.len() as f64,
        max_pagerank: max_pr,
        avg_connections: total_connections as f64 / symbols.len() as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_rust_fixture() -> TempDir {
        let dir = TempDir::new().unwrap();

        // The calls are in one file, thus the basis of each call is the file.
        fs::write(
            dir.path().join("main.rs"),
            r#"
fn a() {
    b();
}

fn b() {
    c();
}

fn c() {
    // leaf
}
"#,
        )
        .unwrap();

        dir
    }

    #[test]
    fn test_characterization_repomap_output() {
        // Characterization test: pins current repomap output for a→b→c chain.
        let dir = create_rust_fixture();
        let path = dir.path();

        let analyzer = Analyzer::new().with_skip_test_files(false);
        let result = analyzer.analyze_repo(path).unwrap();

        // Should have 3 symbols: a, b, c
        assert_eq!(result.symbols.len(), 3);

        // Collect names for checking
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a"), "symbols should include a");
        assert!(names.contains(&"b"), "symbols should include b");
        assert!(names.contains(&"c"), "symbols should include c");

        // By PageRank ordering, c (receives calls from b) should be >= b (receives from a) >= a
        // c is called by b which is called by a → c has highest in_degree
        // Verify files are reported correctly
        let by_name: HashMap<&str, &SymbolEntry> = result
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s))
            .collect();

        let sym_a = by_name["a"];
        let sym_b = by_name["b"];
        let sym_c = by_name["c"];

        // out_degree: a→b (1), b→c (1), c→nothing (0)
        assert_eq!(sym_a.out_degree, 1, "a should call b");
        assert_eq!(sym_b.out_degree, 1, "b should call c");
        assert_eq!(sym_c.out_degree, 0, "c calls nothing");

        // in_degree: a←nothing (0), b←a (1), c←b (1)
        assert_eq!(sym_a.in_degree, 0, "nobody calls a");
        assert_eq!(sym_b.in_degree, 1, "a calls b");
        assert_eq!(sym_c.in_degree, 1, "b calls c");

        // PageRank ordering: c should rank highest (receives rank from b which gets from a)
        // At minimum: c.pagerank >= a.pagerank
        assert!(
            sym_c.pagerank >= sym_a.pagerank,
            "c should rank >= a by pagerank"
        );

        // Verify qualified_name is present
        assert_eq!(sym_a.qualified_name, "main.rs#a");
        assert_eq!(sym_b.qualified_name, "main.rs#b");
        assert_eq!(sym_c.qualified_name, "main.rs#c");
    }

    #[test]
    fn test_a_call_that_only_the_name_resolves_is_not_an_edge() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.rs"), "fn a() {\n    b();\n}\n").unwrap();
        fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();

        let result = Analyzer::new().analyze_repo(dir.path()).unwrap();

        assert!(result.symbols.iter().all(|symbol| symbol.out_degree == 0));
        assert!(result.symbols.iter().all(|symbol| symbol.in_degree == 0));
    }

    #[test]
    fn test_method_has_the_kind_method() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("a.rs"),
            "struct Shape;
impl Shape {
    fn area(&self) {}
}
fn free() {}
",
        )
        .unwrap();

        let result = Analyzer::new().analyze_repo(dir.path()).unwrap();

        let kind_of = |name: &str| {
            result
                .symbols
                .iter()
                .find(|symbol| symbol.name == name)
                .map(|symbol| symbol.kind)
        };
        assert_eq!(kind_of("area"), Some(SymbolKind::Method));
        assert_eq!(kind_of("free"), Some(SymbolKind::Function));
        assert!(
            result.symbols.iter().all(|symbol| symbol.name != "Shape"),
            "a type is not a callable symbol of the map"
        );
    }

    #[test]
    fn test_same_name_in_two_types_of_one_file_has_two_entries() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("a.rs"),
            "struct A;
struct B;
impl A {
    fn run(&self) {}
}
impl B {
    fn run(&self) {}
}
",
        )
        .unwrap();

        let result = Analyzer::new().analyze_repo(dir.path()).unwrap();

        let keys: Vec<&str> = result
            .symbols
            .iter()
            .map(|symbol| symbol.qualified_name.as_str())
            .collect();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"a.rs#A.run"), "{keys:?}");
        assert!(keys.contains(&"a.rs#B.run"), "{keys:?}");
    }

    #[test]
    fn test_symbols_with_equal_rank_are_in_file_and_line_order() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("z.rs"),
            "fn zed() {}
",
        )
        .unwrap();
        fs::write(
            dir.path().join("a.rs"),
            "fn first() {}
fn second() {}
",
        )
        .unwrap();

        let result = Analyzer::new().analyze_repo(dir.path()).unwrap();

        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["first", "second", "zed"]);
    }

    #[test]
    fn test_calls_in_test_files_are_not_edges_when_test_files_are_skipped() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "pub fn target() {}
",
        )
        .unwrap();
        fs::write(
            dir.path().join("lib_test.rs"),
            "fn check() { crate::target(); }
",
        )
        .unwrap();

        let skipping = Analyzer::new().analyze_repo(dir.path()).unwrap();
        let including = Analyzer::new()
            .with_skip_test_files(false)
            .analyze_repo(dir.path())
            .unwrap();

        let in_degree = |analysis: &Analysis| {
            analysis
                .symbols
                .iter()
                .find(|symbol| symbol.name == "target")
                .map(|symbol| symbol.in_degree)
        };
        assert_eq!(skipping.symbols.len(), 1);
        assert_eq!(in_degree(&skipping), Some(0));
        assert_eq!(in_degree(&including), Some(1));
    }

    #[test]
    fn test_analyzer_reads_through_the_content_source() {
        use crate::config::Config as AppConfig;
        use crate::core::{FileSet, FilesystemSource};
        use std::sync::Arc;

        let current = TempDir::new().unwrap();
        let historical = TempDir::new().unwrap();
        fs::write(
            current.path().join("code.rs"),
            "fn current_version() {}
",
        )
        .unwrap();
        fs::write(
            historical.path().join("code.rs"),
            "fn historical_version() {}
",
        )
        .unwrap();
        let files =
            FileSet::from_files(current.path().to_path_buf(), vec![PathBuf::from("code.rs")]);
        let config = AppConfig::default();
        let source = Arc::new(FilesystemSource::new(historical.path()));
        let ctx =
            AnalysisContext::new(&files, &config, Some(current.path())).with_content_source(source);

        let result = Analyzer::new().analyze(&ctx).unwrap();

        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["historical_version"]);
    }

    #[test]
    fn test_analysis_serialization() {
        let analysis = Analysis {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            symbols: vec![SymbolEntry {
                name: "test".to_string(),
                qualified_name: "test.rs#test".to_string(),
                kind: SymbolKind::Function,
                file: "test.rs".to_string(),
                line: 1,
                signature: "fn test()".to_string(),
                pagerank: 0.5,
                in_degree: 1,
                out_degree: 2,
            }],
            summary: Summary {
                total_symbols: 1,
                total_files: 1,
                avg_pagerank: 0.5,
                max_pagerank: 0.5,
                avg_connections: 3.0,
            },
        };

        let json = serde_json::to_string(&analysis).unwrap();
        assert!(json.contains("\"test\""));
        assert!(json.contains("\"Function\""));

        let parsed: Analysis = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.symbols.len(), 1);
        assert_eq!(parsed.symbols[0].name, "test");
        assert_eq!(parsed.symbols[0].qualified_name, "test.rs#test");
    }

    #[test]
    fn test_calculate_summary_single_file_symbols() {
        let symbols = vec![
            SymbolEntry {
                name: "a".to_string(),
                qualified_name: "file1.rs#a".to_string(),
                kind: SymbolKind::Function,
                file: "file1.rs".to_string(),
                line: 1,
                signature: String::new(),
                pagerank: 0.5,
                in_degree: 0,
                out_degree: 0,
            },
            SymbolEntry {
                name: "b".to_string(),
                qualified_name: "file1.rs#b".to_string(),
                kind: SymbolKind::Function,
                file: "file1.rs".to_string(),
                line: 10,
                signature: String::new(),
                pagerank: 0.3,
                in_degree: 1,
                out_degree: 2,
            },
        ];

        let summary = calculate_summary(&symbols);
        assert_eq!(summary.total_symbols, 2);
        // Both in same file
        assert_eq!(summary.total_files, 1);
    }

    #[test]
    fn test_max_symbols_truncation() {
        let analyzer = Analyzer::new().with_max_symbols(2);
        // Verify config is set
        assert_eq!(analyzer.config.max_symbols, 2);
    }

    // ===== is_test_file tests =====

    #[test]
    fn test_is_test_file_known_patterns() {
        // Patterns that SHOULD be identified as test files.
        // Note: the `/test` prefix check requires a leading `/`, so use paths
        // with a directory component to trigger that branch.
        let positives = [
            "src/foo_test.go",     // _test. match
            "src/tests/bar.rs",    // /test match
            "src/foo.test.ts",     // .test.ts match
            "src/foo.spec.ts",     // .spec.ts match
            "src/foo.test.js",     // .test.js match
            "src/foo.spec.js",     // .spec.js match
            "src/foo.test.tsx",    // .test.tsx match (new)
            "src/foo.spec.tsx",    // .spec.tsx match (new)
            "src/foo.test.jsx",    // .test.jsx match (new)
            "src/foo.spec.jsx",    // .spec.jsx match (new)
            "src/test_helpers.rs", // test_ match
        ];
        for p in &positives {
            assert!(is_test_file(Path::new(p)), "expected {p} to be a test file");
        }
    }

    #[test]
    fn test_is_test_file_non_test_paths() {
        let negatives = [
            "src/main.rs",
            "src/foo.ts",
            "src/foo.tsx",
            "src/foo.jsx",
            "src/foo.js",
        ];
        for p in &negatives {
            assert!(
                !is_test_file(Path::new(p)),
                "expected {p} NOT to be a test file"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_is_test_file_windows_backslash() {
        // On Windows paths may use backslash separators.
        assert!(is_test_file(Path::new("src\\tests\\foo.rs")));
        assert!(is_test_file(Path::new("src\\foo.test.tsx")));
    }
}
