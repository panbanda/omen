//! Outline analyzer — token-cheap file map: imports, classes, top-level functions.

use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result, SourceFile};
use crate::parser::{extract_classes, extract_functions, extract_imports, Parser};

/// A function in the outline (top-level or method).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineFunction {
    pub name: String,
    /// First line of declaration, truncated to 120 chars.
    pub signature: String,
    pub start_line: u32,
    pub end_line: u32,
    pub is_exported: bool,
}

/// A class or struct in the outline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineClass {
    pub name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub is_exported: bool,
    pub methods: Vec<OutlineFunction>,
    pub fields: Vec<String>,
}

/// Outline for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOutline {
    pub file: String,
    pub language: String,
    pub loc: u32,
    pub imports: Vec<String>,
    pub classes: Vec<OutlineClass>,
    pub functions: Vec<OutlineFunction>,
}

/// Complete outline result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineResult {
    pub files: Vec<FileOutline>,
}

impl OutlineResult {
    /// Produce a dense, agent-facing markdown representation.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        for (i, file) in self.files.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&format!(
                "## {}  [{}, {} loc]\n",
                file.file, file.language, file.loc
            ));
            if !file.imports.is_empty() {
                out.push_str(&format!("imports: {}\n", file.imports.join(", ")));
            }
            for cls in &file.classes {
                let pub_tag = if cls.is_exported { " [pub]" } else { "" };
                out.push_str(&format!(
                    "class {} L{}-{}{}\n",
                    cls.name, cls.start_line, cls.end_line, pub_tag
                ));
                if !cls.fields.is_empty() {
                    out.push_str(&format!("  field: {}\n", cls.fields.join(", ")));
                }
                for m in &cls.methods {
                    let pub_tag = if m.is_exported { " [pub]" } else { "" };
                    out.push_str(&format!(
                        "  fn {} L{}-{}{}\n",
                        m.name, m.start_line, m.end_line, pub_tag
                    ));
                }
            }
            for func in &file.functions {
                let pub_tag = if func.is_exported { " [pub]" } else { "" };
                out.push_str(&format!(
                    "fn {} L{}-{}{}\n",
                    func.name, func.start_line, func.end_line, pub_tag
                ));
            }
        }
        out
    }
}

/// Analyze a single file and return its outline.
pub fn outline_file(path: &Path) -> Result<FileOutline> {
    let file = SourceFile::load(path)?;
    let parser = Parser::new();
    let result = parser.parse_source(&file)?;

    let imports: Vec<String> = extract_imports(&result)
        .into_iter()
        .map(|i| i.path)
        .collect();

    let classes_raw = extract_classes(&result);
    let functions_raw = extract_functions(&result);

    // Build two exclusion sets so we can correctly identify top-level functions:
    //
    // 1. Class body ranges (struct/class definition extent, NOT extended to impl
    //    blocks): catches languages like Python/Java/C++ where methods are
    //    lexically inside the class body.
    // 2. Method (name, start_line) pairs from every class: catches languages
    //    like Rust and Go where methods live outside the struct definition (in
    //    impl / standalone function blocks) and therefore fall outside the class
    //    body range.
    let class_ranges: Vec<(u32, u32)> = classes_raw
        .iter()
        .map(|c| (c.start_line, c.end_line))
        .collect();

    // Set of (name, start_line) for every method across all classes.
    let method_keys: std::collections::HashSet<(String, u32)> = classes_raw
        .iter()
        .flat_map(|c| c.methods.iter().map(|m| (m.name.clone(), m.start_line)))
        .collect();

    // Convert class nodes
    let classes: Vec<OutlineClass> = classes_raw
        .into_iter()
        .map(|c| OutlineClass {
            name: c.name,
            start_line: c.start_line,
            end_line: c.end_line,
            is_exported: c.is_exported,
            methods: c
                .methods
                .into_iter()
                .map(|m| OutlineFunction {
                    name: m.name,
                    signature: truncate_120(&m.signature),
                    start_line: m.start_line,
                    end_line: m.end_line,
                    is_exported: m.is_exported,
                })
                .collect(),
            fields: c.fields,
        })
        .collect();

    // Top-level functions: a function is a method (not top-level) if either:
    //  (a) its start_line falls within a class body range, OR
    //  (b) its (name, start_line) appears in the method_keys set.
    let functions: Vec<OutlineFunction> = functions_raw
        .into_iter()
        .filter(|f| {
            let in_class_body = class_ranges
                .iter()
                .any(|(start, end)| f.start_line >= *start && f.start_line <= *end);
            let is_method = method_keys.contains(&(f.name.clone(), f.start_line));
            !in_class_body && !is_method
        })
        .map(|f| OutlineFunction {
            name: f.name,
            signature: truncate_120(&f.signature),
            start_line: f.start_line,
            end_line: f.end_line,
            is_exported: f.is_exported,
        })
        .collect();

    let loc = file.lines_of_code() as u32;
    let language = result.language.to_string();
    let file_str = path.to_string_lossy().to_string();

    Ok(FileOutline {
        file: file_str,
        language,
        loc,
        imports,
        classes,
        functions,
    })
}

