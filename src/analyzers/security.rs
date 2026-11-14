//! The security analyzer: the sinks, the entry points, and the paths between them.
//!
//! The report gives facts: "this call exists" and "this path exists". It does not say that
//! the code has a vulnerability. It does not follow data. A sink without a path has the state
//! `no_known_entry`, and that state does not say that the sink is safe.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::{Config, SecurityConfig};
use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Error, Language, Result};
use crate::index::model::ResolutionBasis;
use crate::index::security::analysis::{analyze_with_index, Analysis, Options};
use crate::index::security::entry::{EntryKind, EntryPoint, EntryRule};
use crate::index::security::reach::{ChokePoint, Coverage, SinkPath};
use crate::index::security::spec::{Predicate, SinkSpec, Target};
use crate::index::security::suppress::{InvalidSuppression, Suppressed};
use crate::index::security::{Sink, SinkBasis, SinkCategory};

/// The sections of the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    All,
    Sinks,
    Surface,
    Paths,
}

impl Section {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "all" => Some(Self::All),
            "sinks" => Some(Self::Sinks),
            "surface" => Some(Self::Surface),
            "paths" => Some(Self::Paths),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub sinks: usize,
    pub sinks_by_category: BTreeMap<String, usize>,
    pub entry_points: usize,
    pub entry_points_by_kind: BTreeMap<String, usize>,
    pub paths: usize,
    pub sinks_with_path: usize,
    pub sinks_with_no_known_entry: usize,
    /// The sinks that a comment `omen:allow` suppresses. They are not in `sinks`.
    pub suppressed: usize,
    pub invalid_suppressions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub summary: Summary,
    pub sinks: Vec<Sink>,
    pub suppressed: Vec<Suppressed>,
    pub invalid_suppressions: Vec<InvalidSuppression>,
    pub entry_points: Vec<EntryPoint>,
    pub paths: Vec<SinkPath>,
    /// The keys of the sinks that no entry point reaches. It is not "safe".
    pub no_known_entry: Vec<String>,
    pub choke_points: Vec<ChokePoint>,
    pub coverage: Coverage,
}

impl Report {
    pub fn from_analysis(analysis: Analysis) -> Self {
        let mut sinks_by_category = BTreeMap::new();
        for sink in &analysis.sinks {
            *sinks_by_category
                .entry(sink.category.name().to_string())
                .or_insert(0) += 1;
        }
        let mut entry_points_by_kind = BTreeMap::new();
        for entry in &analysis.entry_points {
            *entry_points_by_kind
                .entry(entry.kind.name().to_string())
                .or_insert(0) += 1;
        }
        let reachability = analysis.reachability;
        let with_path: std::collections::BTreeSet<&str> = reachability
            .paths
            .iter()
            .map(|path| path.sink.as_str())
            .collect();
        let summary = Summary {
            sinks: analysis.sinks.len(),
            sinks_by_category,
            entry_points: analysis.entry_points.len(),
            entry_points_by_kind,
            paths: reachability.paths.len(),
            sinks_with_path: with_path.len(),
            sinks_with_no_known_entry: reachability.no_known_entry.len(),
            suppressed: analysis.suppressed.len(),
            invalid_suppressions: analysis.invalid_suppressions.len(),
        };
        Self {
            summary,
            sinks: analysis.sinks,
            suppressed: analysis.suppressed,
            invalid_suppressions: analysis.invalid_suppressions,
            entry_points: analysis.entry_points,
            paths: reachability.paths,
            no_known_entry: reachability.no_known_entry,
            choke_points: reachability.choke_points,
            coverage: analysis.coverage,
        }
    }

    /// Keeps the sinks of one category, the paths of one entry point, and the paths to one sink.
    /// An entry point or a sink is given by its key. The summary keeps the totals.
    pub fn restricted(
        mut self,
        category: Option<&str>,
        entry: Option<&str>,
        sink: Option<&str>,
    ) -> Self {
        if let Some(category) = category {
            self.sinks.retain(|found| found.category.name() == category);
            let keys: std::collections::BTreeSet<&str> =
                self.sinks.iter().map(|found| found.key.as_str()).collect();
            self.paths.retain(|path| keys.contains(path.sink.as_str()));
            self.no_known_entry
                .retain(|key| keys.contains(key.as_str()));
        }
        if let Some(entry) = entry {
            self.paths.retain(|path| path.entry == entry);
        }
        if let Some(sink) = sink {
            self.paths.retain(|path| path.sink == sink);
        }
        self
    }

