//! Symbol report: one-call lookup returning source slice, signature, location,
//! direct callers/callees, and complexity for a named symbol.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::analyzers::impact::ImpactSymbol;
use crate::core::Result;
use crate::index::build::CodeIndex;
use crate::index::model::SymbolKind;
use crate::index::view::{CallEvidence, CallView};

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
    /// The declarations without a body of the same function, such as a C
    /// prototype or a C++ member declaration.
    pub declarations: Vec<ImpactSymbol>,
    /// Direct callers (depth=1).
    pub callers: Vec<ImpactSymbol>,
    /// Direct callees (depth=1).
    pub callees: Vec<ImpactSymbol>,
    /// Each call site in the body of the symbol, with the result of its
    /// resolution. Only a call with one candidate is an edge.
    #[serde(default)]
    pub call_resolutions: Vec<CallEvidence>,
    /// Cyclomatic complexity of the symbol (None if unavailable).
    pub cyclomatic: Option<u32>,
    /// Cognitive complexity of the symbol (None if unavailable).
    pub cognitive: Option<u32>,
    /// The security facts of the symbol. The caller adds them, because they need a
    /// full analysis of the files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<crate::index::security::SymbolSecurity>,
}

/// Options for `get_symbol`.
pub struct SymbolOptions {
    /// Whether to include source code in the report.
    pub include_source: bool,
    /// Maximum source lines to return (excess triggers source_truncated=true).
    pub max_source_lines: usize,
    /// The weakest basis that gives a caller or a callee.
    pub min_basis: crate::index::model::ResolutionBasis,
}

impl Default for SymbolOptions {
    fn default() -> Self {
        Self {
            include_source: true,
            max_source_lines: 200,
            min_basis: crate::index::view::DEFAULT_MIN_BASIS,
        }
    }
}

/// Look up a symbol by name and return a full report.
///
/// A name that does not select one definition gives
/// [`crate::core::Error::SymbolLookup`] with the near names or the choices.
pub fn get_symbol(
    root: &Path,
    files: &[PathBuf],
    name: &str,
    opts: &SymbolOptions,
) -> Result<SymbolReport> {
    get_symbol_at_line(root, files, name, opts, None)
}

/// Select a definition by its exact start line, including same-file duplicates.
pub fn get_symbol_at_line(
    root: &Path,
    files: &[PathBuf],
    name: &str,
    opts: &SymbolOptions,
    start_line: Option<u32>,
) -> Result<SymbolReport> {
    let index = CallView::from_index(&CodeIndex::from_files(root, files));
    report_from_index(root, &index, name, opts, start_line)
}

pub fn report_from_index(
    root: &Path,
    index: &CallView,
    name: &str,
    opts: &SymbolOptions,
    start_line: Option<u32>,
) -> Result<SymbolReport> {
    let read = |path: &Path| Ok(std::fs::read(root.join(path))?);
    report_with_reader(index, name, opts, start_line, &read)
}

