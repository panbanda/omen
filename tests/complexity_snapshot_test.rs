//! Snapshot tests that lock the public JSON schema/output of the complexity
//! analyzer. These exist to catch unintended changes when the analyzer is
//! modified for performance reasons (e.g. exposing new fields consumed by the
//! defect analyzer). When the schema legitimately changes, review the diff
//! and run `cargo insta accept`.

use std::path::PathBuf;

use omen::analyzers::complexity;
use omen::config::Config;
use omen::core::{AnalysisContext, Analyzer, FileSet};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Replace machine-specific absolute paths with a stable placeholder so the
/// snapshot is portable across machines.
fn normalize_paths(mut value: serde_json::Value, root: &str) -> serde_json::Value {
    fn walk(v: &mut serde_json::Value, root: &str) {
        match v {
            serde_json::Value::String(s) => {
                if let Some(stripped) = s.strip_prefix(root) {
                    *s = format!("[FIXTURE]{stripped}");
                }
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    walk(item, root);
                }
            }
            serde_json::Value::Object(map) => {
                for (_, val) in map.iter_mut() {
                    walk(val, root);
                }
            }
            _ => {}
        }
    }
    walk(&mut value, root);
    value
}

#[test]
fn complexity_sample_rs_snapshot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src = fixtures_dir().join("sample.rs");
    let dst = temp.path().join("sample.rs");
    std::fs::copy(&src, &dst).expect("copy fixture");

    let config = Config::default();
    let files = FileSet::from_path(temp.path(), &config).expect("file set");
    let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));
    let analyzer = complexity::Analyzer::new();
    let analysis = analyzer.analyze(&ctx).expect("analysis");

    let raw_root = temp.path().to_string_lossy().into_owned();
    let canonical_root = temp
        .path()
        .canonicalize()
        .expect("canonicalize")
        .to_string_lossy()
        .into_owned();
    let value = serde_json::to_value(&analysis).expect("serialize");
    let normalized = normalize_paths(value, &raw_root);
    let normalized = normalize_paths(normalized, &canonical_root);

    insta::assert_json_snapshot!("complexity_sample_rs", normalized);
}