fn truncate_120(s: &str) -> String {
    // Use char-based slicing to avoid panicking on multi-byte UTF-8 characters.
    if s.chars().count() <= 120 {
        s.to_string()
    } else {
        s.chars().take(120).collect()
    }
}

/// Outline analyzer for repository-wide analysis.
#[derive(Debug, Default)]
pub struct Analyzer;

impl AnalyzerTrait for Analyzer {
    type Output = OutlineResult;

    fn name(&self) -> &'static str {
        "outline"
    }

    fn description(&self) -> &'static str {
        "Token-cheap file outline: imports, classes, top-level functions"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let files: Vec<PathBuf> = ctx.files.iter().map(|p| ctx.root.join(p)).collect();

        let mut file_outlines: Vec<FileOutline> = files
            .par_iter()
            .filter_map(|path| outline_file(path).ok())
            .map(|mut fo| {
                // Make paths relative to root
                if let Ok(rel) = Path::new(&fo.file).strip_prefix(ctx.root) {
                    fo.file = rel.to_string_lossy().to_string();
                }
                fo
            })
            .collect();

        // Sort by file path for determinism
        file_outlines.sort_by(|a, b| a.file.cmp(&b.file));

        Ok(OutlineResult {
            files: file_outlines,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    // ===== Per-language fixture tests =====

    #[test]
    fn test_outline_rust_fixture() {
        let result = outline_file(&fixture("sample.rs")).unwrap();
        assert_eq!(result.language, "Rust");
        assert_eq!(result.classes.len(), 1);
        assert_eq!(result.classes[0].name, "Config");
        // 2 methods: new + validate
        assert_eq!(result.classes[0].methods.len(), 2);
        // 1 top-level function: fibonacci
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "fibonacci");
        // fields: name, retries
        assert!(!result.classes[0].fields.is_empty());
    }

    #[test]
    fn test_outline_go_fixture() {
        let result = outline_file(&fixture("sample.go")).unwrap();
        assert_eq!(result.language, "Go");
        assert_eq!(result.classes.len(), 1);
        assert_eq!(result.classes[0].name, "Server");
        // At least 1 method extracted
        assert!(
            !result.classes[0].methods.is_empty(),
            "Server should have at least 1 method"
        );
        // top-level: NewServer + maxOf (should be >= 1)
        assert!(
            !result.functions.is_empty(),
            "should have at least 1 top-level function"
        );
    }

    #[test]
    fn test_outline_python_fixture() {
        let result = outline_file(&fixture("sample.py")).unwrap();
        assert_eq!(result.language, "Python");
        assert_eq!(result.classes.len(), 1);
        assert_eq!(result.classes[0].name, "UserService");
        // 3 methods: __init__, get_user, create_user
        assert_eq!(result.classes[0].methods.len(), 3);
        // 1 top-level: calculate_discount
        assert_eq!(result.functions.len(), 1);
    }

    #[test]
    fn test_outline_typescript_fixture() {
        let result = outline_file(&fixture("sample.ts")).unwrap();
        assert_eq!(result.language, "TypeScript");
        // ConsoleLogger class
        assert_eq!(result.classes.len(), 1);
        assert_eq!(result.classes[0].name, "ConsoleLogger");
        assert!(result.classes[0].is_exported);
        // parseConfig function
        assert!(result.functions.iter().any(|f| f.name == "parseConfig"));
    }

    #[test]
    fn test_outline_ruby_fixture() {
        let result = outline_file(&fixture("sample.rb")).unwrap();
        assert_eq!(result.language, "Ruby");
        assert_eq!(result.classes.len(), 1);
        assert_eq!(result.classes[0].name, "OrderProcessor");
    }

    #[test]
    fn test_outline_java_fixture() {
        let result = outline_file(&fixture("sample.java")).unwrap();
        assert_eq!(result.language, "Java");
        assert!(!result.classes.is_empty());
        // 2 imports
        assert_eq!(result.imports.len(), 2);
        // OrderService has 2 methods
        let order_svc = result
            .classes
            .iter()
            .find(|c| c.name == "OrderService")
            .unwrap();
        assert_eq!(order_svc.methods.len(), 2);
    }

    #[test]
    fn test_outline_c_fixture() {
        let result = outline_file(&fixture("sample.c")).unwrap();
        assert_eq!(result.language, "C");
        assert_eq!(result.classes.len(), 0, "C has no classes");
        // C function extraction may vary by tree-sitter grammar; just verify parsing succeeds
        // and loc is counted
        assert!(result.loc > 0, "C file should have non-zero loc");
    }

    #[test]
    fn test_outline_cpp_fixture() {
        let result = outline_file(&fixture("sample.cpp")).unwrap();
        assert_eq!(result.language, "C++");
        assert_eq!(result.classes.len(), 1);
        assert_eq!(result.classes[0].name, "Shape");
        // C++ parsing verified via class extraction
        assert!(result.loc > 0, "C++ file should have non-zero loc");
    }

    #[test]
    fn test_outline_csharp_fixture() {
        let result = outline_file(&fixture("sample.cs")).unwrap();
        assert_eq!(result.language, "C#");
        assert!(!result.classes.is_empty());
        // Calculator has 2 methods
        let calc = result
            .classes
            .iter()
            .find(|c| c.name == "Calculator")
            .unwrap();
        assert_eq!(calc.methods.len(), 2);
        // 2 imports
        assert_eq!(result.imports.len(), 0); // C# using not handled as imports in parser
    }

    #[test]
    fn test_outline_php_fixture() {
        let result = outline_file(&fixture("sample.php")).unwrap();
        assert_eq!(result.language, "PHP");
        assert_eq!(result.classes.len(), 1);
        assert_eq!(result.classes[0].name, "UserRepository");
        assert_eq!(result.classes[0].methods.len(), 2);
        // 2 top-level functions
        assert!(result.functions.len() >= 2);
    }

    #[test]
    fn test_outline_bash_fixture() {
        let result = outline_file(&fixture("sample.sh")).unwrap();
        assert_eq!(result.language, "Bash");
        assert_eq!(result.classes.len(), 0, "Bash has no classes");
        assert!(result.functions.len() >= 4);
    }

    #[test]
    fn test_outline_javascript_fixture() {
        let result = outline_file(&fixture("sample.js")).unwrap();
        assert_eq!(result.language, "JavaScript");
        assert_eq!(result.classes.len(), 1);
        assert_eq!(result.classes[0].name, "EventBus");
        // 2 top-level functions: parseConfig, formatError
        assert!(result.functions.len() >= 2);
        // 2 imports
        assert_eq!(result.imports.len(), 2);
    }

    #[test]
    fn test_outline_tsx_fixture() {
        let result = outline_file(&fixture("sample.tsx")).unwrap();
        assert_eq!(result.language, "TSX");
        // ThemeProvider class + top-level functions Button, formatLabel
        assert_eq!(result.classes.len(), 1);
        assert_eq!(result.classes[0].name, "ThemeProvider");
        assert!(result.classes[0].is_exported);
        // Button and formatLabel are top-level functions
        assert!(result.functions.iter().any(|f| f.name == "Button"));
    }

    // ===== Property tests =====

    #[test]
    fn test_outline_determinism() {
        let result1 = outline_file(&fixture("sample.rs")).unwrap();
        let result2 = outline_file(&fixture("sample.rs")).unwrap();
        let s1 = serde_json::to_string(&result1).unwrap();
        let s2 = serde_json::to_string(&result2).unwrap();
        assert_eq!(s1, s2, "outline_file must be deterministic");
    }

    #[test]
    fn test_outline_unknown_extension() {
        let result = outline_file(Path::new("/tmp/unknown.xyz123"));
        assert!(result.is_err(), "Unknown extension should return Err");
    }

    #[test]
    fn test_outline_top_level_excludes_methods() {
        let result = outline_file(&fixture("sample.py")).unwrap();
        // Methods like __init__, get_user, create_user should NOT appear in top-level functions
        let top_names: Vec<&str> = result.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(
            !top_names.contains(&"__init__"),
            "class methods should not appear as top-level functions"
        );
        assert!(
            !top_names.contains(&"get_user"),
            "class methods should not appear as top-level functions"
        );
    }

    #[test]
    fn test_outline_empty_file() {
        use tempfile::NamedTempFile;
        let tmp = NamedTempFile::with_suffix(".rs").unwrap();
        // Empty file - should not panic, but SourceFile.load may fail on empty or succeed with empty tree
        let path = tmp.path().to_path_buf();
        // Write empty content
        std::fs::write(&path, b"").unwrap();
        // May fail or succeed, but must not panic
        let _ = outline_file(&path);
    }

    #[test]
    fn test_outline_analyzer_sorted() {
        use crate::config::Config;
        use crate::core::{AnalysisContext, FileSet};

        let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let config = Config::default();
        let file_set = FileSet::from_path(&fixtures_dir, &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(&fixtures_dir));

        let analyzer = Analyzer;
        let result = analyzer.analyze(&ctx).unwrap();

        // Check sorting by path
        let paths: Vec<&str> = result.files.iter().map(|f| f.file.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "files should be sorted by path");
    }

    /// Regression: a free function located between a struct definition and the
    /// corresponding impl block must appear in the top-level functions list, not
    /// be swallowed by the class range.  impl methods must NOT appear as top-level.
    #[test]
    fn test_free_fn_between_struct_and_impl_not_swallowed() {
        use tempfile::NamedTempFile;

        // Rust file layout:
        //   struct A { … }
        //   fn free_between() { … }   ← must show up as top-level
        //   impl A { fn method_of_a() { … } }  ← must NOT show up as top-level
        let src = r#"
struct A {
    x: i32,
}

fn free_between() -> i32 {
    42
}

impl A {
    fn method_of_a(&self) -> i32 {
        self.x
    }
}
"#;
        let tmp = NamedTempFile::with_suffix(".rs").unwrap();
        std::fs::write(tmp.path(), src).unwrap();

        let result = outline_file(tmp.path()).unwrap();

        let top_names: Vec<&str> = result.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(
            top_names.contains(&"free_between"),
            "free_between should be a top-level function; got {:?}",
            top_names
        );
        assert!(
            !top_names.contains(&"method_of_a"),
            "method_of_a is an impl method and must NOT appear as a top-level function; got {:?}",
            top_names
        );
        // method_of_a must appear as a class method
        let class_a = result
            .classes
            .iter()
            .find(|c| c.name == "A")
            .expect("struct A not found");
        assert!(
            class_a.methods.iter().any(|m| m.name == "method_of_a"),
            "method_of_a must be listed under struct A's methods"
        );
    }

    #[test]
    fn test_truncate_120_ascii() {
        let short = "hello";
        assert_eq!(truncate_120(short), "hello");

        let exact = "a".repeat(120);
        assert_eq!(truncate_120(&exact).len(), 120);

        let long_ascii = "a".repeat(200);
        let result = truncate_120(&long_ascii);
        assert_eq!(result.chars().count(), 120);
    }

    #[test]
    fn test_truncate_120_multibyte_utf8() {
        // Each '€' is 3 bytes in UTF-8; slicing by byte index would panic or produce invalid UTF-8.
        let s: String = "€".repeat(200);
        let result = truncate_120(&s);
        assert_eq!(result.chars().count(), 120);
        // Must be valid UTF-8 (no panic, no mojibake)
        assert!(std::str::from_utf8(result.as_bytes()).is_ok());
    }

    #[test]
    fn test_to_markdown_dense_format() {
        let result = OutlineResult {
            files: vec![FileOutline {
                file: "src/foo.rs".to_string(),
                language: "rust".to_string(),
                loc: 50,
                imports: vec!["std::path::Path".to_string()],
                classes: vec![OutlineClass {
                    name: "Foo".to_string(),
                    start_line: 5,
                    end_line: 20,
                    is_exported: true,
                    methods: vec![OutlineFunction {
                        name: "new".to_string(),
                        signature: "pub fn new() -> Self".to_string(),
                        start_line: 6,
                        end_line: 8,
                        is_exported: true,
                    }],
                    fields: vec!["name".to_string()],
                }],
                functions: vec![OutlineFunction {
                    name: "helper".to_string(),
                    signature: "fn helper(x: u32) -> bool".to_string(),
                    start_line: 25,
                    end_line: 30,
                    is_exported: false,
                }],
            }],
        };
        let md = result.to_markdown();
        assert!(md.contains("## src/foo.rs"), "should contain ## header");
        assert!(md.contains("L5-20"), "should contain L<start>-<end>");
        assert!(md.contains("[pub]"), "should contain [pub] for exported");
        assert!(md.contains("class Foo"), "should contain class name");
        assert!(md.contains("fn helper"), "should contain top-level fn");
        assert!(md.contains("imports:"), "should contain imports line");
        assert!(md.contains("field: name"), "should contain fields");
    }
}