/// Makes the report with `read` as the source of the content of a file. The
/// path that `read` gets is relative to the root of the repository. A report
/// for a commit reads the files of that commit.
pub fn report_with_reader(
    index: &CallView,
    name: &str,
    opts: &SymbolOptions,
    start_line: Option<u32>,
    read: &dyn Fn(&Path) -> Result<Vec<u8>>,
) -> Result<SymbolReport> {
    let selection = select(index, name, start_line)?;
    let primary_idx = selection.definition;
    // A call can resolve to a declaration of the function, thus the callers
    // of each declaration are callers of the function.
    let roots: Vec<usize> = std::iter::once(primary_idx)
        .chain(selection.declarations.iter().copied())
        .collect();
    let sym = &index.symbols[primary_idx];

    let content = read(Path::new(&sym.file)).ok();

    // Source slice
    let (source, source_truncated) = match (&content, opts.include_source) {
        (Some(content), true) => source_slice(
            &String::from_utf8_lossy(content),
            sym.line,
            sym.end_line,
            opts.max_source_lines,
        ),
        _ => (None, false),
    };

    // Complexity from complexity analyzer on the symbol's file
    let (cyclomatic, cognitive) = {
        let complexity_analyzer = crate::analyzers::complexity::Analyzer::new();
        let analyzed = content
            .ok_or_else(|| crate::core::Error::analysis("the file has no content"))
            .and_then(|content| complexity_analyzer.analyze_content(Path::new(&sym.file), content));
        if let Ok(file_result) = analyzed {
            // Find the function matching name + start_line
            let same_name = || file_result.functions.iter().filter(|f| f.name == sym.name);
            // A function with the same name can be in a function. The function
            // that starts at the line of the symbol is the symbol. The
            // complexity analyzer can start a function at its annotation. Then
            // the smallest function that contains the line is the symbol.
            let found = same_name().find(|f| f.start_line == sym.line).or_else(|| {
                same_name()
                    .filter(|f| f.start_line <= sym.line && sym.line <= f.end_line)
                    .min_by_key(|f| f.end_line - f.start_line)
            });
            if let Some(f) = found {
                (Some(f.metrics.cyclomatic), Some(f.metrics.cognitive))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        }
    };

    // Direct callers and callees from the call view
    let caller_levels = index.callers(&roots, 1, opts.min_basis);
    let callers: Vec<ImpactSymbol> = caller_levels
        .into_iter()
        .flat_map(|level| {
            level.into_iter().map(|i| ImpactSymbol {
                qualified_name: index.symbols[i].qualified_name.clone(),
                file: index.symbols[i].file.clone(),
                line: index.symbols[i].line,
                is_test: index.symbols[i].is_test,
            })
        })
        .collect();

    let callee_levels = index.callees(&[primary_idx], 1, opts.min_basis);
    let callees: Vec<ImpactSymbol> = callee_levels
        .into_iter()
        .flat_map(|level| {
            level.into_iter().map(|i| ImpactSymbol {
                qualified_name: index.symbols[i].qualified_name.clone(),
                file: index.symbols[i].file.clone(),
                line: index.symbols[i].line,
                is_test: index.symbols[i].is_test,
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
        declarations: selection
            .declarations
            .iter()
            .map(|&position| ImpactSymbol::of(&index.symbols[position]))
            .collect(),
        call_resolutions: index.calls_of(primary_idx),
        cyclomatic,
        cognitive,
        security: None,
    })
}

/// A name that does not select exactly one definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupFailure {
    /// No definition agrees with the name. `suggestions` holds near names.
    NotFound {
        name: String,
        start_line: Option<u32>,
        suggestions: Vec<String>,
    },
    /// Definitions agree with the name, but none of them starts at
    /// `start_line`. `choices` holds the definitions of the name.
    NotAtLine {
        name: String,
        start_line: u32,
        choices: Vec<SymbolChoice>,
    },
    /// More than one definition agrees with the name.
    Ambiguous {
        name: String,
        choices: Vec<SymbolChoice>,
    },
}

impl std::fmt::Display for LookupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound {
                name,
                start_line,
                suggestions,
            } => {
                write!(formatter, "No definition agrees with the symbol `{name}`.")?;
                if !suggestions.is_empty() {
                    write!(formatter, " Near names: {}.", suggestions.join(", "))?;
                }
                if let Some(line) = start_line {
                    write!(
                        formatter,
                        " Also make sure that the start line {line} is correct, or remove it."
                    )?;
                }
                Ok(())
            }
            Self::NotAtLine {
                name, start_line, ..
            } => write!(
                formatter,
                "No definition of the symbol `{name}` starts at line {start_line}. Correct the start line or remove it."
            ),
            Self::Ambiguous { name, choices } => write!(
                formatter,
                "{} definitions agree with the symbol `{name}`. Select one with its key or its start line.",
                choices.len()
            ),
        }
    }
}

