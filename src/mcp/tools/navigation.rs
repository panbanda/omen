//! The tools that an agent uses to find and read code.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{
    Arguments, HandlerResult, Pagination, Param, ParamDefault, ToolData, ToolOutput, ToolSpec,
    COMPACT, MAX_COUNT, PATH,
};
use crate::core::{AnalysisContext, Analyzer, FileSet};
use crate::git::GitRepo;
use crate::mcp::envelope::{Hint, IndexInfo, Truncation};
use crate::mcp::error::{ErrorCode, ToolError};
use crate::mcp::McpServer;
use crate::symbol::{LookupFailure, SymbolChoice};

const START_LINE: Param = Param::integer(
    "start_line",
    1,
    MAX_COUNT,
    "The line on which the definition starts. Use it to select one of two definitions with the same name.",
);

pub(super) static CONTEXT: ToolSpec = ToolSpec {
    name: "context",
    description: "Use this tool first. \
        It gives the top symbols by PageRank, the risks, the languages, the directory tree, \
        and the entry points of the repository. \
        The tool is slow on a large repository, thus call it one time.",
    params: &[
        PATH,
        Param::integer(
            "max_symbols",
            0,
            MAX_COUNT,
            "The maximum number of symbols. The default comes from `max_tokens`.",
        ),
        Param::integer("max_risks", 0, MAX_COUNT, "The maximum number of risks."),
        Param::integer(
            "max_tokens",
            1,
            MAX_COUNT,
            "The token budget. The tool removes content until the result is in the budget.",
        )
        .with_default(ParamDefault::Integer(8000)),
        Param::choice(
            "format",
            &["json", "markdown"],
            "The output format. With `markdown`, the result is text without an envelope. \
             `limit` and `offset` then apply to the symbols in the text.",
        )
        .with_default(ParamDefault::String("json")),
        COMPACT,
    ],
    paginated: Pagination::Field("top_symbols"),
    handler: context,
};

fn context(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    let scope = server.scope(arguments)?;
    let max_tokens = arguments.count("max_tokens").unwrap_or(8000);
    let max_symbols = arguments
        .count("max_symbols")
        .unwrap_or_else(|| crate::context::max_symbols_for_token_budget(max_tokens));

    let (mut context, totals) = crate::context::build_context_with_totals(
        &scope.path,
        &scope.file_set,
        &server.config,
        Some(max_symbols),
        arguments.count("max_risks"),
    )?;
    // The totals are the lengths before `max_symbols` and `max_risks`.
    let mut before_budget = context_list_lengths(&context);
    for (path, total) in &mut before_budget {
        match *path {
            "/risks" => *total = totals.risks.max(*total),
            "/top_symbols" => *total = totals.top_symbols.max(*total),
            _ => {}
        }
    }
    crate::context::apply_token_budget(&mut context, max_tokens);
    let truncated: Vec<Truncation> = before_budget
        .into_iter()
        .zip(context_list_lengths(&context))
        .filter(|((_, total), (_, returned))| returned < total)
        .map(|((path, total), (_, returned))| Truncation {
            path: path.to_string(),
            total,
            returned,
        })
        .collect();

    if arguments.string("format") == Some("markdown") {
        // The text has no envelope, thus apply the page to the symbols here.
        let offset = arguments.count("offset").unwrap_or(0);
        let limit = arguments.count("limit").unwrap_or(0);
        context
            .top_symbols
            .drain(0..offset.min(context.top_symbols.len()));
        if limit > 0 {
            context.top_symbols.truncate(limit);
        }
        return Ok(ToolOutput::Text(context.render_markdown()));
    }
    Ok(ToolOutput::Data(ToolData {
        truncated,
        ..ToolData::new(serde_json::to_value(&context)?)
    }))
}

