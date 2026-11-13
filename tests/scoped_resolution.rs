use omen::analyzers::repomap::{build_index, ResolutionStatus};
use tempfile::TempDir;

fn calls(files: &[(&str, &str)], caller: &str) -> Vec<omen::analyzers::repomap::CallResolution> {
    let dir = TempDir::new().unwrap();
    let paths: Vec<_> = files
        .iter()
        .map(|(name, source)| {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, source).unwrap();
            path
        })
        .collect();
    let index = build_index(dir.path(), &paths).unwrap();
    index
        .call_resolutions
        .into_iter()
        .filter(|r| r.caller.qualified_name == caller)
        .collect()
}

#[test]
fn nested_same_name_chooses_visible_definition() {
    for (file, source, line) in [
        (
            "main.ts",
            "function helper() {}\nfunction caller() {\n function helper() {}\n helper();\n}\n",
            3,
        ),
        (
            "main.rs",
            "fn helper() {}\nfn caller() {\n fn helper() {}\n helper();\n}\n",
            3,
        ),
        (
            "main.py",
            "def helper(): pass\ndef caller():\n def helper(): pass\n helper()\n",
            3,
        ),
    ] {
        let r = calls(&[(file, source)], &format!("{file}:caller"));
        assert_eq!(r[0].status, ResolutionStatus::UniqueCandidate, "{file}");
        assert_eq!(r[0].candidates[0].line, line, "{file}");
    }
}

#[test]
fn sibling_nested_definition_is_not_visible() {
    let r = calls(
        &[(
            "main.ts",
            "function other() {\n function helper() {}\n}\nfunction caller() { helper(); }\n",
        )],
        "main.ts:caller",
    );
    assert_eq!(r[0].status, ResolutionStatus::Unresolved);
}

#[test]
fn imported_alias_targets_original_definition() {
    for (file, source, target, target_source, distractor, distractor_source) in [
        (
            "main.ts",
            "import { helper as renamed } from './a';\nfunction caller() { renamed(); }",
            "a.ts",
            "export function helper() {}",
            "b.ts",
            "export function helper() {}",
        ),
        (
            "main.rs",
            "use crate::a::helper as renamed;\nfn caller() { renamed(); }",
            "a.rs",
            "pub fn helper() {}",
            "b.rs",
            "pub fn helper() {}",
        ),
    ] {
        let r = calls(
            &[
                (file, source),
                (target, target_source),
                (distractor, distractor_source),
            ],
            &format!("{file}:caller"),
        );
        assert_eq!(r[0].status, ResolutionStatus::UniqueCandidate, "{file}");
        assert_eq!(r[0].candidates[0].file, target, "{file}");
    }
}

#[test]
fn relative_import_does_not_match_an_unrelated_same_basename() {
    let r = calls(
        &[
            (
                "src/main.ts",
                "import { helper } from './a';\nfunction caller(){helper();}",
            ),
            ("src/a.ts", "export function helper() {}"),
            ("vendor/a.ts", "export function helper() {}"),
        ],
        "src/main.ts:caller",
    );
    assert_eq!(r[0].status, ResolutionStatus::UniqueCandidate);
    assert_eq!(r[0].candidates[0].file, "src/a.ts");
}

#[test]
fn enclosing_type_disambiguates_self_and_this() {
    for (file, source, expected) in [
        ("main.rs", "struct A; struct B;\nimpl A {\n fn helper(&self) {}\n fn caller(&self) { self.helper(); }\n}\nimpl B { fn helper(&self) {} }", 3),
        ("main.ts", "class A {\n helper() {}\n caller() { this.helper(); }\n}\nclass B { helper() {} }", 2),
        ("main.rb", "class A\n def helper; end\n def caller; self.helper(); end\nend\nclass B\n def helper; end\nend", 2),
    ] {
        let r = calls(&[(file, source)], &format!("{file}:caller"));
        assert_eq!(r[0].status, ResolutionStatus::UniqueCandidate, "{file}: {r:?}");
        assert_eq!(r[0].candidates[0].line, expected);
    }
}

#[test]
fn a_nested_definition_shadows_an_import_alias() {
    let r = calls(&[("main.ts", "import {helper as renamed} from './a';\nfunction caller(){\n function renamed() {}\n renamed();\n}"),
        ("a.ts", "export function helper() {}")], "main.ts:caller");
    assert_eq!(r[0].candidates[0].qualified_name, "main.ts:renamed");
}

#[test]
fn block_scoped_rust_alias_does_not_leak() {
    let r = calls(
        &[
            (
                "main.rs",
                "fn other(){use crate::a::helper as renamed;}\nfn caller(){renamed();}",
            ),
            ("a.rs", "pub fn helper() {}"),
        ],
        "main.rs:caller",
    );
    assert_eq!(r[0].status, ResolutionStatus::Unresolved);
}
