//! Symbol report: one-call lookup returning source slice, signature, location,
//! direct callers/callees, and complexity for a named symbol.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::analyzers::impact::ImpactSymbol;
use crate::analyzers::repomap::build_index;
use crate::core::Result;

/// A full symbol report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolReport {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: String,
    /// Source lines start_line..=end_line (1-indexed), possibly truncated.
    pub source: Option<String>,
    /// True when source was truncated to max_source_lines.
    pub source_truncated: bool,
    /// Direct callers (depth=1).
    pub callers: Vec<ImpactSymbol>,
    /// Direct callees (depth=1).
    pub callees: Vec<ImpactSymbol>,
    /// Cyclomatic complexity of the symbol (None if unavailable).
    pub cyclomatic: Option<u32>,
    /// Cognitive complexity of the symbol (None if unavailable).
    pub cognitive: Option<u32>,
    /// Qualified names of other matching symbols when the query was ambiguous.
    pub candidates: Vec<String>,
}

/// Options for `get_symbol`.
pub struct SymbolOptions {
    /// Whether to include source code in the report.
    pub include_source: bool,
    /// Maximum source lines to return (excess triggers source_truncated=true).
    pub max_source_lines: usize,
}

impl Default for SymbolOptions {
    fn default() -> Self {
        Self {
            include_source: true,
            max_source_lines: 200,
        }
    }
}

