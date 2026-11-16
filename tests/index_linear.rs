//! The cost of index building must be linear in the size of the input,
//! because omen analyzes repositories that it does not trust.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use omen::analyzers::cohesion::{Analysis, Analyzer};
use omen::index::build::CodeIndex;

fn build(file: &str, source: &str) -> CodeIndex {
    let read = |path: &Path| {
        if path == Path::new(file) {
            Ok(source.as_bytes().to_vec())
        } else {
            Err(omen::core::Error::git("no such file".to_string()))
        }
    };
    let list = |_: &Path| -> Vec<PathBuf> { Vec::new() };
    CodeIndex::build(Path::new("."), &[PathBuf::from(file)], &read, &list)
}

fn analyze(sources: &[(String, String)]) -> Analysis {
    let read = |path: &Path| {
        sources
            .iter()
            .find(|(name, _)| Path::new(name) == path)
            .map(|(_, text)| text.as_bytes().to_vec())
            .ok_or_else(|| omen::core::Error::git("no such file".to_string()))
    };
    let list = |_: &Path| -> Vec<PathBuf> { Vec::new() };
    let files: Vec<PathBuf> = sources
        .iter()
        .map(|(name, _)| PathBuf::from(name))
        .collect();
    let index = CodeIndex::build(Path::new("."), &files, &read, &list);
    Analyzer::new().analyze_index(&index, &HashSet::new())
}

fn timed_build(file: &str, source: &str) -> Duration {
    let started = std::time::Instant::now();
    let index = build(file, source);
    let elapsed = started.elapsed();
    drop(index);
    elapsed
}

fn nested_csharp(depth: usize) -> String {
    let mut source = String::from("class Part {}\nclass Host {\n  void Run() {\n");
    for _ in 0..depth {
        source.push_str("{ var part = new Part();\n");
    }
    source.push_str(&"}".repeat(depth));
    source.push_str("\n  }\n}\n");
    source
}

fn nested_javascript(depth: usize) -> String {
    let mut source = String::from("class Part {}\nfunction run() {\n");
    for _ in 0..depth {
        source.push_str("{ const part = new Part();\n");
    }
    source.push_str(&"}".repeat(depth));
    source.push_str("\n}\n");
    source
}

fn nested_rust(depth: usize) -> String {
    let mut source = String::from("struct Part;\nfn run() {\n");
    for _ in 0..depth {
        source.push_str("{ let part = Part;\n");
    }
    source.push_str(&"}".repeat(depth));
    source.push_str("\n}\n");
    source
}

fn nested_blocks_at(depth: usize) -> Vec<(&'static str, String)> {
    let nested = |head: &str, open: &str, close: &str, tail: &str| {
        format!("{head}{}{}{tail}", open.repeat(depth), close.repeat(depth))
    };
    vec![
        (
            "Deep.java",
            nested(
                "class Part {}\nclass Host {\n void run() {\n",
                "{ Part part = new Part();\n",
                "}",
                "\n}\n}\n",
            ),
        ),
        (
            "deep.go",
            nested(
                "package p\ntype Part struct{}\nfunc run() {\n",
                "{ part := Part{}\n",
                "}",
                "\n}\n",
            ),
        ),
        (
            "deep.c",
            nested(
                "struct Part { int a; };\nvoid run(void) {\n",
                "{ struct Part part;\n",
                "}",
                "\n}\n",
            ),
        ),
        (
            "deep.cpp",
            nested(
                "struct Part {};\nvoid run() {\n",
                "{ Part part;\n",
                "}",
                "\n}\n",
            ),
        ),
        (
            "deep.php",
            nested(
                "<?php\nclass Part {}\nfunction run() {\n",
                "{ $part = new Part();\n",
                "}",
                "\n}\n",
            ),
        ),
        (
            "deep.rb",
            nested(
                "class Part; end\ndef run\n",
                "if true\n part = Part.new\n",
                "end\n",
                "end\n",
            ),
        ),
        ("deep.cs", nested_csharp(depth)),
        ("deep.js", nested_javascript(depth)),
        ("deep.rs", nested_rust(depth)),
    ]
}

#[test]
fn test_deeply_nested_blocks_are_indexed() {
    for (file, source) in nested_blocks_at(2_000) {
        let index = build(file, &source);
        assert!(!index.files.is_empty(), "{file}");
    }
}

#[test]
#[ignore = "timing: run in release"]
fn timing_deeply_nested_blocks_are_linear() {
    for (file, source) in nested_blocks_at(20_000) {
        let elapsed = timed_build(file, &source);
        eprintln!("{file}: {elapsed:?}");
        assert!(elapsed.as_secs() < 30, "{file}: {elapsed:?}");
    }
}

