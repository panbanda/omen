use omen::analyzers::repomap::build_index;
use omen::task_context::{build, cache::IndexCache, Options};
use tempfile::TempDir;

#[test]
fn unchanged_then_same_size_edit_then_delete_matches_fresh_graph() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    std::fs::write(&a, "fn caller(){help();}").unwrap();
    std::fs::write(&b, "fn help(){}").unwrap();
    let mut cache = IndexCache::default();
    let paths = vec![a.clone(), b.clone()];
    let first = cache.load(dir.path(), &paths).unwrap();
    assert_eq!(first.parsed_files, 2);
    let second = cache.load(dir.path(), &paths).unwrap();
    assert_eq!(second.parsed_files, 0);
    assert!(!second.graph_rebuilt);
    assert!(std::sync::Arc::ptr_eq(&first.index, &second.index));
    std::fs::write(&b, "fn nope(){}").unwrap();
    let edited = cache.load(dir.path(), &paths).unwrap();
    assert_eq!(edited.parsed_files, 1);
    assert_eq!(edited.reused_files, 1);
    assert_ne!(edited.fingerprint, first.fingerprint);
    assert_eq!(
        edited.index.call_resolutions,
        build_index(dir.path(), &paths).unwrap().call_resolutions
    );
    std::fs::remove_file(&b).unwrap();
    let deleted = cache.load(dir.path(), std::slice::from_ref(&a)).unwrap();
    assert_eq!(deleted.parsed_files, 0);
    assert!(deleted.graph_rebuilt);
    assert_eq!(deleted.index.symbols.len(), 1);
    assert_eq!(
        deleted.index.call_resolutions,
        build_index(dir.path(), &[a]).unwrap().call_resolutions
    );
}

#[test]
fn cache_isolates_repository_roots_and_cannot_escape_root() {
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let fa = a.path().join("a.rs");
    let fb = b.path().join("a.rs");
    std::fs::write(&fa, "fn a(){}").unwrap();
    std::fs::write(&fb, "fn b(){}").unwrap();
    let mut cache = IndexCache::default();
    cache.load(a.path(), &[fa]).unwrap();
    assert!(cache.load(a.path(), std::slice::from_ref(&fb)).is_err());
    let snapshot = cache.load(b.path(), &[fb]).unwrap();
    assert_eq!(snapshot.parsed_files, 1);
    assert_eq!(snapshot.index.symbols[0].name, "b");
}

#[test]
fn bounded_context_retains_seed_and_discovers_transitive_test_candidates() {
    for (file, source) in [
        ("main.rs", "fn helper(){}\nfn service(){helper();}\nfn wrapper(){service();}\nfn test_service(){wrapper();}"),
        ("main.ts", "function helper(){}\nfunction service(){helper();}\nfunction wrapper(){service();}\nfunction test_service(){wrapper();}"),
        ("main.rb", "def helper; end\ndef service; helper(); end\ndef wrapper; service(); end\ndef test_service; wrapper(); end"),
        ("main.go", "package main\nfunc helper(){}\nfunc service(){helper()}\nfunc wrapper(){service()}\nfunc TestService(){wrapper()}"),
    ] {
        let dir = TempDir::new().unwrap(); let path = dir.path().join(file);
        std::fs::write(&path, source).unwrap();
        let snapshot = IndexCache::default().load(dir.path(), &[path]).unwrap();
        let report = build(&snapshot, dir.path(), "service", &Options::default()).unwrap();
        assert_eq!(report["symbols"].as_array().unwrap().len(), 4, "{file}");
        assert!(report["symbols"].as_array().unwrap().iter().any(|symbol|
            symbol["role_hints"].as_array().unwrap().iter().any(|role| role["role"] == "test_candidate")), "{file}");
        let budget = 1800;
        let report = build(&snapshot, dir.path(), "service", &Options { max_bytes: budget, ..Options::default() }).unwrap();
        assert!(report.to_string().len() <= budget);
        assert_eq!(report["symbols"][0]["relation"], "seed");
        assert_eq!(report["symbols"][0]["location"], report["seed"]);
    }
}