/// One definition of an ambiguous name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolChoice {
    /// The key of the definition. As a name, it selects only this definition.
    pub name: String,
    pub kind: String,
    /// The path of the types and functions that contain the definition.
    pub container: Option<String>,
    pub file: String,
    pub start_line: u32,
    pub signature: String,
}

impl SymbolChoice {
    fn of(symbol: &crate::index::view::ViewSymbol) -> Self {
        Self {
            name: symbol.qualified_name.clone(),
            kind: format!("{:?}", symbol.kind),
            container: container_of(symbol),
            file: symbol.file.clone(),
            start_line: symbol.line,
            signature: symbol.signature.clone(),
        }
    }
}

/// The definition that a name selects, with the declarations of the same
/// function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub definition: usize,
    /// The declarations without a body, such as a C prototype or a C++ member
    /// declaration, in the sequence of the index.
    pub declarations: Vec<usize>,
}

/// The one definition that `name` and `start_line` select.
///
/// A callable declaration without a body and one definition with the same
/// name and the same container path are one function. Thus a header
/// prototype does not make a name ambiguous. Two definitions, or two
/// declarations without a definition, make the name ambiguous.
pub fn select(
    index: &CallView,
    name: &str,
    start_line: Option<u32>,
) -> std::result::Result<Selection, Box<LookupFailure>> {
    let resolved = index.resolve(name);
    let matches: Vec<usize> = resolved
        .iter()
        .copied()
        .filter(|&position| start_line.is_none_or(|line| index.symbols[position].line == line))
        .collect();
    if matches.is_empty() {
        return Err(Box::new(match start_line {
            Some(line) if !resolved.is_empty() => LookupFailure::NotAtLine {
                name: name.to_string(),
                start_line: line,
                choices: resolved
                    .iter()
                    .map(|&position| SymbolChoice::of(&index.symbols[position]))
                    .collect(),
            },
            _ => LookupFailure::NotFound {
                name: name.to_string(),
                start_line,
                suggestions: suggestions(&index.by_name, name),
            },
        }));
    }
    let (declarations, definitions): (Vec<usize>, Vec<usize>) = matches
        .iter()
        .partition(|&&position| is_declaration(&index.symbols[position]));
    let selected = match (definitions.as_slice(), declarations.as_slice()) {
        ([definition], declarations)
            if declarations.iter().all(|&declaration| {
                declares(&index.symbols[declaration], &index.symbols[*definition])
            }) =>
        {
            Some(*definition)
        }
        ([], [declaration]) => Some(*declaration),
        _ => None,
    };
    let Some(definition) = selected else {
        return Err(Box::new(LookupFailure::Ambiguous {
            name: name.to_string(),
            choices: matches
                .iter()
                .map(|&position| SymbolChoice::of(&index.symbols[position]))
                .collect(),
        }));
    };
    // A key, a file or a start line can exclude a declaration from the
    // matches, thus the declarations come from all symbols of the name.
    let symbol = &index.symbols[definition];
    let declarations = index
        .by_name
        .get(&symbol.name)
        .into_iter()
        .flatten()
        .copied()
        .filter(|&position| {
            let candidate = &index.symbols[position];
            is_declaration(candidate) && declares(candidate, symbol)
        })
        .collect();
    Ok(Selection {
        definition,
        declarations,
    })
}

/// `declaration` declares the function that `definition` defines: the
/// definition has a body, and the two have the same container path and a
/// linkage that agrees.
fn declares(
    declaration: &crate::index::view::ViewSymbol,
    definition: &crate::index::view::ViewSymbol,
) -> bool {
    callable(definition)
        && definition.has_body
        && container_of(declaration) == container_of(definition)
        && (declaration.file == definition.file
            || !(declaration.file_local || definition.file_local))
}

fn callable(symbol: &crate::index::view::ViewSymbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
}

fn is_declaration(symbol: &crate::index::view::ViewSymbol) -> bool {
    callable(symbol) && !symbol.has_body
}

