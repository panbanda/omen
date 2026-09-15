//! Bounded, evidence-labelled context for one exact definition.
pub mod cache;
mod facts;

use crate::analyzers::repomap::SymbolLocation;
use crate::core::{is_test_file, Error, Result};
use cache::Snapshot;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub struct Options {
    pub start_line: Option<u32>,
    pub depth: usize,
    pub max_bytes: usize,
    pub include_history: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            start_line: None,
            depth: 2,
            max_bytes: 12_000,
            include_history: false,
        }
    }
}

pub fn build(snapshot: &Snapshot, root: &Path, name: &str, options: &Options) -> Result<Value> {
    if !(1024..=1_048_576).contains(&options.max_bytes) || options.depth > 4 {
        return Err(Error::analysis(
            "max_bytes must be 1024..1048576; depth must be 0..4",
        ));
    }
    let index = &snapshot.index;
    let matches: Vec<_> = index
        .resolve(name)
        .into_iter()
        .filter(|&i| {
            options
                .start_line
                .is_none_or(|line| index.symbols[i].line == line)
        })
        .collect();
    if matches.len() != 1 {
        let choices: Vec<_> = matches
            .iter()
            .map(|&i| SymbolLocation::from(&index.symbols[i]))
            .collect();
        let mut report = json!({"found": !matches.is_empty(), "ambiguous": matches.len() > 1,
            "choices": choices, "omitted": {"choices": 0},
            "hint": "Select qualified_name and start_line. No definition selected."});
        fit(&mut report, options.max_bytes, &["choices"])?;
        return Ok(report);
    }
    let seed = matches[0];
    let mut related = BTreeMap::new();
    related.insert(seed, ("seed", 0));
    for (relation, levels) in [
        ("caller", index.callers(&[seed], options.depth)),
        ("callee", index.callees(&[seed], options.depth)),
    ] {
        for (level, symbols) in levels.iter().enumerate() {
            for &i in symbols {
                related.entry(i).or_insert((relation, level + 1));
            }
        }
    }
    let mut related: Vec<_> = related.into_iter().collect();
    related.sort_by_key(|&(i, (_, depth))| {
        (
            i != seed,
            depth,
            !test_hint(&index.symbols[i].file, &index.symbols[i].name),
            i,
        )
    });
    let symbols: Vec<_> = related.iter().map(|&(i, (relation, depth))| {
        let symbol = &index.symbols[i];
        json!({"location": SymbolLocation::from(symbol), "signature": symbol.signature,
            "relation": relation, "depth": depth, "role_hints": role_hints(&symbol.file, &symbol.name)})
    }).collect();
    let calls: Vec<_> = index
        .call_resolutions
        .iter()
        .filter(|call| call.caller == SymbolLocation::from(&index.symbols[seed]))
        .collect();
    let symbol = &index.symbols[seed];
    let canonical = root.canonicalize()?.join(&symbol.file);
    let syntax = snapshot
        .sources
        .get(&canonical)
        .map(|parsed| facts::extract(parsed, symbol.body_start));
    let parse_errors = snapshot
        .sources
        .values()
        .filter(|p| p.tree.root_node().has_error())
        .count();
    let mut report = json!({"found": true, "ambiguous": false, "snapshot": snapshot.fingerprint,
        "diagnostics": {"files_with_parse_errors": parse_errors},
        "seed": SymbolLocation::from(symbol), "symbols": symbols, "call_evidence": calls,
        "syntax_facts": syntax, "history_hints": [], "history_status": "not_requested",
        "cache": {"parsed_files": snapshot.parsed_files, "reused_files": snapshot.reused_files,
            "graph_rebuilt": snapshot.graph_rebuilt},
        "omitted": {"symbols": 0, "call_evidence": 0, "history_hints": 0, "syntax_facts": 0},
        "limits": {"depth": options.depth, "max_result_bytes": options.max_bytes},
        "caveat": "Bounded name-candidate graph, not compiler bindings or exhaustive impact. Role/test hints are conventions, not coverage. Syntax uses are not def-use or taint flow. Snapshot is content-addressed, not an atomic cross-file checkout."});
    if options.include_history {
        match crate::analyzers::temporal::Analyzer::new().analyze_repo(root) {
            Ok(history) => {
                let hints: Vec<_> = history
                    .couplings
                    .into_iter()
                    .filter(|c| c.file_a == symbol.file || c.file_b == symbol.file)
                    .map(|c| {
                        json!({"file": if c.file_a == symbol.file {c.file_b} else {c.file_a},
                        "cochanges": c.cochange_count, "strength": c.coupling_strength,
                        "basis": "cochange_last_30_days_not_semantic_dependency"})
                    })
                    .collect();
                report["history_hints"] = json!(hints);
                report["history_status"] = json!("available");
            }
            Err(error) => report["history_status"] = json!({"unavailable": error.to_string()}),
        }
    }
    // Keep the seed even when its neighborhood is omitted. Never silently claim
    // a budget was met when an indivisible seed/metadata alone is too large.
    if report.to_string().len() > options.max_bytes && !report["syntax_facts"].is_null() {
        report["syntax_facts"] = Value::Null;
        report["omitted"]["syntax_facts"] = json!(1);
    }
    fit(
        &mut report,
        options.max_bytes,
        &["history_hints", "call_evidence", "symbols"],
    )?;
    Ok(report)
}

fn fit(value: &mut Value, max_bytes: usize, fields: &[&str]) -> Result<()> {
    let mut size = value.to_string().len();
    for field in fields {
        let minimum = usize::from(*field == "symbols");
        while size > max_bytes && value[*field].as_array().is_some_and(|v| v.len() > minimum) {
            if let Some(items) = value[*field].as_array_mut() {
                if let Some(removed) = items.pop() {
                    size = size
                        .saturating_sub(removed.to_string().len() + usize::from(!items.is_empty()));
                }
            }
            let count = value["omitted"][*field].as_u64().unwrap_or(0);
            value["omitted"][*field] = json!(count + 1);
            size = size.saturating_sub(count.to_string().len()) + (count + 1).to_string().len();
        }
    }
    // Independently verify the byte accounting before emitting a hard-budget result.
    if value.to_string().len() > max_bytes {
        return Err(Error::analysis(
            "Budget too small for seed and required metadata",
        ));
    }
    Ok(())
}

fn test_hint(file: &str, name: &str) -> bool {
    is_test_file(Path::new(file)) || name.starts_with("test_") || name.starts_with("Test")
}

fn role_hints(file: &str, name: &str) -> Vec<Value> {
    let mut roles = Vec::new();
    if test_hint(file, name) {
        roles.push(json!({"role": "test_candidate", "basis": "path_or_name_convention"}));
    }
    let components: Vec<_> = file.split('/').collect();
    for (component, role) in [
        ("controllers", "controller"),
        ("migrations", "migration"),
        ("schemas", "schema"),
        ("jobs", "job"),
        ("middleware", "middleware"),
    ] {
        if components.contains(&component) {
            roles.push(json!({"role": role, "basis": "directory_convention"}));
        }
    }
    roles
}
