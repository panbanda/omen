//! Impact (blast-radius) analyzer.
//!
//! Given a symbol name, finds all transitive callers and callees up to a
//! configurable BFS depth, and reports the affected files. The name must
//! select one definition.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::Result;
use crate::index::build::CodeIndex;
use crate::index::view::CallView;

/// A single symbol reference in an impact report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactSymbol {
    pub qualified_name: String,
    pub file: String,
    pub line: u32,
}

impl ImpactSymbol {
    pub fn of(symbol: &crate::index::view::ViewSymbol) -> Self {
        Self {
            qualified_name: symbol.qualified_name.clone(),
            file: symbol.file.clone(),
            line: symbol.line,
        }
    }
}

/// One BFS level in the impact report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactLevel {
    pub depth: usize,
    pub symbols: Vec<ImpactSymbol>,
}

/// The full result of an impact analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactReport {
    /// Evidence counts across all input files, not just the traversed subgraph.
    #[serde(default)]
    pub resolution_summary: crate::index::view::ResolutionSummary,
    /// The query symbol name.
    pub symbol: String,
    /// The key of the one root definition.
    pub resolved: Vec<String>,
    /// The declarations without a body of the same function, such as a C
    /// prototype. They are roots of the traversal also.
    pub declarations: Vec<ImpactSymbol>,
    /// Callers by BFS level (empty when direction is Callees).
    pub callers: Vec<ImpactLevel>,
    /// Callees by BFS level (empty when direction is Callers).
    pub callees: Vec<ImpactLevel>,
    /// All affected files: deduped, sorted, including the resolved symbols' own files.
    pub files_affected: Vec<String>,
    pub total_callers: usize,
    pub total_callees: usize,
    /// The security facts of the root. The caller adds them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<crate::index::security::SymbolSecurity>,
}

/// Direction of impact traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Callers,
    Callees,
    Both,
}

/// Run the impact analysis.
///
/// The symbol must select one definition. An unknown name or an ambiguous
/// name gives [`crate::core::Error::SymbolLookup`].
pub fn analyze(
    root: &Path,
    files: &[PathBuf],
    symbol: &str,
    depth: usize,
    direction: Direction,
) -> Result<ImpactReport> {
    let index = CallView::from_index(&CodeIndex::from_files(root, files));
    analyze_view(
        &index,
        symbol,
        None,
        depth,
        direction,
        crate::index::view::DEFAULT_MIN_BASIS,
    )
}