#[test]
fn ambiguity_and_unavailable_history_remain_explicit() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("a.rs");
    std::fs::write(
        &path,
        "struct A;struct B;\nimpl A {fn helper(){}}\nimpl B {fn helper(){}}\n",
    )
    .unwrap();
    let snapshot = IndexCache::default().load(dir.path(), &[path]).unwrap();
    let report = build(&snapshot, dir.path(), "helper", &Options::default()).unwrap();
    assert_eq!(report["ambiguous"], true);
    assert!(report.get("symbols").is_none());
    let selected = build(
        &snapshot,
        dir.path(),
        "a.rs:helper",
        &Options {
            start_line: Some(2),
            include_history: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(selected["seed"]["line"], 2);
    assert!(selected["history_status"].get("unavailable").is_some());
    assert!(build(
        &snapshot,
        dir.path(),
        "helper",
        &Options {
            depth: 5,
            ..Options::default()
        }
    )
    .is_err());
}

#[test]
fn syntax_facts_skip_nested_function_uses_and_label_framework_conventions() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("controllers/user.ts");
    std::fs::create_dir(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "function handler() {let result = value; function inner() { secret(); } consume(result);}",
    )
    .unwrap();
    let snapshot = IndexCache::default().load(dir.path(), &[path]).unwrap();
    let report = build(&snapshot, dir.path(), "handler", &Options::default()).unwrap();
    let uses = report["syntax_facts"]["uses"].as_array().unwrap();
    assert!(uses
        .iter()
        .any(|u| u["name"] == "result" && u["access"] == "syntactic_write"));
    assert!(!uses.iter().any(|u| u["name"] == "secret"));
    assert_eq!(
        report["symbols"][0]["role_hints"][0]["basis"],
        "directory_convention"
    );
    assert_eq!(report["symbols"][0]["role_hints"][0]["role"], "controller");
}

#[test]
fn cochange_hints_are_not_promoted_to_semantic_edges() {
    let dir = TempDir::new().unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Test Fixture"]);
    git(&["config", "user.email", "me@jonathanreyes.com"]);
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    for version in 0..4 {
        std::fs::write(&a, format!("fn service() {{ let value = {version}; }}\n")).unwrap();
        std::fs::write(&b, format!("fn other() {{ let value = {version}; }}\n")).unwrap();
        git(&["add", "a.rs", "b.rs"]);
        git(&["commit", "-qm", "paired fixture edit"]);
    }
    let snapshot = IndexCache::default().load(dir.path(), &[a, b]).unwrap();
    let report = build(
        &snapshot,
        dir.path(),
        "service",
        &Options {
            include_history: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(report["history_status"], "available");
    assert!(report["history_hints"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["file"] == "b.rs"));
    assert_eq!(report["symbols"].as_array().unwrap().len(), 1);
}

#[test]
fn wide_unicode_neighborhood_has_exact_omission_counts_and_byte_bounds() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("模块.ts");
    let mut source = "function service() {}\n".to_string();
    for i in 0..100 {
        source.push_str(&format!("function caller_{i}() {{ service(); }}\n"));
    }
    std::fs::write(&path, source).unwrap();
    let snapshot = IndexCache::default().load(dir.path(), &[path]).unwrap();
    for budget in [1800, 3000, 6000] {
        let report = build(
            &snapshot,
            dir.path(),
            "service",
            &Options {
                max_bytes: budget,
                ..Options::default()
            },
        )
        .unwrap();
        assert!(report.to_string().len() <= budget);
        assert_eq!(
            report["symbols"].as_array().unwrap().len() as u64
                + report["omitted"]["symbols"].as_u64().unwrap(),
            101
        );
    }
}