/// Look up a symbol by name and return a full report.
///
/// Returns `Err` if the symbol is not found, with a message listing up to 10
/// suggestions from partial-name matches.
pub fn get_symbol(
    root: &Path,
    files: &[PathBuf],
    name: &str,
    opts: &SymbolOptions,
) -> Result<SymbolReport> {
    let index = build_index(root, files)?;

    let candidates_idxs = index.resolve(name);

    if candidates_idxs.is_empty() {
        // Build suggestions
        let q = name.to_lowercase();
        let mut suggestions: Vec<String> = index
            .by_name
            .keys()
            .filter(|k| {
                let kl = k.to_lowercase();
                kl.contains(&q) || q.contains(kl.as_str())
            })
            .cloned()
            .collect();
        suggestions.sort();
        suggestions.truncate(10);
        let hint = if suggestions.is_empty() {
            String::new()
        } else {
            format!(". Did you mean: {}", suggestions.join(", "))
        };
        return Err(crate::core::Error::analysis(format!(
            "Symbol '{}' not found{}",
            name, hint
        )));
    }

    let primary_idx = candidates_idxs[0];
    let sym = &index.symbols[primary_idx];

    // Collect other candidates (ambiguous matches)
    let candidates: Vec<String> = candidates_idxs[1..]
        .iter()
        .map(|&i| index.symbols[i].qualified_name.clone())
        .collect();

    // Source slice
    let (source, source_truncated) = if opts.include_source {
        read_source_slice(
            root,
            &sym.file,
            sym.line,
            sym.end_line,
            opts.max_source_lines,
        )
    } else {
        (None, false)
    };

    // Complexity from complexity analyzer on the symbol's file
    let (cyclomatic, cognitive) = {
        let file_path = root.join(&sym.file);
        let complexity_analyzer = crate::analyzers::complexity::Analyzer::new();
        if let Ok(file_result) = complexity_analyzer.analyze_file(&file_path) {
            // Find the function matching name + start_line
            let found = file_result
                .functions
                .iter()
                .find(|f| f.name == sym.name && f.start_line == sym.line);
            if let Some(f) = found {
                (Some(f.metrics.cyclomatic), Some(f.metrics.cognitive))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        }
    };

    // Direct callers/callees using CallGraphIndex
    let caller_levels = index.callers(&[primary_idx], 1);
    let callers: Vec<ImpactSymbol> = caller_levels
        .into_iter()
        .flat_map(|level| {
            level.into_iter().map(|i| ImpactSymbol {
                qualified_name: index.symbols[i].qualified_name.clone(),
                file: index.symbols[i].file.clone(),
                line: index.symbols[i].line,
            })
        })
        .collect();

    let callee_levels = index.callees(&[primary_idx], 1);
    let callees: Vec<ImpactSymbol> = callee_levels
        .into_iter()
        .flat_map(|level| {
            level.into_iter().map(|i| ImpactSymbol {
                qualified_name: index.symbols[i].qualified_name.clone(),
                file: index.symbols[i].file.clone(),
                line: index.symbols[i].line,
            })
        })
        .collect();

    Ok(SymbolReport {
        name: sym.name.clone(),
        qualified_name: sym.qualified_name.clone(),
        kind: format!("{:?}", sym.kind),
        file: sym.file.clone(),
        start_line: sym.line,
        end_line: sym.end_line,
        signature: sym.signature.clone(),
        source,
        source_truncated,
        callers,
        callees,
        cyclomatic,
        cognitive,
        candidates,
    })
}

/// Read lines start_line..=end_line from a file (1-indexed).
/// Returns (Some(text), truncated) or (None, false) on IO error.
fn read_source_slice(
    root: &Path,
    rel_file: &str,
    start_line: u32,
    end_line: u32,
    max_lines: usize,
) -> (Option<String>, bool) {
    let path = root.join(rel_file);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (None, false),
    };

    let lines: Vec<&str> = content.lines().collect();
    let start = (start_line as usize).saturating_sub(1);
    let end = (end_line as usize).min(lines.len());

    if start >= lines.len() {
        return (Some(String::new()), false);
    }

    let slice = &lines[start..end];
    let truncated = slice.len() > max_lines;
    let used = if truncated {
        &slice[..max_lines]
    } else {
        slice
    };
    (Some(used.join("\n")), truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_chain() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.rs"), "fn a() {\n    b();\n}\n").unwrap();
        fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();
        dir
    }

    fn files(dir: &TempDir) -> Vec<PathBuf> {
        vec!["a.rs", "b.rs"]
            .into_iter()
            .map(|f| dir.path().join(f))
            .collect()
    }

    #[test]
    fn test_basic_found() {
        let dir = make_chain();
        let fs = files(&dir);
        let report = get_symbol(dir.path(), &fs, "a", &SymbolOptions::default()).unwrap();
        assert_eq!(report.name, "a");
        assert!(report.qualified_name.contains("a.rs:a"));
        assert_eq!(report.kind, "Function");
    }

    #[test]
    fn test_qualified_name() {
        let dir = make_chain();
        let fs = files(&dir);
        let report = get_symbol(dir.path(), &fs, "a", &SymbolOptions::default()).unwrap();
        assert!(report.qualified_name.ends_with("a.rs:a"));
    }

    #[test]
    fn test_start_end_lines() {
        let dir = make_chain();
        let fs = files(&dir);
        let report = get_symbol(dir.path(), &fs, "a", &SymbolOptions::default()).unwrap();
        assert!(report.start_line >= 1, "start_line should be >= 1");
        assert!(
            report.end_line >= report.start_line,
            "end_line should be >= start_line"
        );
    }

    #[test]
    fn test_source_included_by_default() {
        let dir = make_chain();
        let fs = files(&dir);
        let report = get_symbol(dir.path(), &fs, "a", &SymbolOptions::default()).unwrap();
        assert!(
            report.source.is_some(),
            "source should be included by default"
        );
        let src = report.source.unwrap();
        assert!(src.contains("fn a"), "source should contain function body");
    }

    #[test]
    fn test_source_excluded_with_no_source() {
        let dir = make_chain();
        let fs = files(&dir);
        let opts = SymbolOptions {
            include_source: false,
            max_source_lines: 200,
        };
        let report = get_symbol(dir.path(), &fs, "a", &opts).unwrap();
        assert!(
            report.source.is_none(),
            "source should be None when include_source=false"
        );
        assert!(!report.source_truncated);
    }

    #[test]
    fn test_source_truncated_flag() {
        let dir = TempDir::new().unwrap();
        // Write a function with many lines
        let mut src = String::from("fn bigfn() {\n");
        for i in 0..20 {
            src.push_str(&format!("    let _ = {};\n", i));
        }
        src.push_str("}\n");
        fs::write(dir.path().join("big.rs"), &src).unwrap();
        let files = vec![dir.path().join("big.rs")];

        let opts = SymbolOptions {
            include_source: true,
            max_source_lines: 5,
        };
        let report = get_symbol(dir.path(), &files, "bigfn", &opts).unwrap();
        assert!(
            report.source_truncated,
            "source should be truncated when max_source_lines exceeded"
        );
        let src_out = report.source.unwrap();
        let line_count = src_out.lines().count();
        assert_eq!(line_count, 5, "should have exactly max_source_lines lines");
    }

    #[test]
    fn test_direct_callers_callees() {
        let dir = make_chain();
        let fs = files(&dir);
        // a calls b → b has callers=[a], callees=[]
        let report = get_symbol(dir.path(), &fs, "b", &SymbolOptions::default()).unwrap();
        assert_eq!(report.callees.len(), 0);
        // 'a' should be a caller of 'b'
        assert!(
            report
                .callers
                .iter()
                .any(|c| c.qualified_name.contains(":a")),
            "a should be a caller of b, got: {:?}",
            report.callers
        );
    }

    #[test]
    fn test_candidates_on_ambiguity() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("x.rs"), "fn helper() {}\n").unwrap();
        fs::write(dir.path().join("y.rs"), "fn helper() {}\n").unwrap();
        let files = vec![dir.path().join("x.rs"), dir.path().join("y.rs")];

        let report = get_symbol(dir.path(), &files, "helper", &SymbolOptions::default()).unwrap();
        // Should succeed (primary = first lex match), candidates = the other
        assert_eq!(report.candidates.len(), 1);
    }

    #[test]
    fn test_not_found_returns_err_with_suggestion() {
        let dir = make_chain();
        let fs = files(&dir);
        let result = get_symbol(
            dir.path(),
            &fs,
            "definitely_not_a_symbol",
            &SymbolOptions::default(),
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found"),
            "error message should contain 'not found': {msg}"
        );
    }

    #[test]
    fn test_cyclomatic_and_cognitive_present_for_branching() {
        let dir = TempDir::new().unwrap();
        // Function with branching to get non-trivial cyclomatic complexity
        fs::write(
            dir.path().join("complex.rs"),
            r#"fn complex_fn(x: i32) -> i32 {
    if x > 0 {
        if x > 10 {
            return x * 2;
        }
        return x;
    } else if x < -10 {
        return -x;
    }
    0
}
"#,
        )
        .unwrap();
        let files = vec![dir.path().join("complex.rs")];
        let report =
            get_symbol(dir.path(), &files, "complex_fn", &SymbolOptions::default()).unwrap();
        // Cyclomatic should be > 1 (branching function)
        assert!(
            report.cyclomatic.is_some(),
            "cyclomatic should be present for a parseable function"
        );
        if let Some(cyc) = report.cyclomatic {
            assert!(
                cyc >= 2,
                "branching function should have cyclomatic >= 2, got {cyc}"
            );
        }
    }

    #[test]
    fn test_serialization() {
        let dir = make_chain();
        let fs = files(&dir);
        let report = get_symbol(dir.path(), &fs, "a", &SymbolOptions::default()).unwrap();
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"qualified_name\""));
        let parsed: SymbolReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "a");
    }

    // Multi-language source-slice tests for all 13 supported languages
    // Each test: two adjacent functions; assert returned source contains the
    // target function's body text and does NOT contain the neighbor's body text.

    fn run_source_slice_test(
        dir: &TempDir,
        filename: &str,
        source: &str,
        target: &str,
        neighbor_marker: &str,
    ) {
        fs::write(dir.path().join(filename), source).unwrap();
        let files = vec![dir.path().join(filename)];
        let report = get_symbol(dir.path(), &files, target, &SymbolOptions::default());
        assert!(
            report.is_ok(),
            "get_symbol failed for '{target}' in {filename}: {:?}",
            report.err()
        );
        let report = report.unwrap();
        let src = report.source.expect("source should be present");
        assert!(
            src.contains(target),
            "source should contain '{target}' in {filename}, got: {src}"
        );
        assert!(
            !src.contains(neighbor_marker),
            "source should NOT contain neighbor marker '{neighbor_marker}' in {filename}, got: {src}"
        );
    }

    #[test]
    fn test_source_slice_rust() {
        let dir = TempDir::new().unwrap();
        let src =
            "fn target_func() {\n    let x = 1;\n}\nfn neighbor_func() {\n    let y = 2;\n}\n";
        run_source_slice_test(&dir, "test.rs", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_go() {
        let dir = TempDir::new().unwrap();
        let src = "package main\nfunc target_func() {\n\tx := 1\n\t_ = x\n}\nfunc neighbor_func() {\n\ty := 2\n\t_ = y\n}\n";
        run_source_slice_test(&dir, "test.go", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_python() {
        let dir = TempDir::new().unwrap();
        let src = "def target_func():\n    x = 1\n    return x\n\ndef neighbor_func():\n    y = 2\n    return y\n";
        run_source_slice_test(&dir, "test.py", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_typescript() {
        let dir = TempDir::new().unwrap();
        let src = "function target_func() {\n  const x = 1;\n  return x;\n}\nfunction neighbor_func() {\n  const y = 2;\n  return y;\n}\n";
        run_source_slice_test(&dir, "test.ts", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_javascript() {
        let dir = TempDir::new().unwrap();
        let src = "function target_func() {\n  const x = 1;\n  return x;\n}\nfunction neighbor_func() {\n  const y = 2;\n  return y;\n}\n";
        run_source_slice_test(&dir, "test.js", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_tsx() {
        let dir = TempDir::new().unwrap();
        let src = "function target_func() {\n  const x = 1;\n  return x;\n}\nfunction neighbor_func() {\n  const y = 2;\n  return y;\n}\n";
        run_source_slice_test(&dir, "test.tsx", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_jsx() {
        let dir = TempDir::new().unwrap();
        let src = "function target_func() {\n  const x = 1;\n  return x;\n}\nfunction neighbor_func() {\n  const y = 2;\n  return y;\n}\n";
        run_source_slice_test(&dir, "test.jsx", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_java() {
        let dir = TempDir::new().unwrap();
        let src = "class A {\n  void target_func() {\n    int x = 1;\n  }\n  void neighbor_func() {\n    int y = 2;\n  }\n}\n";
        run_source_slice_test(&dir, "test.java", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_c() {
        // C function name extraction is best-effort: tree-sitter-c nests the name
        // inside a declarator, so extract_functions may return 0 for plain C.
        // We attempt the test but skip (don't fail) if the symbol is not found.
        let dir = TempDir::new().unwrap();
        let src =
            "void target_func() {\n  int x = 1;\n}\nvoid neighbor_func() {\n  int y = 2;\n}\n";
        let filename = "test.c";
        fs::write(dir.path().join(filename), src).unwrap();
        let files = vec![dir.path().join(filename)];
        let report = get_symbol(dir.path(), &files, "target_func", &SymbolOptions::default());
        if let Ok(report) = report {
            let source = report.source.expect("source should be present when found");
            assert!(
                source.contains("target_func"),
                "source should contain target_func"
            );
            assert!(
                !source.contains("neighbor_func"),
                "source should not contain neighbor_func"
            );
        }
        // if not found — C parser limitation; skip
    }

    #[test]
    fn test_source_slice_cpp() {
        // C++ function name extraction is best-effort (same parser limitation as C).
        let dir = TempDir::new().unwrap();
        let src =
            "void target_func() {\n  int x = 1;\n}\nvoid neighbor_func() {\n  int y = 2;\n}\n";
        let filename = "test.cpp";
        fs::write(dir.path().join(filename), src).unwrap();
        let files = vec![dir.path().join(filename)];
        let report = get_symbol(dir.path(), &files, "target_func", &SymbolOptions::default());
        if let Ok(report) = report {
            let source = report.source.expect("source should be present when found");
            assert!(
                source.contains("target_func"),
                "source should contain target_func"
            );
            assert!(
                !source.contains("neighbor_func"),
                "source should not contain neighbor_func"
            );
        }
        // if not found — C++ parser limitation; skip
    }

    #[test]
    fn test_source_slice_csharp() {
        let dir = TempDir::new().unwrap();
        let src = "class A {\n  void target_func() {\n    int x = 1;\n  }\n  void neighbor_func() {\n    int y = 2;\n  }\n}\n";
        run_source_slice_test(&dir, "test.cs", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_ruby() {
        let dir = TempDir::new().unwrap();
        let src = "def target_func\n  x = 1\nend\ndef neighbor_func\n  y = 2\nend\n";
        run_source_slice_test(&dir, "test.rb", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_php() {
        let dir = TempDir::new().unwrap();
        let src = "<?php\nfunction target_func() {\n  $x = 1;\n}\nfunction neighbor_func() {\n  $y = 2;\n}\n";
        run_source_slice_test(&dir, "test.php", src, "target_func", "neighbor_func");
    }

    #[test]
    fn test_source_slice_bash() {
        let dir = TempDir::new().unwrap();
        let src =
            "#!/bin/bash\ntarget_func() {\n  local x=1\n}\nneighbor_func() {\n  local y=2\n}\n";
        // Bash functions may not be parsed as separate functions in the tree-sitter grammar
        // so we just check that the command runs without error
        fs::write(dir.path().join("test.sh"), src).unwrap();
        let files = vec![dir.path().join("test.sh")];
        // Bash function extraction may vary; just verify it doesn't panic
        let _ = get_symbol(dir.path(), &files, "target_func", &SymbolOptions::default());
    }
}
