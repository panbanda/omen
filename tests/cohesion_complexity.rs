//! WMC is the sum of the cyclomatic complexity of the methods, as `omen
//! complexity` computes it. Each test compares the decision points that the
//! index records for a definition with the result of the complexity analyzer.

use std::path::{Path, PathBuf};

use omen::analyzers::cohesion::Analyzer;
use omen::analyzers::complexity::analyze_symbol_complexity;
use omen::core::Language;
use omen::index::build::CodeIndex;
use omen::index::extract::extract_file;
use omen::parser::Parser;

/// Each definition that has a body must have the complexity that the complexity analyzer gives.
fn check_equal(file: &str, language: Language, source: &[u8]) -> usize {
    let parsed = Parser::new()
        .parse(source, language, Path::new(file))
        .unwrap();
    let facts = extract_file(&parsed, file);
    let mut checked = 0;
    if facts.members.decision_points.is_empty() {
        return checked;
    }
    for (position, symbol) in facts.symbols.iter().enumerate() {
        if !symbol.is_callable()
            || symbol.body_range.is_none()
            || facts.link_facts[position].named_by_binding
        {
            continue;
        }
        let expected = analyze_symbol_complexity(symbol, &parsed).cyclomatic;
        let recorded = 1 + facts.members.decision_points[position];
        assert_eq!(
            recorded, expected,
            "{file}: {} at line {}",
            symbol.name, symbol.range.start_line
        );
        checked += 1;
    }
    checked
}

const CONSTRUCTS: &[(&str, &str)] = &[
    (
        "A.java",
        "class A {\n    int f(int x, boolean y) {\n        do { x++; } while (x < 3);\n        if (x > 1 && y || !y) { x = y ? 1 : 2; }\n        try { x++; } catch (Exception e) { x--; }\n        switch (x) { case 1: break; default: break; }\n        for (int i : new int[0]) { x++; }\n        while (x > 0) { x--; }\n        Runnable r = () -> { if (y) { x++; } };\n        return x;\n    }\n}\n",
    ),
    (
        "A.cs",
        "class A {\n    int F(int x, bool y) {\n        do { x++; } while (x < 3);\n        if (x > 1 && y || !y) { x = y ? 1 : 2; }\n        try { x++; } catch (System.Exception e) { x--; }\n        switch (x) { case 1: break; default: break; }\n        foreach (var i in new int[0]) { x++; }\n        for (int i = 0; i < 2; i++) { x++; }\n        while (x > 0) { x--; }\n        return x;\n    }\n}\n",
    ),
    (
        "A.cpp",
        "class A {\npublic:\n    int f(int x, bool y) {\n        do { x++; } while (x < 3);\n        if (x > 1 && y || !y) { x = y ? 1 : 2; }\n        try { x++; } catch (int e) { x--; }\n        switch (x) { case 1: break; default: break; }\n        for (int i = 0; i < 2; i++) { x++; }\n        while (x > 0) { x--; }\n        return x;\n    }\n};\n",
    ),
    (
        "a.ts",
        "class A {\n  f(x: number, y: boolean): number {\n    do { x++; } while (x < 3);\n    if (x > 1 && y || !y) { x = y ? 1 : 2; }\n    try { x++; } catch (e) { x--; }\n    switch (x) { case 1: break; default: break; }\n    for (const i of [1]) { x++; }\n    for (const k in {}) { x++; }\n    while (x > 0) { x--; }\n    const g = () => { if (y) { x++; } };\n    return x ?? 1;\n  }\n}\n",
    ),
    (
        "a.tsx",
        "class A {\n  f(x: number, y: boolean): number {\n    do { x++; } while (x < 3);\n    if (x > 1 && y || !y) { x = y ? 1 : 2; }\n    return x;\n  }\n}\n",
    ),
    (
        "a.js",
        "class A {\n  f(x, y) {\n    do { x++; } while (x < 3);\n    if (x > 1 && y || !y) { x = y ? 1 : 2; }\n    try { x++; } catch (e) { x--; }\n    switch (x) { case 1: break; default: break; }\n    for (const i of [1]) { x++; }\n    while (x > 0) { x--; }\n    return x;\n  }\n}\n",
    ),
    (
        "a.jsx",
        "class A {\n  f(x, y) {\n    do { x++; } while (x < 3);\n    if (x > 1 && y || !y) { x = y ? 1 : 2; }\n    return x;\n  }\n}\n",
    ),
    (
        "a.py",
        "class A:\n    def f(self, x, y):\n        if x > 1 and y or not y:\n            x = 1 if y else 2\n        elif x:\n            pass\n        for i in range(3):\n            x += i\n        while x > 0:\n            x -= 1\n        try:\n            x += 1\n        except ValueError:\n            x -= 1\n        with open('a') as handle:\n            pass\n        return [i for i in range(x)]\n",
    ),
    (
        "a.rb",
        "class A\n  def f(x, y)\n    if x > 1 && y || !y\n      x = 1\n    elsif x\n      x = 2\n    end\n    unless y\n      x = 3\n    end\n    while x > 0\n      x -= 1\n    end\n    until x > 3\n      x += 1\n    end\n    for i in 1..3\n      x += i\n    end\n    case x\n    when 1 then x = 4\n    when 2 then x = 5\n    end\n    begin\n      x += 1\n    rescue StandardError\n      x -= 1\n    end\n    x > 1 ? 1 : 2\n    [1].each { |v| x += v if y }\n    x\n  end\nend\n",
    ),
    (
        "A.php",
        "<?php\nclass A {\n    function f($x, $y) {\n        do { $x++; } while ($x < 3);\n        if ($x > 1 && $y || !$y) { $x = 1; } elseif ($x) { $x = 2; }\n        foreach ([1] as $i) { $x++; }\n        for ($i = 0; $i < 2; $i++) { $x++; }\n        while ($x > 0) { $x--; }\n        try { $x++; } catch (Exception $e) { $x--; }\n        switch ($x) { case 1: break; default: break; }\n        $z = $y ? 1 : 2;\n        return $x;\n    }\n}\n",
    ),
    (
        "a.rs",
        "struct A { n: u32 }\nimpl A {\n    fn f(&self, y: bool) -> u32 {\n        let mut x = self.n;\n        if x > 1 && y || !y { x = 1; }\n        for i in 0..3 { x += i; }\n        while x > 0 { x -= 1; }\n        loop { break; }\n        match x { 1 => 2, _ => 3 };\n        let c = |v: u32| if v > 1 { v } else { 0 };\n        x\n    }\n}\n",
    ),
    (
        "a.go",
        "package a\n\ntype A struct{ n int }\n\nfunc (a *A) F(x int, y bool) int {\n\tif x > 1 && y || !y {\n\t\tx = 1\n\t}\n\tfor i := 0; i < 3; i++ {\n\t\tx += i\n\t}\n\tswitch x {\n\tcase 1:\n\t\tx = 2\n\tcase 2:\n\t\tx = 3\n\t}\n\tselect {\n\tdefault:\n\t}\n\tf := func() {\n\t\tif y {\n\t\t\tx++\n\t\t}\n\t}\n\tf()\n\treturn x\n}\n",
    ),
];