/// Run the impact analysis on a view of the shared index. `start_line`
/// selects one of the definitions of the name.
pub fn analyze_view(
    index: &CallView,
    symbol: &str,
    start_line: Option<u32>,
    depth: usize,
    direction: Direction,
    min_basis: crate::index::model::ResolutionBasis,
) -> Result<ImpactReport> {
    let selection = crate::symbol::select(index, symbol, start_line)?;
    let root = selection.definition;
    // A call can resolve to a declaration of the function, thus each
    // declaration is a root of the traversal also.
    let root_indices: Vec<usize> = std::iter::once(root)
        .chain(selection.declarations.iter().copied())
        .collect();

    let resolved = vec![index.symbols[root].qualified_name.clone()];

    // BFS callers
    let caller_levels = if direction == Direction::Callers || direction == Direction::Both {
        index.callers(&root_indices, depth, min_basis)
    } else {
        vec![]
    };

    // BFS callees
    let callee_levels = if direction == Direction::Callees || direction == Direction::Both {
        index.callees(&root_indices, depth, min_basis)
    } else {
        vec![]
    };

    // Convert levels to ImpactLevel structs
    let to_impact_levels = |raw_levels: Vec<Vec<usize>>| -> Vec<ImpactLevel> {
        raw_levels
            .into_iter()
            .enumerate()
            .map(|(d, idxs)| {
                let symbols = idxs
                    .iter()
                    .map(|&i| ImpactSymbol {
                        qualified_name: index.symbols[i].qualified_name.clone(),
                        file: index.symbols[i].file.clone(),
                        line: index.symbols[i].line,
                    })
                    .collect();
                ImpactLevel {
                    depth: d + 1,
                    symbols,
                }
            })
            .collect()
    };

    let caller_impact = to_impact_levels(caller_levels);
    let callee_impact = to_impact_levels(callee_levels);

    let total_callers: usize = caller_impact.iter().map(|l| l.symbols.len()).sum();
    let total_callees: usize = callee_impact.iter().map(|l| l.symbols.len()).sum();

    // Build files_affected: deduped, sorted; includes resolved symbols' own files
    let mut files_set: HashSet<String> = HashSet::new();
    for &i in &root_indices {
        files_set.insert(index.symbols[i].file.clone());
    }
    for level in &caller_impact {
        for sym in &level.symbols {
            files_set.insert(sym.file.clone());
        }
    }
    for level in &callee_impact {
        for sym in &level.symbols {
            files_set.insert(sym.file.clone());
        }
    }
    let mut files_affected: Vec<String> = files_set.into_iter().collect();
    files_affected.sort();

    Ok(ImpactReport {
        resolution_summary: index.resolution_summary(),
        symbol: symbol.to_string(),
        resolved,
        declarations: selection
            .declarations
            .iter()
            .map(|&position| ImpactSymbol::of(&index.symbols[position]))
            .collect(),
        callers: caller_impact,
        callees: callee_impact,
        files_affected,
        total_callers,
        total_callees,
        security: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn rust_chain() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("main.rs"),
            "mod b;\nmod c;\nfn a() { b::b(); }\n",
        )
        .unwrap();
        fs::write(dir.path().join("b.rs"), "pub fn b() { crate::c::c(); }\n").unwrap();
        fs::write(dir.path().join("c.rs"), "pub fn c() {}\n").unwrap();
        dir
    }

    fn all_files(dir: &TempDir) -> Vec<PathBuf> {
        vec!["main.rs", "b.rs", "c.rs"]
            .into_iter()
            .map(|f| dir.path().join(f))
            .collect()
    }

    #[test]
    fn test_callers_depth_semantics() {
        let dir = rust_chain();
        let files = all_files(&dir);
        let report = analyze(dir.path(), &files, "c", 2, Direction::Callers).unwrap();
        assert_eq!(report.symbol, "c");
        assert!(!report.resolved.is_empty());
        assert_eq!(report.callers.len(), 2);
        assert_eq!(report.callers[0].depth, 1);
        assert_eq!(report.callers[0].symbols.len(), 1);
        assert!(report.callers[0].symbols[0]
            .qualified_name
            .contains("b.rs#b"));
        assert_eq!(report.callers[1].depth, 2);
        assert_eq!(report.callers[1].symbols.len(), 1);
        assert!(report.callers[1].symbols[0]
            .qualified_name
            .contains("main.rs#a"));
        // callees should be empty for Callers direction
        assert!(report.callees.is_empty());
    }

    #[test]
    fn test_direction_filtering_callees_only() {
        let dir = rust_chain();
        let files = all_files(&dir);
        let report = analyze(dir.path(), &files, "a", 2, Direction::Callees).unwrap();
        assert!(!report.callees.is_empty());
        assert!(report.callers.is_empty());
        assert_eq!(report.total_callers, 0);
    }

    #[test]
    fn test_direction_both() {
        let dir = rust_chain();
        let files = all_files(&dir);
        let report = analyze(dir.path(), &files, "b", 2, Direction::Both).unwrap();
        // b is called by a, calls c
        assert!(!report.callers.is_empty());
        assert!(!report.callees.is_empty());
    }

    fn lookup_failure(result: Result<ImpactReport>) -> crate::symbol::LookupFailure {
        match result {
            Err(crate::core::Error::SymbolLookup(failure)) => *failure,
            other => panic!("expected a lookup failure: {other:?}"),
        }
    }

    #[test]
    fn test_an_unknown_symbol_gives_the_near_names() {
        let dir = rust_chain();
        let files = all_files(&dir);
        let failure = lookup_failure(analyze(
            dir.path(),
            &files,
            "bb_unknown",
            2,
            Direction::Both,
        ));
        let crate::symbol::LookupFailure::NotFound { suggestions, .. } = failure else {
            panic!("expected not found: {failure:?}");
        };
        // `bb_unknown` contains the name `b`.
        assert_eq!(suggestions, ["b"]);
    }

    /// Two definitions of `helper`, each with one caller.
    fn two_helpers() -> (TempDir, Vec<PathBuf>) {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("x.py"),
            "def helper():\n    pass\n\ndef use_x():\n    helper()\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("y.py"),
            "def helper():\n    pass\n\ndef use_y():\n    helper()\n",
        )
        .unwrap();
        let files = vec![dir.path().join("x.py"), dir.path().join("y.py")];
        (dir, files)
    }

    #[test]
    fn test_an_ambiguous_seed_gives_the_choices_and_no_report() {
        let (dir, files) = two_helpers();
        let failure = lookup_failure(analyze(dir.path(), &files, "helper", 2, Direction::Both));
        let crate::symbol::LookupFailure::Ambiguous { choices, .. } = failure else {
            panic!("expected ambiguous: {failure:?}");
        };
        let keys: Vec<&str> = choices.iter().map(|choice| choice.name.as_str()).collect();
        assert_eq!(keys, ["x.py#helper", "y.py#helper"]);
    }

    #[test]
    fn test_the_key_of_a_choice_selects_one_root() {
        let (dir, files) = two_helpers();
        let report = analyze(dir.path(), &files, "y.py#helper", 1, Direction::Callers).unwrap();
        assert_eq!(report.resolved, ["y.py#helper"]);
        assert_eq!(report.callers[0].symbols[0].qualified_name, "y.py#use_y");
        assert_eq!(report.files_affected, ["y.py"]);
    }

    #[test]
    fn test_a_c_prototype_is_a_root_with_its_definition() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("area.h"), "int area(void);\n").unwrap();
        fs::write(
            dir.path().join("area.c"),
            "#include \"area.h\"\nint area(void) { return 1; }\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("main.c"),
            "#include \"area.h\"\nint main(void) { return area(); }\n",
        )
        .unwrap();
        let files = vec![
            dir.path().join("area.h"),
            dir.path().join("area.c"),
            dir.path().join("main.c"),
        ];
        let report = analyze(dir.path(), &files, "area", 1, Direction::Callers).unwrap();
        assert_eq!(report.resolved, ["area.c#area"]);
        assert_eq!(
            report.declarations,
            [ImpactSymbol {
                qualified_name: "area.h#area".to_string(),
                file: "area.h".to_string(),
                line: 1,
            }]
        );
        assert_eq!(report.callers[0].symbols[0].qualified_name, "main.c#main");
        assert_eq!(report.files_affected, ["area.c", "area.h", "main.c"]);
    }

    #[test]
    fn test_a_file_and_a_start_line_select_one_root() {
        let (dir, files) = two_helpers();
        let view = CallView::from_index(&CodeIndex::from_files(dir.path(), &files));
        let report = analyze_view(
            &view,
            "x.py:helper",
            Some(1),
            1,
            Direction::Callers,
            crate::index::view::DEFAULT_MIN_BASIS,
        )
        .unwrap();
        assert_eq!(report.resolved, ["x.py#helper"]);
    }

    #[test]
    fn test_cycle_safety() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.rs"), "mod b;\nfn a() { b::b(); }\n").unwrap();
        fs::write(dir.path().join("b.rs"), "pub fn b() { crate::a(); }\n").unwrap();
        let files = vec![dir.path().join("main.rs"), dir.path().join("b.rs")];
        // Should not hang or panic
        let report = analyze(dir.path(), &files, "a", 5, Direction::Both).unwrap();
        assert!(!report.resolved.is_empty());
    }

    #[test]
    fn test_files_affected_deduped_sorted_includes_own() {
        let dir = rust_chain();
        let files = all_files(&dir);
        let report = analyze(dir.path(), &files, "c", 2, Direction::Callers).unwrap();
        // c's own file should be in files_affected
        assert!(report.files_affected.iter().any(|f| f.contains("c.rs")));
        // Should be sorted
        let is_sorted = report.files_affected.windows(2).all(|w| w[0] <= w[1]);
        assert!(is_sorted, "files_affected must be sorted");
        // No duplicates
        let unique: HashSet<_> = report.files_affected.iter().collect();
        assert_eq!(unique.len(), report.files_affected.len());
    }

    #[test]
    fn test_depth_zero_empty_levels() {
        let dir = rust_chain();
        let files = all_files(&dir);
        let report = analyze(dir.path(), &files, "b", 0, Direction::Both).unwrap();
        assert!(report.callers.is_empty());
        assert!(report.callees.is_empty());
    }

    #[test]
    fn test_serialization() {
        let dir = rust_chain();
        let files = all_files(&dir);
        let report = analyze(dir.path(), &files, "b", 1, Direction::Both).unwrap();
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"symbol\""));
        assert!(json.contains("\"callers\""));
        assert!(json.contains("\"callees\""));
        let parsed: ImpactReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.symbol, "b");
    }

    // Multi-language tests: a-calls-b fixture in each language
    fn run_multi_lang_test(dir: &TempDir, a_file: &str, a_src: &str, b_file: &str, b_src: &str) {
        fs::write(dir.path().join(a_file), a_src).unwrap();
        fs::write(dir.path().join(b_file), b_src).unwrap();
        let files = vec![dir.path().join(a_file), dir.path().join(b_file)];
        let report = analyze(
            dir.path(),
            &files,
            &format!("{b_file}:b"),
            1,
            Direction::Callers,
        )
        .unwrap();
        // b should be resolved
        assert!(
            !report.resolved.is_empty(),
            "b not found in {a_file}/{b_file}"
        );
        // callers of b should include a
        let callers_all: Vec<&str> = report
            .callers
            .iter()
            .flat_map(|l| l.symbols.iter())
            .map(|s| s.qualified_name.as_str())
            .collect();
        assert!(
            callers_all
                .iter()
                .any(|q| q.rsplit(['#', '.']).next() == Some("a")),
            "expected a to be a caller of b in {a_file}/{b_file}, got: {callers_all:?}"
        );
    }

    #[test]
    fn test_multi_lang_rust() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "main.rs",
            "mod b;\nfn a() { b::b(); }\n",
            "b.rs",
            "pub fn b() {}\n",
        );
    }

    #[test]
    fn test_multi_lang_typescript() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "a.ts",
            "import { b } from './b';\nfunction a() { b(); }\n",
            "b.ts",
            "export function b() {}\n",
        );
    }

    #[test]
    fn test_multi_lang_python() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "a.py",
            "from b import b\n\n\ndef a():\n    b()\n",
            "b.py",
            "def b():\n    pass\n",
        );
    }

    #[test]
    fn test_multi_lang_go() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "a.go",
            "package main\nfunc a() { b() }\n",
            "b.go",
            "package main\nfunc b() {}\n",
        );
    }

    #[test]
    fn test_multi_lang_javascript() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "a.js",
            "import { b } from './b';\nfunction a() { b(); }\n",
            "b.js",
            "export function b() {}\n",
        );
    }

    #[test]
    fn test_multi_lang_tsx() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "a.tsx",
            "import { b } from './b';\nfunction a() { b(); }\n",
            "b.tsx",
            "export function b() {}\n",
        );
    }

    /// Java methods are inside classes; the repomap extractor handles methods.
    #[test]
    fn test_multi_lang_java() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "A.java",
            "class A { void a() { new B().b(); } }\n",
            "B.java",
            "class B { void b() {} }\n",
        );
    }

    /// C# invocation_expression via member_access_expression chain.
    /// `new B().b()` — the `b` identifier lives inside a member_access_expression
    /// that is the function child of the invocation_expression.
    #[test]
    fn test_multi_lang_csharp() {
        let dir = TempDir::new().unwrap();
        // Use both member-access call (new B().b()) and a bare call within the
        // same class so we cover both code paths.
        let a_src = "class A { void a() { new B().b(); } }\n";
        let b_src = "class B { void b() {} }\n";
        run_multi_lang_test(&dir, "A.cs", a_src, "B.cs", b_src);
    }

    /// C function name extraction via declarator chain.
    /// tree-sitter-c nests the function name inside function_declarator > identifier.
    #[test]
    fn test_multi_lang_c() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "a.c",
            "void b(void);\nvoid a(void) { b(); }\n",
            "b.c",
            "void b(void) {}\n",
        );
    }

    /// C++ function name extraction via declarator chain (same as C).
    #[test]
    fn test_multi_lang_cpp() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "a.cpp",
            "void b();\nvoid a() { b(); }\n",
            "b.cpp",
            "void b() {}\n",
        );
    }

    #[test]
    fn test_multi_lang_ruby() {
        let dir = TempDir::new().unwrap();
        // Use explicit parens so tree-sitter parses the invocation as a `call`
        // node rather than a bare identifier.
        run_multi_lang_test(
            &dir,
            "a.rb",
            "require_relative 'b'\ndef a\n  b()\nend\n",
            "b.rb",
            "def b\nend\n",
        );
    }

    /// PHP `function_call_expression` uses a `name` child (PHP-grammar-specific).
    /// The call-name extractor now handles this child kind.
    #[test]
    fn test_multi_lang_php() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "a.php",
            "<?php\nrequire 'b.php';\nfunction a() { b(); }\n",
            "b.php",
            "<?php\nfunction b() {}\n",
        );
    }

    /// Bash: bare command invocations (`command` > `command_name` > `word`)
    /// are mapped to call edges so callers can be traced.
    #[test]
    fn test_multi_lang_bash() {
        let dir = TempDir::new().unwrap();
        run_multi_lang_test(
            &dir,
            "a.sh",
            "source ./b.sh\na() { b; }\n",
            "b.sh",
            "b() { :; }\n",
        );
    }
}