/// The lists that the token budget of `context` can cut, with their lengths,
/// in the sequence of their paths.
fn context_list_lengths(context: &crate::context::Context) -> [(&'static str, usize); 5] {
    [
        ("/entry_points", context.entry_points.len()),
        ("/hints", context.hints.len()),
        ("/risks", context.risks.len()),
        ("/top_symbols", context.top_symbols.len()),
        ("/tree", context.tree.len()),
    ]
}

pub(super) static OUTLINE: ToolSpec = ToolSpec {
    name: "outline",
    description: "Gives the imports, the classes with their methods, and the top-level functions \
        of one file or of each file. \
        Use it before you read a full source file, because the result is small.",
    params: &[
        PATH,
        Param::string(
            "file",
            "The one file to outline. The default is each file below `path`.",
        ),
    ],
    paginated: Pagination::Field("files"),
    handler: outline,
};

fn outline(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    use crate::analyzers::outline::{outline_file_in, Analyzer as OutlineAnalyzer, OutlineResult};

    let path = server.requested_path(arguments)?;
    let mut result = if let Some(file) = arguments.string("file") {
        // A relative file path starts at `path`, not at the server root.
        let file = server.resolve_confined_path("file", Path::new(file), Some(&path))?;
        OutlineResult {
            files: vec![outline_file_in(&path, &file)?],
        }
    } else if path.is_file() {
        OutlineResult {
            files: vec![outline_file_in(&path, &path)?],
        }
    } else {
        let file_set = FileSet::from_path(&path, &server.config)?;
        let git_root = GitRepo::open(&path)
            .ok()
            .map(|repository| repository.root().to_path_buf());
        let mut analysis = AnalysisContext::new(&file_set, &server.config, Some(&path));
        if let Some(git_root) = &git_root {
            analysis = analysis.with_git_path(git_root);
        }
        OutlineAnalyzer.analyze(&analysis)?
    };
    // The analyzer gives absolute paths. The other tools give and accept
    // paths from `path`, thus give the same form here.
    let base = if path.is_dir() {
        path.as_path()
    } else {
        path.parent().unwrap_or(&path)
    };
    for file in &mut result.files {
        if let Ok(relative) = Path::new(&file.file).strip_prefix(base) {
            file.file = relative.to_string_lossy().replace('\\', "/");
        }
    }
    Ok(ToolOutput::data(serde_json::to_value(&result)?))
}

pub(super) static GET_SYMBOL: ToolSpec = ToolSpec {
    name: "get_symbol",
    description: "Gives the source, the location, the complexity, the callers, and the callees \
        of one symbol. \
        `call_resolutions` shows the unique, ambiguous, and unresolved candidates of each call. \
        The edges come from names, and a compiler does not prove them. \
        The graph does not include ambiguous and unresolved calls. \
        A name that agrees with more than one definition gives the `ambiguous` error.",
    params: &[
        Param::string(
            "name",
            "The symbol name: a bare name, the qualified form `file:name`, or the key of a definition.",
        )
        .required(),
        Param::boolean(
            "include_source",
            "If true, the result includes the source code.",
        )
        .with_default(ParamDefault::Boolean(true)),
        START_LINE,
        Param::integer(
            "max_source_lines",
            0,
            MAX_COUNT,
            "The maximum number of source lines in the result.",
        )
        .with_default(ParamDefault::Integer(200)),
        MIN_BASIS,
        PATH,
        COMPACT,
    ],
    paginated: Pagination::Field("callers"),
    handler: get_symbol,
};

/// The basis names from the weakest evidence to the strongest evidence, in the
/// order of `ResolutionBasis::RANKED`.
pub(super) const BASIS_NAMES: &[&str] = &[
    "name_only",
    "implementation",
    "same_package",
    "imported_module",
    "qualified_path",
    "receiver_declared_type",
    "enclosing_type",
    "imported_alias",
    "imported_name",
    "same_file",
    "lexical_scope",
    "scip",
];

const MIN_BASIS: Param = Param::choice(
    "min_basis",
    BASIS_NAMES,
    "The weakest basis of a call that gives a caller or a callee. \
     The value `name_only` adds each call whose one candidate has only the name.",
)
.with_default(ParamDefault::String("same_package"));

fn min_basis(arguments: &Arguments<'_>) -> crate::index::model::ResolutionBasis {
    arguments
        .string("min_basis")
        .and_then(crate::index::model::ResolutionBasis::from_name)
        .unwrap_or(crate::index::view::DEFAULT_MIN_BASIS)
}

fn absolute_files(scope_path: &Path, file_set: &FileSet) -> Vec<PathBuf> {
    file_set
        .index_files()
        .iter()
        .map(|file| scope_path.join(file))
        .collect()
}

fn get_symbol(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    use crate::symbol::{report_from_index, SymbolOptions};

    let scope = server.scope(arguments)?;
    let name = arguments.string("name").unwrap_or_default();
    let options = SymbolOptions {
        include_source: arguments.boolean("include_source").unwrap_or(true),
        max_source_lines: arguments.count("max_source_lines").unwrap_or(200),
        min_basis: min_basis(arguments),
    };
    let start_line = arguments.count_u32("start_line");
    let files = absolute_files(&scope.path, &scope.file_set);

    let index = server.load_view(&scope.path, &files)?;
    let index = index.as_ref();
    match report_from_index(&scope.path, index, name, &options, start_line) {
        Ok(mut report) => {
            let analysis =
                crate::analyzers::security::analyze_paths(&scope.path, &files, &server.config)?;
            report.security = Some(crate::index::security::symbol_security(
                &analysis,
                &report.qualified_name,
            ));
            let mut truncated = Vec::new();
            if report.source_truncated {
                truncated.push(Truncation {
                    path: "/source".to_string(),
                    total: (report.end_line.saturating_sub(report.start_line) + 1) as usize,
                    returned: report
                        .source
                        .as_deref()
                        .map_or(0, |source| source.lines().count()),
                });
            }
            let mut hints = Vec::new();
            if !report.callers.is_empty() || !report.callees.is_empty() {
                hints.push(Hint {
                    tool: "impact".to_string(),
                    arguments: json!({"symbol": report.qualified_name}),
                    reason: format!(
                        "Callers: {}. Callees: {}.",
                        report.callers.len(),
                        report.callees.len()
                    ),
                });
            }
            Ok(ToolOutput::Data(ToolData {
                truncated,
                hints,
                index: index_info(index),
                ..ToolData::new(serde_json::to_value(&report)?)
            }))
        }
        Err(crate::core::Error::SymbolLookup(failure)) => {
            Err(lookup_error(&GET_SYMBOL, "name", &failure))
        }
        Err(error) => Err(error.into()),
    }
}

/// The error for a symbol name that does not select one definition. Each
/// choice has the `arguments` that select only it.
fn lookup_error(tool: &ToolSpec, param: &'static str, failure: &LookupFailure) -> ToolError {
    let with_arguments = |choices: &[SymbolChoice], with_line: bool| -> Vec<Value> {
        choices
            .iter()
            .map(|choice| {
                let mut value = json!(choice);
                value["arguments"] = if with_line {
                    json!({ (param): choice.name, "start_line": choice.start_line })
                } else {
                    json!({ (param): choice.name })
                };
                value
            })
            .collect()
    };
    let same_call = format!(
        "Call `{}` again with the arguments of this call.",
        tool.name
    );
    match failure {
        LookupFailure::NotFound {
            name,
            start_line,
            suggestions,
        } => {
            let mut next = if suggestions.is_empty() {
                "Find the symbol with `outline` or `search`.".to_string()
            } else {
                "Use a name from `suggestions`.".to_string()
            };
            if start_line.is_some() {
                next.push_str(" Also correct `start_line` or remove it.");
            }
            ToolError::new(ErrorCode::NotFound, failure.to_string())
                .with_param(param)
                .with_received(&json!(name))
                .with_suggestions(suggestions.clone())
                .with_next(next)
        }
        LookupFailure::NotAtLine {
            start_line,
            choices,
            ..
        } => ToolError::new(ErrorCode::NotFound, failure.to_string())
            .with_param("start_line")
            .with_expected("the start line of a definition of the name")
            .with_received(&json!(start_line))
            .with_suggestions(with_arguments(choices, true))
            .with_next(format!(
                "{same_call} Correct `start_line` or remove it, or use the `arguments` of one \
                 suggestion in place of the arguments with the same names."
            )),
        LookupFailure::Ambiguous { name, choices } => {
            ToolError::new(ErrorCode::Ambiguous, failure.to_string())
                .with_param(param)
                .with_expected("a name, or a name and a start line, that select one definition")
                .with_received(&json!(name))
                .with_suggestions(with_arguments(choices, false))
                .with_next(format!(
                    "{same_call} Use the `arguments` of one suggestion in place of the \
                     arguments with the same names."
                ))
        }
    }
}

pub(super) static TASK_CONTEXT: ToolSpec = ToolSpec {
    name: "task_context",
    description: "Gives bounded context for one definition: candidate callers and callees, \
        test and role hints, syntax uses, and a content fingerprint. \
        The result is not a full impact analysis and not a compiler proof. \
        `max_bytes` controls the size, thus this tool has no `limit` and no `offset`.",
    params: &[
        Param::string(
            "name",
            "The symbol name. The qualified form `file:name` is best.",
        )
        .required(),
        START_LINE,
        Param::integer(
            "depth",
            0,
            4,
            "The radius of the call graph around the definition.",
        )
        .with_default(ParamDefault::Integer(2)),
        Param::integer(
            "max_bytes",
            1024,
            1_048_576,
            "The maximum size of `result` in UTF-8 bytes. The envelope is not in this budget.",
        )
        .with_default(ParamDefault::Integer(12_000)),
        Param::boolean(
            "include_history",
            "If true, the result includes files that changed together with this file \
             in the last 30 days. These hints are not semantic dependencies.",
        )
        .with_default(ParamDefault::Boolean(false)),
        PATH,
    ],
    paginated: Pagination::None,
    handler: task_context,
};

fn task_context(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    let scope = server.scope(arguments)?;
    let name = arguments.string("name").unwrap_or_default();
    let options = crate::task_context::Options {
        start_line: arguments.count_u32("start_line"),
        depth: arguments.count("depth").unwrap_or(2),
        max_bytes: arguments.count("max_bytes").unwrap_or(12_000),
        include_history: arguments.boolean("include_history").unwrap_or(false),
    };
    let files = absolute_files(&scope.path, &scope.file_set);
    let snapshot = server.load_index(&scope.path, &files)?;
    let result = match crate::task_context::build(&snapshot, &scope.path, name, &options) {
        Err(crate::core::Error::SymbolLookup(failure)) => {
            return Err(lookup_error(&TASK_CONTEXT, "name", &failure));
        }
        other => other?,
    };
    let truncated = omitted_lists(&result);
    let symbols = result["symbols"].as_array().map_or(0, Vec::len);
    Ok(ToolOutput::Data(ToolData {
        truncated,
        item_count: Some(symbols),
        index: Some(IndexInfo {
            fingerprint: snapshot.fingerprint.clone(),
            sources: snapshot.sources.len(),
        }),
        ..ToolData::new(result)
    }))
}

/// The lists and the objects that the byte budget of `task_context` cut, in
/// the sequence of their paths. The counts come from the `omitted` object of the result.
fn omitted_lists(result: &Value) -> Vec<Truncation> {
    let entry = |path: &str, list: &Value, omitted: &Value| {
        let returned = list.as_array().map_or(0, Vec::len);
        let omitted = omitted.as_u64().unwrap_or(0) as usize;
        (omitted > 0).then(|| Truncation {
            path: path.to_string(),
            total: returned + omitted,
            returned,
        })
    };
    let omitted = &result["omitted"];
    let syntax_facts = &result["syntax_facts"];
    [
        entry(
            "/call_evidence",
            &result["call_evidence"],
            &omitted["call_evidence"],
        ),
        entry(
            "/history_hints",
            &result["history_hints"],
            &omitted["history_hints"],
        ),
        entry("/symbols", &result["symbols"], &omitted["symbols"]),
        // The budget can remove the full object. Its count is then 1 of 1.
        entry("/syntax_facts", &Value::Null, &omitted["syntax_facts"]),
        entry(
            "/syntax_facts/uses",
            &syntax_facts["uses"],
            &syntax_facts["omitted_uses"],
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

pub(super) static IMPACT: ToolSpec = ToolSpec {
    name: "impact",
    description: "Gives the possible blast radius of a symbol: its callers and callees \
        to a given depth. \
        The edges come from names. The tool does not include ambiguous and unresolved calls, \
        thus the result is not a full safety check. \
        A name that agrees with more than one definition gives the `ambiguous` error. \
        The limit applies to the symbols of all levels.",
    params: &[
        Param::string(
            "symbol",
            "The symbol name: a bare name, the qualified form `file:name`, or the key of a definition.",
        )
        .required(),
        START_LINE,
        Param::integer("depth", 0, MAX_COUNT, "The maximum depth of the traversal.")
            .with_default(ParamDefault::Integer(2)),
        Param::choice(
            "direction",
            &["callers", "callees", "both"],
            "The direction of the traversal.",
        )
        .with_default(ParamDefault::String("both")),
        MIN_BASIS,
        PATH,
        COMPACT,
    ],
    paginated: Pagination::Nested {
        parents: &["callers", "callees"],
        child: "symbols",
    },
    handler: impact,
};

fn impact(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    use crate::analyzers::impact::{analyze_view, Direction};

    let scope = server.scope(arguments)?;
    let symbol = arguments.string("symbol").unwrap_or_default();
    let depth = arguments.count("depth").unwrap_or(2);
    let direction = match arguments.string("direction") {
        Some("callers") => Direction::Callers,
        Some("callees") => Direction::Callees,
        _ => Direction::Both,
    };
    let files = absolute_files(&scope.path, &scope.file_set);
    let view = server.load_view(&scope.path, &files)?;
    let start_line = arguments.count_u32("start_line");
    let mut report = match analyze_view(
        &view,
        symbol,
        start_line,
        depth,
        direction,
        min_basis(arguments),
    ) {
        Err(crate::core::Error::SymbolLookup(failure)) => {
            return Err(lookup_error(&IMPACT, "symbol", &failure));
        }
        other => other?,
    };
    let analysis = crate::analyzers::security::analyze_paths(&scope.path, &files, &server.config)?;
    report.security = Some(crate::index::security::symbol_security(
        &analysis,
        &report.resolved[0],
    ));
    // The traversal stops at `depth`. The totals without a limit tell if
    // the limit cut callers or callees.
    let all = analyze_view(
        &view,
        symbol,
        start_line,
        usize::MAX,
        direction,
        min_basis(arguments),
    )?;
    let mut truncated = Vec::new();
    for (path, total, returned) in [
        (
            "/callers/*/symbols",
            all.total_callers,
            report.total_callers,
        ),
        (
            "/callees/*/symbols",
            all.total_callees,
            report.total_callees,
        ),
    ] {
        if total > returned {
            truncated.push(Truncation {
                path: path.to_string(),
                total,
                returned,
            });
        }
    }
    let hints = report
        .resolved
        .iter()
        .take(MAX_ROOT_HINTS)
        .map(|name| Hint {
            tool: "get_symbol".to_string(),
            arguments: json!({"name": name}),
            reason: "The symbol is a root of this report.".to_string(),
        })
        .collect();
    Ok(ToolOutput::Data(ToolData {
        truncated,
        hints,
        index: index_info(&view),
        ..ToolData::new(serde_json::to_value(&report)?)
    }))
}

/// The largest number of root symbols of a report that get a hint.
const MAX_ROOT_HINTS: usize = 3;

fn index_info(view: &crate::index::view::CallView) -> Option<IndexInfo> {
    view.fingerprint.as_ref().map(|fingerprint| IndexInfo {
        fingerprint: fingerprint.clone(),
        sources: view.sources,
    })
}