#[test]
fn test_recorded_decision_points_equal_the_complexity_analyzer_on_each_construct() {
    for (file, source) in CONSTRUCTS {
        let language = Language::detect(Path::new(file)).unwrap();
        let checked = check_equal(file, language, source.as_bytes());
        assert!(checked > 0, "{file} has no method");
    }
}

fn fixture_files(directory: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            fixture_files(&path, out);
        } else if Language::detect(&path).is_some() {
            out.push(path);
        }
    }
}

#[test]
fn test_recorded_decision_points_equal_the_complexity_analyzer_on_the_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut files = Vec::new();
    fixture_files(&root, &mut files);
    let mut languages = std::collections::BTreeSet::new();
    for path in files {
        let language = Language::detect(&path).unwrap();
        let source = std::fs::read(&path).unwrap();
        let name = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        if check_equal(&name, language, &source) > 0 {
            languages.insert(language.to_string());
        }
    }
    for language in [
        "Go",
        "Rust",
        "Python",
        "TypeScript",
        "JavaScript",
        "Java",
        "C#",
        "C++",
        "Ruby",
        "PHP",
    ] {
        assert!(languages.contains(language), "{language}");
    }
}

fn wmc(file: &str, source: &str) -> u32 {
    let read = |path: &Path| {
        (path == Path::new(file))
            .then(|| source.as_bytes().to_vec())
            .ok_or_else(|| omen::core::Error::git("no such file".to_string()))
    };
    let list = |_: &Path| -> Vec<PathBuf> { Vec::new() };
    let index = CodeIndex::build(Path::new("."), &[PathBuf::from(file)], &read, &list);
    let analysis = Analyzer::new().analyze_index(&index, &std::collections::HashSet::new());
    analysis.classes[0].wmc
}

#[test]
fn test_do_while_loop_adds_one_to_wmc() {
    let cases: &[(&str, &str)] = &[
        ("Poller.java", "class Poller {\n    void poll() {\n        do { tick(); } while (ready());\n    }\n}\n"),
        ("Poller.cs", "class Poller {\n    void Poll() {\n        do { Tick(); } while (Ready());\n    }\n}\n"),
        ("poller.cpp", "class Poller {\npublic:\n    void poll() {\n        do { tick(); } while (ready());\n    }\n};\n"),
        ("poller.js", "class Poller {\n  poll() {\n    do { tick(); } while (ready());\n  }\n}\n"),
        ("poller.ts", "class Poller {\n  poll() {\n    do { tick(); } while (ready());\n  }\n}\n"),
        ("poller.tsx", "class Poller {\n  poll() {\n    do { tick(); } while (ready());\n  }\n}\n"),
        ("poller.jsx", "class Poller {\n  poll() {\n    do { tick(); } while (ready());\n  }\n}\n"),
        ("Poller.php", "<?php\nclass Poller {\n    function poll() {\n        do { tick(); } while (ready());\n    }\n}\n"),
    ];
    for (file, source) in cases {
        assert_eq!(wmc(file, source), 2, "{file}");
    }
}

#[test]
fn test_boolean_operators_add_to_wmc() {
    let java =
        "class A {\n    boolean f(boolean x, boolean y) {\n        return x && y || x;\n    }\n}\n";
    assert_eq!(wmc("A.java", java), 3);
}