fn classes_with_methods(size: usize) -> Vec<(&'static str, String)> {
    let class = |file: &'static str, head: &str, method: &str, tail: &str| {
        let mut source = String::from(head);
        for index in 0..size {
            source.push_str(&method.replace("{n}", &index.to_string()));
            source.push('\n');
        }
        source.push_str(tail);
        (file, source)
    };
    vec![
        class(
            "big.cs",
            "class Part {}\nclass Factory {\n",
            "  Part M{n}() { return new Part(); }",
            "}\n",
        ),
        class(
            "Big.java",
            "class Part {}\nclass Factory {\n",
            "  Part m{n}() { return new Part(); }",
            "}\n",
        ),
        class(
            "big.rb",
            "class Part; end\nclass Factory\n",
            "  def m{n}\n    Part.new\n  end",
            "end\n",
        ),
        class(
            "big.py",
            "class Part: pass\nclass Factory:\n",
            "    def m{n}(self):\n        return Part()",
            "",
        ),
        class(
            "big.php",
            "<?php\nclass Part {}\nclass Factory {\n",
            "  function m{n}() { return new Part(); }",
            "}\n",
        ),
        class(
            "big.ts",
            "class Part {}\nclass Factory {\n",
            "  m{n}(): Part { return new Part(); }",
            "}\n",
        ),
        class(
            "big.cpp",
            "struct Part {};\nstruct Factory {\n",
            "  Part m{n}() { return Part(); }",
            "};\n",
        ),
        class(
            "big.rs",
            "struct Part;\nstruct Factory;\nimpl Factory {\n",
            "  fn m{n}() -> Part { Part }",
            "}\n",
        ),
    ]
}

#[test]
fn test_class_with_many_methods_is_indexed() {
    for (file, source) in classes_with_methods(2_000) {
        let index = build(file, &source);
        assert!(!index.files.is_empty(), "{file}");
    }
}

#[test]
#[ignore = "timing: run in release"]
fn timing_class_with_many_methods_is_linear() {
    for (file, source) in classes_with_methods(50_000) {
        let elapsed = timed_build(file, &source);
        eprintln!("{file}: {elapsed:?}");
        assert!(elapsed.as_secs() < 30, "{file}: {elapsed:?}");
    }
}

fn inheritance_chain(length: usize) -> Vec<(String, String)> {
    let mut source = String::from("class C0 {}\n");
    for index in 1..length {
        source.push_str(&format!("class C{index} : C{} {{}}\n", index - 1));
    }
    vec![("chain.cs".to_string(), source)]
}

#[test]
fn test_inheritance_chain_depth_is_exact() {
    let analysis = analyze(&inheritance_chain(2_000));
    let deepest = analysis.classes.iter().map(|class| class.dit).max();
    assert_eq!(deepest, Some(1_999));
}

#[test]
#[ignore = "timing: run in release"]
fn timing_inheritance_chain_is_linear() {
    let sources = inheritance_chain(100_000);
    let started = std::time::Instant::now();
    let analysis = analyze(&sources);
    let elapsed = started.elapsed();
    eprintln!("{elapsed:?}");
    assert_eq!(analysis.classes.len(), 100_000);
    assert!(elapsed.as_secs() < 30, "{elapsed:?}");
}

fn constructor_call_classes(size: usize) -> Vec<(&'static str, String)> {
    classes_with_methods(size)
        .into_iter()
        .filter(|(file, _)| matches!(*file, "big.py" | "big.cpp"))
        .collect()
}

#[test]
fn test_constructor_calls_in_class_with_many_methods_resolve() {
    for (file, source) in constructor_call_classes(2_000) {
        let index = build(file, &source);
        assert!(!index.files.is_empty(), "{file}");
    }
}

#[test]
#[ignore = "timing: run in release"]
fn timing_constructor_calls_in_large_class_are_linear() {
    for (file, source) in constructor_call_classes(200_000) {
        let elapsed = timed_build(file, &source);
        eprintln!("{file}: {elapsed:?}");
        assert!(elapsed.as_secs() < 30, "{file}: {elapsed:?}");
    }
}

#[test]
fn test_csharp_nested_blocks_are_indexed() {
    let index = build("deep.cs", &nested_csharp(2_000));
    assert!(!index.files.is_empty());
}

#[test]
#[ignore = "timing: run in release"]
fn timing_csharp_nested_blocks_are_linear() {
    let elapsed = timed_build("deep.cs", &nested_csharp(200_000));
    eprintln!("deep.cs: {elapsed:?}");
    assert!(elapsed.as_secs() < 30, "{elapsed:?}");
}