    /// The report as a JSON value, with the fields of one section.
    pub fn section(&self, section: Section) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        let keep: &[&str] = match section {
            Section::All => return value,
            Section::Sinks => &[
                "summary",
                "sinks",
                "suppressed",
                "invalid_suppressions",
                "coverage",
            ],
            Section::Surface => &["summary", "entry_points", "coverage"],
            Section::Paths => &[
                "summary",
                "paths",
                "no_known_entry",
                "choke_points",
                "coverage",
            ],
        };
        if let Some(map) = value.as_object_mut() {
            map.retain(|key, _| keep.contains(&key.as_str()));
        }
        value
    }

    /// The sinks that a gate counts: a category in the list, or any category when the list
    /// is empty, and a basis of `basis` or a stronger one.
    pub fn gated<'a>(sinks: &'a [Sink], categories: &[String], basis: SinkBasis) -> Vec<&'a Sink> {
        sinks
            .iter()
            .filter(|sink| {
                (categories.is_empty()
                    || categories.iter().any(|name| name == sink.category.name()))
                    && sink.basis >= basis
            })
            .collect()
    }
}

/// The analysis of the files of a context. The context gives the index and the content of
/// the files, thus a context of a commit gives the analysis of that commit.
pub fn analyze_context(ctx: &AnalysisContext<'_>) -> Result<Analysis> {
    let files: Vec<PathBuf> = ctx.files.iter().cloned().collect();
    let index = ctx.code_index()?;
    let options = options_of(&ctx.config.security)?;
    Ok(analyze_with_index(
        &index,
        ctx.root,
        &files,
        &|path| ctx.read_file(path),
        &options,
    ))
}

/// The analysis of files of the file system. The paths are absolute or relative to the
/// current directory, and `root` is their common root.
pub fn analyze_paths(
    root: &std::path::Path,
    files: &[PathBuf],
    config: &Config,
) -> Result<Analysis> {
    let options = options_of(&config.security)?;
    let read = |path: &std::path::Path| Ok(std::fs::read(path)?);
    let index = crate::index::build::CodeIndex::build(
        root,
        files,
        &read,
        &crate::index::build::list_directory,
    );
    Ok(analyze_with_index(&index, root, files, &read, &options))
}

/// The analyzer. Build it with [`Analyzer::from_config`].
#[derive(Default)]
pub struct Analyzer {
    options: Options,
}

fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

fn language_of(name: &str) -> Result<Language> {
    serde_json::from_value(Value::String(name.to_string()))
        .map_err(|_| Error::config(format!("security: `{name}` is not a language.")))
}

fn category_of(name: &str) -> Result<SinkCategory> {
    SinkCategory::ALL
        .iter()
        .copied()
        .find(|category| category.name() == name)
        .ok_or_else(|| Error::config(format!("security: `{name}` is not a sink category.")))
}

pub fn basis_of(name: &str) -> Result<SinkBasis> {
    [
        SinkBasis::NameOnly,
        SinkBasis::Builtin,
        SinkBasis::ImportedSymbol,
    ]
    .into_iter()
    .find(|basis| basis.name() == name)
    .ok_or_else(|| {
        Error::config(format!(
            "security: `{name}` is not a basis of a sink. Use name_only, builtin, or imported_symbol."
        ))
    })
}

/// The options of the analysis from the section `[security]` of the configuration.
///
/// The rows of the user live as long as the process, because the matcher keeps its rows
/// in static form.
pub fn options_of(config: &SecurityConfig) -> Result<Options> {
    let min_basis = ResolutionBasis::from_name(&config.min_basis).ok_or_else(|| {
        Error::config(format!(
            "security.min_basis: `{}` is not a resolution basis.",
            config.min_basis
        ))
    })?;
    let mut rows: Vec<SinkSpec> = Vec::new();
    for sink in &config.sinks {
        let languages: Vec<Language> = sink
            .languages
            .iter()
            .map(|name| language_of(name))
            .collect::<Result<_>>()?;
        rows.push(SinkSpec {
            category: category_of(&sink.category)?,
            languages: leak(languages),
            target: Target::Path {
                module: leak(sink.module.clone()),
                callee: leak(sink.callee.clone()),
            },
            predicate: Predicate::Always,
        });
    }
    let mut entry_rules = Vec::new();
    for entry in &config.entry_points {
        let kind = EntryKind::ALL
            .iter()
            .copied()
            .find(|kind| kind.name() == entry.kind)
            .ok_or_else(|| {
                Error::config(format!(
                    "security: `{}` is not an entry point kind.",
                    entry.kind
                ))
            })?;
        entry_rules.push(EntryRule {
            kind,
            framework: leak(entry.framework.clone()),
            languages: entry
                .languages
                .iter()
                .map(|name| language_of(name))
                .collect::<Result<_>>()?,
            symbols: entry.symbols.clone(),
        });
    }
    basis_of(&config.gate_basis)?;
    for name in &config.gate_categories {
        category_of(name)?;
    }
    Ok(Options {
        min_basis,
        sink_rows: Box::leak(rows.into_boxed_slice()),
        entry_rules,
        experimental: config.experimental,
    })
}