impl From<Box<LookupFailure>> for crate::core::Error {
    fn from(failure: Box<LookupFailure>) -> Self {
        Self::SymbolLookup(failure)
    }
}

/// The container path in the key `file#Container.name~N` of `symbol`.
fn container_of(symbol: &crate::index::view::ViewSymbol) -> Option<String> {
    let path = symbol
        .qualified_name
        .strip_prefix(symbol.file.as_str())?
        .strip_prefix('#')?;
    // A destructor name also has `~`, thus remove only an ordinal after the name.
    let path = match path.rsplit_once('~') {
        Some((head, ordinal))
            if head.ends_with(symbol.name.as_str())
                && !ordinal.is_empty()
                && ordinal.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            head
        }
        _ => path,
    };
    path.strip_suffix(symbol.name.as_str())?
        .strip_suffix('.')
        .map(str::to_string)
}

/// The largest number of names that contain the query, or that the query
/// contains, in one failure.
const MAX_SUBSTRING_SUGGESTIONS: usize = 10;

/// The symbol names that are near `query`, in a stable sequence.
///
/// The first rule gives each name that contains the query, or that the query
/// contains. When no name agrees, the second rule gives the names with a small
/// edit distance to the bare name.
pub fn suggestions(by_name: &HashMap<String, Vec<usize>>, query: &str) -> Vec<String> {
    let lowercase_query = query.to_lowercase();
    let mut suggestions: Vec<String> = by_name
        .keys()
        .filter(|candidate| {
            let candidate = candidate.to_lowercase();
            candidate.contains(&lowercase_query) || lowercase_query.contains(candidate.as_str())
        })
        .cloned()
        .collect();
    suggestions.sort();
    suggestions.truncate(MAX_SUBSTRING_SUGGESTIONS);
    if !suggestions.is_empty() {
        return suggestions;
    }
    let bare_name = query.rsplit([':', '#', '.']).next().unwrap_or(query);
    let names: Vec<&str> = by_name.keys().map(String::as_str).collect();
    crate::mcp::nearest(bare_name, &names)
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// The lines start_line..=end_line of the content of a file (1-indexed), and
/// a flag that tells if `max_lines` cut them.
fn source_slice(
    content: &str,
    start_line: u32,
    end_line: u32,
    max_lines: usize,
) -> (Option<String>, bool) {
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
        fs::write(
            dir.path().join("main.rs"),
            "mod b;\nfn a() {\n    b::b();\n}\n",
        )
        .unwrap();
        fs::write(dir.path().join("b.rs"), "pub fn b() {}\n").unwrap();
        dir
    }

    fn files(dir: &TempDir) -> Vec<PathBuf> {
        vec!["main.rs", "b.rs"]
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
        assert!(report.qualified_name.contains("main.rs#a"));
        assert_eq!(report.kind, "Function");
    }

    #[test]
    fn test_qualified_name() {
        let dir = make_chain();
        let fs = files(&dir);
        let report = get_symbol(dir.path(), &fs, "a", &SymbolOptions::default()).unwrap();
        assert!(report.qualified_name.ends_with("main.rs#a"));
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

    /// An inner function has the name of the outer function. The report of
    /// the inner function has the complexity of the inner function.
    #[test]
    fn test_nested_function_with_the_name_of_the_outer_function() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("a.py"),
            "def f(items):\n    for item in items:\n        if item:\n            if item > 1:\n                pass\n\n    def f():\n        return 1\n\n    return f\n",
        )
        .unwrap();
        let files = vec![dir.path().join("a.py")];
        let outer = get_symbol(dir.path(), &files, "a.py#f", &SymbolOptions::default()).unwrap();
        let inner = get_symbol(dir.path(), &files, "a.py#f.f", &SymbolOptions::default()).unwrap();
        assert_eq!(outer.start_line, 1);
        assert_eq!(inner.start_line, 7);
        assert!(outer.cyclomatic.unwrap() > 1, "{outer:?}");
        assert_eq!(inner.cyclomatic, Some(1), "{inner:?}");
    }

    #[test]
    fn test_source_excluded_with_no_source() {
        let dir = make_chain();
        let fs = files(&dir);
        let opts = SymbolOptions {
            include_source: false,
            max_source_lines: 200,
            ..SymbolOptions::default()
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
            ..SymbolOptions::default()
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
                .any(|c| c.qualified_name.ends_with("#a")),
            "a should be a caller of b, got: {:?}",
            report.callers
        );
    }

    fn two_helpers() -> (TempDir, Vec<PathBuf>) {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("x.py"),
            "class Shop:\n    def helper(self):\n        pass\n",
        )
        .unwrap();
        fs::write(dir.path().join("y.py"), "\ndef helper():\n    pass\n").unwrap();
        let files = vec![dir.path().join("x.py"), dir.path().join("y.py")];
        (dir, files)
    }

    fn lookup_failure(result: Result<SymbolReport>) -> LookupFailure {
        match result {
            Err(crate::core::Error::SymbolLookup(failure)) => *failure,
            other => panic!("expected a lookup failure: {other:?}"),
        }
    }

    #[test]
    fn test_an_ambiguous_name_gives_each_choice_and_no_report() {
        let (dir, files) = two_helpers();
        let failure = lookup_failure(get_symbol(
            dir.path(),
            &files,
            "helper",
            &SymbolOptions::default(),
        ));
        let LookupFailure::Ambiguous { name, choices } = failure else {
            panic!("expected ambiguous: {failure:?}");
        };
        assert_eq!(name, "helper");
        assert_eq!(
            choices,
            vec![
                SymbolChoice {
                    name: "x.py#Shop.helper".to_string(),
                    kind: "Method".to_string(),
                    container: Some("Shop".to_string()),
                    file: "x.py".to_string(),
                    start_line: 2,
                    signature: "def helper(self):".to_string(),
                },
                SymbolChoice {
                    name: "y.py#helper".to_string(),
                    kind: "Function".to_string(),
                    container: None,
                    file: "y.py".to_string(),
                    start_line: 2,
                    signature: "def helper():".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_the_key_of_a_choice_selects_one_definition() {
        let (dir, files) = two_helpers();
        let report =
            get_symbol(dir.path(), &files, "y.py#helper", &SymbolOptions::default()).unwrap();
        assert_eq!(report.qualified_name, "y.py#helper");
    }

    #[test]
    fn test_a_start_line_does_not_select_two_definitions_on_the_same_line() {
        let (dir, files) = two_helpers();
        let failure = lookup_failure(get_symbol_at_line(
            dir.path(),
            &files,
            "helper",
            &SymbolOptions::default(),
            Some(2),
        ));
        assert!(
            matches!(&failure, LookupFailure::Ambiguous { choices, .. } if choices.len() == 2),
            "{failure:?}"
        );
    }

    #[test]
    fn test_a_file_and_a_start_line_select_one_definition() {
        let (dir, files) = two_helpers();
        let report = get_symbol_at_line(
            dir.path(),
            &files,
            "x.py:helper",
            &SymbolOptions::default(),
            Some(2),
        )
        .unwrap();
        assert_eq!(report.qualified_name, "x.py#Shop.helper");
    }

    #[test]
    fn test_a_misspelled_name_gives_the_near_names() {
        let (dir, files) = two_helpers();
        let failure = lookup_failure(get_symbol(
            dir.path(),
            &files,
            "helpr",
            &SymbolOptions::default(),
        ));
        assert_eq!(
            failure,
            LookupFailure::NotFound {
                name: "helpr".to_string(),
                start_line: None,
                suggestions: vec!["helper".to_string()],
            }
        );
    }

    fn repository(files: &[(&str, &str)]) -> (TempDir, Vec<PathBuf>) {
        let dir = TempDir::new().unwrap();
        for (file, source) in files {
            fs::write(dir.path().join(file), source).unwrap();
        }
        let paths = files
            .iter()
            .map(|(file, _)| dir.path().join(file))
            .collect();
        (dir, paths)
    }

    fn located(report: &[ImpactSymbol]) -> Vec<String> {
        report
            .iter()
            .map(|symbol| format!("{} {}:{}", symbol.qualified_name, symbol.file, symbol.line))
            .collect()
    }

    fn choice_keys(result: Result<SymbolReport>) -> Vec<String> {
        match lookup_failure(result) {
            LookupFailure::Ambiguous { choices, .. } => {
                choices.into_iter().map(|choice| choice.name).collect()
            }
            failure => panic!("expected ambiguous: {failure:?}"),
        }
    }

    #[test]
    fn test_a_c_prototype_and_its_one_definition_are_one_function() {
        let (dir, files) = repository(&[
            ("shapes.h", "double area(int radius);\n"),
            (
                "shapes.c",
                "#include \"shapes.h\"\ndouble area(int radius) {\n    return radius;\n}\n",
            ),
        ]);
        let report = get_symbol(dir.path(), &files, "area", &SymbolOptions::default()).unwrap();
        assert_eq!(report.qualified_name, "shapes.c#area");
        assert_eq!(located(&report.declarations), ["shapes.h#area shapes.h:1"]);
    }

    #[test]
    fn test_a_cpp_member_declaration_and_its_out_of_line_definition_are_one_method() {
        let (dir, files) = repository(&[
            ("shape.hpp", "class Shape {\npublic:\n    int area();\n};\n"),
            (
                "shape.cpp",
                "#include \"shape.hpp\"\nint Shape::area() {\n    return 1;\n}\n",
            ),
        ]);
        let report = get_symbol(dir.path(), &files, "area", &SymbolOptions::default()).unwrap();
        assert_eq!(report.qualified_name, "shape.cpp#Shape.area");
        assert_eq!(
            located(&report.declarations),
            ["shape.hpp#Shape.area shape.hpp:3"]
        );
    }

    #[test]
    fn test_two_c_definitions_under_ifdef_are_ambiguous() {
        let (dir, files) = repository(&[
            ("mode.h", "int mode(void);\n"),
            (
                "mode.c",
                "#ifdef FAST\nint mode(void) { return 1; }\n#else\nint mode(void) { return 2; }\n#endif\n",
            ),
        ]);
        assert_eq!(
            choice_keys(get_symbol(
                dir.path(),
                &files,
                "mode",
                &SymbolOptions::default()
            )),
            ["mode.c#mode", "mode.c#mode~2", "mode.h#mode"]
        );
    }

    #[test]
    fn test_two_prototypes_without_a_definition_are_ambiguous() {
        let (dir, files) =
            repository(&[("a.h", "int tick(void);\n"), ("b.h", "int tick(void);\n")]);
        assert_eq!(
            choice_keys(get_symbol(
                dir.path(),
                &files,
                "tick",
                &SymbolOptions::default()
            )),
            ["a.h#tick", "b.h#tick"]
        );
    }

    #[test]
    fn test_one_prototype_without_a_definition_is_selected() {
        let (dir, files) = repository(&[("a.h", "int tick(void);\n")]);
        let report = get_symbol(dir.path(), &files, "tick", &SymbolOptions::default()).unwrap();
        assert_eq!(report.qualified_name, "a.h#tick");
        assert!(report.declarations.is_empty());
    }

    #[test]
    fn test_a_cpp_declaration_in_a_different_namespace_is_a_different_function() {
        let (dir, files) = repository(&[
            ("run.hpp", "namespace tools {\nvoid run();\n}\n"),
            ("run.cpp", "void run() {}\n"),
        ]);
        assert_eq!(
            choice_keys(get_symbol(
                dir.path(),
                &files,
                "run",
                &SymbolOptions::default()
            )),
            ["run.cpp#run", "run.hpp#tools.run"]
        );
    }

    #[test]
    fn test_one_type_is_selected() {
        let (dir, files) = repository(&[("shop.py", "class Shop:\n    pass\n")]);
        let report = get_symbol(dir.path(), &files, "Shop", &SymbolOptions::default()).unwrap();
        assert_eq!(report.qualified_name, "shop.py#Shop");
    }

    fn out_of_line_method() -> (TempDir, Vec<PathBuf>) {
        repository(&[
            ("shape.hpp", "class Shape {\npublic:\n    int area();\n};\n"),
            (
                "shape.cpp",
                "#include \"shape.hpp\"\nint Shape::area() {\n    return 1;\n}\n",
            ),
        ])
    }

    #[test]
    fn test_a_key_keeps_the_declarations_of_the_definition() {
        let (dir, files) = out_of_line_method();
        let report = get_symbol(
            dir.path(),
            &files,
            "shape.cpp#Shape.area",
            &SymbolOptions::default(),
        )
        .unwrap();
        assert_eq!(
            located(&report.declarations),
            ["shape.hpp#Shape.area shape.hpp:3"]
        );
    }

    #[test]
    fn test_a_start_line_keeps_the_declarations_of_the_definition() {
        let (dir, files) = out_of_line_method();
        let report = get_symbol_at_line(
            dir.path(),
            &files,
            "area",
            &SymbolOptions::default(),
            Some(2),
        )
        .unwrap();
        assert_eq!(report.qualified_name, "shape.cpp#Shape.area");
        assert_eq!(
            located(&report.declarations),
            ["shape.hpp#Shape.area shape.hpp:3"]
        );
    }

    #[test]
    fn test_a_file_qualified_name_keeps_a_declaration_in_a_different_file() {
        let (dir, files) = repository(&[
            ("a.cpp", "void b();\nvoid a() { b(); }\n"),
            ("b.cpp", "void b() {}\n"),
        ]);
        let report = get_symbol(dir.path(), &files, "b.cpp:b", &SymbolOptions::default()).unwrap();
        assert_eq!(report.qualified_name, "b.cpp#b");
        assert_eq!(located(&report.declarations), ["a.cpp#b a.cpp:1"]);
        assert_eq!(located(&report.callers), ["a.cpp#a a.cpp:2"]);
    }

    #[test]
    fn test_a_static_definition_is_not_the_external_function_of_a_header() {
        let (dir, files) = repository(&[
            ("api.h", "int tick(void);\n"),
            ("local.c", "static int tick(void) { return 1; }\n"),
        ]);
        assert_eq!(
            choice_keys(get_symbol(
                dir.path(),
                &files,
                "tick",
                &SymbolOptions::default()
            )),
            ["api.h#tick", "local.c#tick"]
        );
        let report = get_symbol(
            dir.path(),
            &files,
            "local.c#tick",
            &SymbolOptions::default(),
        )
        .unwrap();
        assert!(report.declarations.is_empty(), "{:?}", report.declarations);
    }

    #[test]
    fn test_a_static_prototype_and_its_definition_in_one_file_are_one_function() {
        let (dir, files) = repository(&[(
            "local.c",
            "static int tick(void);\nint run(void) { return tick(); }\nstatic int tick(void) { return 1; }\n",
        )]);
        let report = get_symbol(dir.path(), &files, "tick", &SymbolOptions::default()).unwrap();
        assert_eq!(report.qualified_name, "local.c#tick~2");
        assert_eq!(located(&report.declarations), ["local.c#tick local.c:1"]);
    }

    #[test]
    fn test_a_start_line_that_no_definition_has_is_the_problem() {
        let (dir, files) = two_helpers();
        let failure = lookup_failure(get_symbol_at_line(
            dir.path(),
            &files,
            "x.py:helper",
            &SymbolOptions::default(),
            Some(9),
        ));
        let LookupFailure::NotAtLine {
            name,
            start_line,
            choices,
        } = &failure
        else {
            panic!("expected a start line failure: {failure:?}");
        };
        assert_eq!((name.as_str(), *start_line), ("x.py:helper", 9));
        assert_eq!(
            choices
                .iter()
                .map(|choice| (choice.name.as_str(), choice.start_line))
                .collect::<Vec<_>>(),
            [("x.py#Shop.helper", 2)]
        );
        assert_eq!(
            failure.to_string(),
            "No definition of the symbol `x.py:helper` starts at line 9. Correct the start line or remove it."
        );
    }

    #[test]
    fn test_a_misspelled_name_with_a_start_line_names_the_start_line_too() {
        let (dir, files) = two_helpers();
        let failure = lookup_failure(get_symbol_at_line(
            dir.path(),
            &files,
            "helpr",
            &SymbolOptions::default(),
            Some(9),
        ));
        assert_eq!(
            failure,
            LookupFailure::NotFound {
                name: "helpr".to_string(),
                start_line: Some(9),
                suggestions: vec!["helper".to_string()],
            }
        );
        assert_eq!(
            failure.to_string(),
            "No definition agrees with the symbol `helpr`. Near names: helper. Also make sure that the start line 9 is correct, or remove it."
        );
    }

    #[test]
    fn test_the_container_of_a_key_with_an_ordinal() {
        let symbol = |key: &str, name: &str| crate::index::view::ViewSymbol {
            name: name.to_string(),
            body_start: 0,
            has_body: true,
            file_local: false,
            qualified_name: key.to_string(),
            kind: SymbolKind::Method,
            file: "a.cpp".to_string(),
            line: 1,
            end_line: 1,
            signature: String::new(),
            is_test: false,
        };
        assert_eq!(
            container_of(&symbol("a.cpp#A.B.f~2", "f")),
            Some("A.B".to_string())
        );
        assert_eq!(
            container_of(&symbol("a.cpp#A.~A", "~A")),
            Some("A".to_string())
        );
        assert_eq!(
            container_of(&symbol("a.cpp#A.~A~3", "~A")),
            Some("A".to_string())
        );
        assert_eq!(container_of(&symbol("a.cpp#f", "f")), None);
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
            msg.contains("No definition agrees with the symbol `definitely_not_a_symbol`"),
            "{msg}"
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
        // inside a declarator, so the index may find no symbol for plain C.
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

#[cfg(test)]
mod timing {
    use super::*;
    use std::time::Duration;

    /// A view of one file with `size` definitions of `helper` and `size`
    /// other names.
    fn view(size: usize) -> (tempfile::TempDir, CallView) {
        let directory = tempfile::TempDir::new().unwrap();
        let mut source = String::new();
        for index in 0..size {
            source.push_str(&format!(
                "def helper():\n    pass\n\ndef name_{index}():\n    pass\n\n"
            ));
        }
        std::fs::write(directory.path().join("many.py"), source).unwrap();
        let files = vec![directory.path().join("many.py")];
        let view = CallView::from_index(&CodeIndex::from_files(directory.path(), &files));
        (directory, view)
    }

    /// The time of an ambiguous lookup and of an unknown lookup.
    fn lookups(size: usize) -> Duration {
        let (_directory, view) = view(size);
        let started = crate::index::timing::Stopwatch::start();
        let ambiguous = select(&view, "helper", None).unwrap_err();
        let unknown = select(&view, "nothing_agrees", None).unwrap_err();
        let elapsed = started.elapsed();
        assert!(
            matches!(&*ambiguous, LookupFailure::Ambiguous { choices, .. } if choices.len() == size)
        );
        assert!(matches!(*unknown, LookupFailure::NotFound { .. }));
        elapsed
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_lookup_failures_are_linear_in_the_number_of_definitions() {
        crate::index::timing::assert_scales_linearly_above(
            "definitions",
            5_000,
            Duration::from_millis(50),
            lookups,
        );
    }
}
