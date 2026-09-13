use omen::analyzers::repomap::build_index;
use omen::symbol::{get_symbol, SymbolOptions};
use tempfile::TempDir;

fn fixture(files: &[(&str, &str)]) -> (TempDir, Vec<std::path::PathBuf>) {
    let dir = TempDir::new().unwrap();
    let paths = files
        .iter()
        .map(|(name, src)| {
            let path = dir.path().join(name);
            std::fs::write(&path, src).unwrap();
            path
        })
        .collect();
    (dir, paths)
}

#[test]
fn ambiguous_names_are_not_asserted_edges() {
    let (dir, paths) = fixture(&[
        ("main.rs", "fn caller(){helper(); external();}"),
        ("a.rs", "fn helper(){}"),
        ("z.rs", "fn helper(){}"),
    ]);
    let report = get_symbol(dir.path(), &paths, "caller", &SymbolOptions::default()).unwrap();
    assert!(report.callees.is_empty());
    let json = serde_json::to_value(report).unwrap();
    assert_eq!(json["call_resolutions"][0]["status"], "ambiguous");
    assert_eq!(
        json["call_resolutions"][0]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(json["call_resolutions"][1]["status"], "unresolved");
}

#[test]
fn qualified_lookup_preserves_duplicates() {
    let (dir, paths) = fixture(&[(
        "main.rs",
        "struct A; struct B; impl A {fn helper(){}} impl B {fn helper(){}}",
    )]);
    assert_eq!(
        build_index(dir.path(), &paths)
            .unwrap()
            .resolve("main.rs:helper")
            .len(),
        2
    );
}

#[test]
fn foreign_language_is_not_a_candidate() {
    let (dir, paths) = fixture(&[
        ("main.rs", "fn caller(){helper();}"),
        ("a.rb", "def helper\nend\n"),
        ("z.rs", "fn helper(){}"),
    ]);
    let report = get_symbol(dir.path(), &paths, "caller", &SymbolOptions::default()).unwrap();
    assert_eq!(report.callees[0].qualified_name, "z.rs:helper");
}

#[test]
fn direct_calls_survive_in_every_language() {
    for (ext, src) in [
        ("rs", "fn helper(){} fn caller(){helper();}"),
        (
            "go",
            "package main\nfunc helper(){}\nfunc caller(){helper()}",
        ),
        ("py", "def helper():\n pass\ndef caller():\n helper()\n"),
        ("ts", "function helper(){} function caller(){helper();}"),
        ("tsx", "function helper(){} function caller(){helper();}"),
        ("js", "function helper(){} function caller(){helper();}"),
        ("jsx", "function helper(){} function caller(){helper();}"),
        ("java", "class A {void helper(){} void caller(){helper();}}"),
        ("cs", "class A {void helper(){} void caller(){helper();}}"),
        ("c", "void helper(){} void caller(){helper();}"),
        ("cpp", "void helper(){} void caller(){helper();}"),
        ("rb", "def helper\nend\ndef caller\n helper()\nend\n"),
        (
            "php",
            "<?php function helper(){} function caller(){helper();}",
        ),
        ("sh", "helper() { :; }\ncaller() { helper; }\n"),
    ] {
        let filename = format!("main.{ext}");
        let (dir, paths) = fixture(&[(&filename, src)]);
        let report = get_symbol(dir.path(), &paths, "caller", &SymbolOptions::default()).unwrap();
        assert_eq!(report.callees.len(), 1, "{ext}");
    }
}

#[test]
fn nested_bodies_do_not_create_outer_edges() {
    for (ext, src) in [
        ("rs", "fn helper(){} fn caller(){fn inner(){helper();}}"),
        (
            "go",
            "package main\nfunc helper(){}\nfunc caller(){inner := func(){helper()}; _ = inner}",
        ),
        (
            "py",
            "def helper():\n pass\ndef caller():\n def inner():\n  helper()\n",
        ),
        (
            "ts",
            "function helper(){} function caller(){function inner(){helper();}}",
        ),
        (
            "tsx",
            "function helper(){} function caller(){const inner = ()=>helper();}",
        ),
        (
            "js",
            "function helper(){} function caller(){function inner(){helper();}}",
        ),
        (
            "jsx",
            "function helper(){} function caller(){const inner = ()=>helper();}",
        ),
        (
            "java",
            "class A {void helper(){} void caller(){Runnable inner = () -> helper();}}",
        ),
        (
            "cs",
            "class A {void helper(){} void caller(){System.Action inner = () => helper();}}",
        ),
        (
            "cpp",
            "void helper(){} void caller(){auto inner = [](){helper();};}",
        ),
        (
            "rb",
            "def helper\nend\ndef caller\n def inner\n  helper()\n end\nend\n",
        ),
        (
            "php",
            "<?php function helper(){} function caller(){function inner(){helper();}}",
        ),
        ("sh", "helper() { :; }\ncaller() { inner() { helper; }; }\n"),
    ] {
        let filename = format!("main.{ext}");
        let (dir, paths) = fixture(&[(&filename, src)]);
        let report = get_symbol(dir.path(), &paths, "caller", &SymbolOptions::default()).unwrap();
        assert!(report.callees.is_empty(), "{ext}: {:?}", report.callees);
    }
}

#[test]
fn input_order_does_not_change_evidence() {
    let (dir, mut paths) = fixture(&[
        ("main.rs", "fn caller(){helper();}"),
        ("a.rs", "fn helper(){}"),
        ("z.rs", "fn helper(){}"),
    ]);
    let a = serde_json::to_value(
        get_symbol(dir.path(), &paths, "caller", &SymbolOptions::default()).unwrap(),
    )
    .unwrap();
    paths.reverse();
    let b = serde_json::to_value(
        get_symbol(dir.path(), &paths, "caller", &SymbolOptions::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(a, b);
}

#[test]
fn same_line_symbols_keep_separate_evidence() {
    let (dir, paths) = fixture(&[(
        "main.rs",
        "struct A; struct B; impl A {fn run(){first();}} impl B {fn run(){second();}}",
    )]);
    let report = get_symbol(dir.path(), &paths, "main.rs:run", &SymbolOptions::default()).unwrap();
    assert_eq!(report.call_resolutions.len(), 1);
    assert_eq!(report.call_resolutions[0].call, "first");
    assert_eq!(report.candidates.len(), 1);
}

#[test]
fn invoked_nested_function_keeps_transitive_edge() {
    let (dir, paths) = fixture(&[(
        "main.rs",
        "fn helper(){} fn caller(){fn inner(){helper();} inner();}",
    )]);
    let index = build_index(dir.path(), &paths).unwrap();
    let levels = index.callees(&index.resolve("caller"), 2);
    assert_eq!(index.symbols[levels[0][0]].name, "inner");
    assert_eq!(index.symbols[levels[1][0]].name, "helper");
}

#[test]
fn javascript_family_remains_connected() {
    let (dir, paths) = fixture(&[
        ("main.ts", "function caller(){helper();}"),
        ("helper.js", "function helper(){}"),
    ]);
    let index = build_index(dir.path(), &paths).unwrap();
    assert_eq!(index.callees(&index.resolve("caller"), 1)[0].len(), 1);
}

#[test]
fn impact_reports_input_wide_uncertainty() {
    let (dir, paths) = fixture(&[
        ("main.rs", "fn caller(){helper(); external();}"),
        ("a.rs", "fn helper(){}"),
        ("z.rs", "fn helper(){}"),
    ]);
    let report = omen::analyzers::impact::analyze(
        dir.path(),
        &paths,
        "caller",
        2,
        omen::analyzers::impact::Direction::Both,
    )
    .unwrap();
    assert_eq!(report.resolution_summary.ambiguous, 1);
    assert_eq!(report.resolution_summary.unresolved, 1);
    assert_eq!(report.total_callees, 0);
}

#[test]
fn rust_self_calls_have_name_evidence() {
    let (dir, paths) = fixture(&[(
        "main.rs",
        "struct A; impl A {fn helper(&self){} fn caller(&self){self.helper();}}",
    )]);
    let report = get_symbol(dir.path(), &paths, "caller", &SymbolOptions::default()).unwrap();
    assert!(report.call_resolutions.iter().any(|c| c.call == "helper"));
}

#[test]
fn rust_generic_receiver_is_not_guessed_from_a_bare_name() {
    let (dir, paths) = fixture(&[
        (
            "main.rs",
            "fn caller<T: AsRef<str>>(path:T){path.as_ref();}",
        ),
        ("other.rs", "fn as_ref(){}"),
    ]);
    let report = get_symbol(dir.path(), &paths, "caller", &SymbolOptions::default()).unwrap();
    assert!(report.callees.is_empty());
}