impl Analyzer {
    pub fn from_config(config: &Config) -> Result<Self> {
        Ok(Self {
            options: options_of(&config.security)?,
        })
    }

    /// Look for the sinks of the experimental categories also.
    pub fn with_experimental(mut self, experimental: bool) -> Self {
        self.options.experimental = experimental;
        self
    }

    /// The weakest basis of an edge of a path.
    pub fn with_min_basis(mut self, basis: ResolutionBasis) -> Self {
        self.options.min_basis = basis;
        self
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Report;

    fn name(&self) -> &'static str {
        "security"
    }

    fn description(&self) -> &'static str {
        "Sinks, entry points, and the paths between them"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Report> {
        let files: Vec<PathBuf> = ctx.files.iter().cloned().collect();
        let index = ctx.code_index()?;
        let analysis = analyze_with_index(
            &index,
            ctx.root,
            &files,
            &|path| ctx.read_file(path),
            &self.options,
        );
        Ok(Report::from_analysis(analysis))
    }
}

/// The report as SARIF 2.1.0. A result is a sink. The level is `warning` for a sink
/// with an import or a builtin as its basis, and `note` for a sink with only the name.
pub fn sarif(report: &Report) -> Value {
    let rules: Vec<Value> = SinkCategory::ALL
        .iter()
        .map(|category| {
            json!({
                "id": category.name(),
                "name": category.name(),
                "shortDescription": {
                    "text": format!("A call, a member, or a literal of the category {}.", category.name())
                },
                "properties": {"cwe": format!("CWE-{}", category.cwe())},
            })
        })
        .collect();
    let results: Vec<Value> = report
        .sinks
        .iter()
        .map(|sink| {
            let level = if sink.basis == SinkBasis::NameOnly {
                "note"
            } else {
                "warning"
            };
            json!({
                "ruleId": sink.category.name(),
                "level": level,
                "message": {
                    "text": format!(
                        "A {} sink (CWE-{}) exists: {}. The basis is {}.",
                        sink.category.name(), sink.cwe, sink.callee_text, sink.basis.name()
                    )
                },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": {"uri": sink.file},
                        "region": {
                            "startLine": sink.range.start_line,
                            "startColumn": sink.range.start_column + 1,
                        },
                    }
                }],
                "properties": {
                    "basis": sink.basis.name(),
                    "in_test": sink.flags.in_test,
                    "constant_argument": sink.flags.constant_argument,
                },
            })
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {"driver": {
                "name": "omen",
                "version": env!("CARGO_PKG_VERSION"),
                "informationUri": "https://github.com/panbanda/omen",
                "rules": rules,
            }},
            "results": results,
            "properties": {
                "coverage": report.coverage,
                "sinks_with_no_known_entry": report.summary.sinks_with_no_known_entry,
                "entry_points": report.summary.entry_points,
                "experimental_off": report.coverage.experimental_off,
            },
        }],
    })
}

fn name_of(key: &str) -> &str {
    key.rsplit_once('#').map_or(key, |(_, name)| name)
}

/// A Mermaid flowchart of the paths: one node for each symbol, one node for each entry
/// point, and one node for each sink. The chart has at most `limit` paths.
pub fn mermaid(report: &Report, limit: usize) -> String {
    let entries: BTreeMap<&str, &EntryPoint> = report
        .entry_points
        .iter()
        .map(|entry| (entry.key.as_str(), entry))
        .collect();
    let sinks: BTreeMap<&str, &Sink> = report
        .sinks
        .iter()
        .map(|sink| (sink.key.as_str(), sink))
        .collect();
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let mut text = String::from("flowchart LR\n");
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut node = |key: String, label: String, text: &mut String| -> String {
        let next = ids.len();
        ids.entry(key)
            .or_insert_with(|| {
                let id = format!("n{next}");
                text.push_str(&format!(
                    "  {id}[\"{}\"]\n",
                    crate::changeset::mermaid::escape(&label)
                ));
                id
            })
            .clone()
    };
    for path in report.paths.iter().take(limit) {
        let (Some(entry), Some(sink)) = (
            entries.get(path.entry.as_str()),
            sinks.get(path.sink.as_str()),
        ) else {
            continue;
        };
        let entry_label = match &entry.route {
            Some(route) => format!("{} {}", route.method, route.path),
            None => format!("{} {}", entry.framework, entry.kind.name()),
        };
        let mut previous = node(format!("entry:{}", entry.key), entry_label, &mut text);
        for step in &path.steps {
            let current = node(
                format!("symbol:{}", step.symbol),
                name_of(&step.symbol).to_string(),
                &mut text,
            );
            edges.push((previous, current.clone()));
            previous = current;
        }
        let last = node(
            format!("sink:{}", sink.key),
            format!("{} (CWE-{})", sink.category.name(), sink.cwe),
            &mut text,
        );
        edges.push((previous, last));
    }
    edges.dedup();
    let mut seen = std::collections::BTreeSet::new();
    for (from, to) in edges {
        if seen.insert((from.clone(), to.clone())) {
            text.push_str(&format!("  {from} --> {to}\n"));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::security::analysis::analyze_sources;
    use crate::index::view::DEFAULT_MIN_BASIS;

    const APP: &str = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.route(\"/run\", methods=[\"POST\"])\ndef run_command():\n    return helper()\n\ndef helper():\n    os.system(\"x\" + os.environ[\"A\"])\n\ndef dead(expression):\n    eval(expression)\n";

    fn report() -> Report {
        Report::from_analysis(analyze_sources(&[("app.py", APP)], DEFAULT_MIN_BASIS))
    }

    #[test]
    fn the_summary_counts_the_sinks_the_entry_points_and_the_paths() {
        let report = report();
        assert_eq!(report.summary.sinks, 2);
        assert_eq!(report.summary.sinks_by_category["command_execution"], 1);
        assert_eq!(report.summary.sinks_by_category["code_evaluation"], 1);
        assert_eq!(report.summary.entry_points, 1);
        assert_eq!(report.summary.entry_points_by_kind["http_handler"], 1);
        assert_eq!(report.summary.paths, 1);
        assert_eq!(report.summary.sinks_with_path, 1);
        assert_eq!(report.summary.sinks_with_no_known_entry, 1);
        assert_eq!(report.summary.suppressed, 0);
    }

    #[test]
    fn each_section_has_its_own_fields() {
        let report = report();
        let keys = |section| -> Vec<String> {
            let value = report.section(section);
            let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(
            keys(Section::Sinks),
            [
                "coverage",
                "invalid_suppressions",
                "sinks",
                "summary",
                "suppressed"
            ]
        );
        assert_eq!(
            keys(Section::Surface),
            ["coverage", "entry_points", "summary"]
        );
        assert_eq!(
            keys(Section::Paths),
            [
                "choke_points",
                "coverage",
                "no_known_entry",
                "paths",
                "summary"
            ]
        );
        assert_eq!(keys(Section::All).len(), 9);
    }

    #[test]
    fn the_filters_keep_one_category_one_entry_or_one_sink() {
        let report = report();
        let by_category = report
            .clone()
            .restricted(Some("code_evaluation"), None, None);
        assert_eq!(by_category.sinks.len(), 1);
        assert!(by_category.paths.is_empty());
        assert_eq!(by_category.no_known_entry.len(), 1);
        let entry = report.entry_points[0].key.clone();
        assert_eq!(
            report
                .clone()
                .restricted(None, Some(&entry), None)
                .paths
                .len(),
            1
        );
        assert!(report
            .clone()
            .restricted(None, Some("nobody"), None)
            .paths
            .is_empty());
        let sink = report.paths[0].sink.clone();
        assert_eq!(report.restricted(None, None, Some(&sink)).paths.len(), 1);
    }

    #[test]
    fn the_gate_counts_the_categories_and_the_basis_that_it_names() {
        let report = report();
        assert_eq!(
            Report::gated(&report.sinks, &[], SinkBasis::NameOnly).len(),
            2
        );
        assert_eq!(
            Report::gated(
                &report.sinks,
                &["code_evaluation".to_string()],
                SinkBasis::Builtin
            )
            .len(),
            1
        );
        assert_eq!(
            Report::gated(&report.sinks, &[], SinkBasis::ImportedSymbol).len(),
            1,
            "the builtin `eval` has a weaker basis than an import"
        );
    }

    #[test]
    fn sarif_has_a_rule_for_each_category_and_a_result_for_each_sink() {
        let report = report();
        let value = sarif(&report);
        assert_eq!(value["version"], "2.1.0");
        let run = &value["runs"][0];
        assert_eq!(run["tool"]["driver"]["rules"].as_array().unwrap().len(), 9);
        let results = run["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["ruleId"], "command_execution");
        assert_eq!(results[0]["level"], "warning");
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "app.py"
        );
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["region"]["startLine"],
            11
        );
    }

    #[test]
    fn sarif_carries_the_coverage_and_the_count_of_sinks_without_an_entry_point() {
        let report = report();
        let value = sarif(&report);
        let properties = &value["runs"][0]["properties"];
        assert_eq!(
            properties["coverage"]["statement"],
            report.coverage.statement.as_str()
        );
        assert_eq!(properties["sinks_with_no_known_entry"], 1);
        assert_eq!(properties["entry_points"], report.summary.entry_points);
        assert_eq!(
            properties["experimental_off"],
            json!(["hardcoded_credential"])
        );
    }

    #[test]
    fn the_mermaid_chart_has_the_entry_the_hops_and_the_sink() {
        let report = report();
        let text = mermaid(&report, 10);
        assert!(text.starts_with("flowchart LR\n"));
        assert!(text.contains("POST /run"));
        assert!(text.contains("run_command"));
        assert!(text.contains("helper"));
        assert!(text.contains("command_execution (CWE-78)"));
        assert_eq!(text.matches(" --> ").count(), 3);
        assert!(
            !text.contains("eval"),
            "a sink without a path is not in the chart"
        );
        assert_eq!(mermaid(&report, 0), "flowchart LR\n");
    }

    #[test]
    fn the_configuration_adds_sinks_and_entry_points_and_rejects_a_wrong_name() {
        let mut config = SecurityConfig::default();
        config.sinks.push(crate::config::SecuritySinkConfig {
            category: "command_execution".to_string(),
            languages: vec!["python".to_string()],
            module: "acme.shell".to_string(),
            callee: "run_checked".to_string(),
        });
        config
            .entry_points
            .push(crate::config::SecurityEntryConfig {
                kind: "http_handler".to_string(),
                framework: "acme-web".to_string(),
                languages: vec!["python".to_string()],
                symbols: vec!["handle_request".to_string()],
            });
        let options = options_of(&config).unwrap();
        assert_eq!(options.sink_rows.len(), 1);
        assert_eq!(options.entry_rules.len(), 1);
        for broken in [
            |config: &mut SecurityConfig| config.min_basis = "strongest".to_string(),
            |config: &mut SecurityConfig| config.gate_basis = "certain".to_string(),
            |config: &mut SecurityConfig| {
                config.gate_categories = vec!["weak_everything".to_string()]
            },
            |config: &mut SecurityConfig| config.sinks[0].languages = vec!["cobol".to_string()],
            |config: &mut SecurityConfig| config.entry_points[0].kind = "web".to_string(),
        ] {
            let mut wrong = config.clone();
            broken(&mut wrong);
            assert!(options_of(&wrong).is_err());
        }
    }

    #[test]
    fn a_configured_sink_and_entry_point_are_in_the_analysis() {
        use crate::index::security::analysis::analyze_with_index;
        let source = "import acme.shell as shell\n\ndef handle_request(command):\n    shell.run_checked(command)\n";
        let mut config = SecurityConfig::default();
        config.sinks.push(crate::config::SecuritySinkConfig {
            category: "command_execution".to_string(),
            languages: vec!["python".to_string()],
            module: "acme.shell".to_string(),
            callee: "run_checked".to_string(),
        });
        config
            .entry_points
            .push(crate::config::SecurityEntryConfig {
                kind: "http_handler".to_string(),
                framework: "acme-web".to_string(),
                languages: vec!["python".to_string()],
                symbols: vec!["handle_request".to_string()],
            });
        let options = options_of(&config).unwrap();
        let paths = vec![PathBuf::from("a.py")];
        let read = |_: &std::path::Path| Ok(source.as_bytes().to_vec());
        let index =
            crate::index::build::CodeIndex::build(std::path::Path::new(""), &paths, &read, &|_| {
                Vec::new()
            });
        let analysis =
            analyze_with_index(&index, std::path::Path::new(""), &paths, &read, &options);
        assert_eq!(analysis.sinks.len(), 1);
        assert_eq!(analysis.entry_points.len(), 1);
        assert_eq!(analysis.entry_points[0].framework, "acme-web");
        assert_eq!(analysis.reachability.paths.len(), 1);
    }
}
