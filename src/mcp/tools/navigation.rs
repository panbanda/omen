//! The tools that an agent uses to find and read code.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{
    Arguments, HandlerResult, Pagination, Param, ParamDefault, ToolData, ToolOutput, ToolSpec,
    COMPACT, MAX_COUNT, PATH,
};
use crate::core::{AnalysisContext, Analyzer, FileSet};
use crate::git::GitRepo;
use crate::mcp::envelope::{ItemCount, Truncation};
use crate::mcp::error::{ErrorCode, ToolError};
use crate::mcp::McpServer;

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
            "The output format. With `markdown`, the result is text without an envelope.",
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

    let mut context = crate::context::build_context(
        &scope.path,
        &scope.file_set,
        &server.config,
        Some(max_symbols),
        arguments.count("max_risks"),
    )?;
    crate::context::apply_token_budget(&mut context, max_tokens);

    if arguments.string("format") == Some("markdown") {
        return Ok(ToolOutput::Text(context.render_markdown()));
    }
    Ok(ToolOutput::data(serde_json::to_value(&context)?))
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
    use crate::analyzers::outline::{outline_file, Analyzer as OutlineAnalyzer, OutlineResult};

    let path = server.requested_path(arguments)?;
    let result = if let Some(file) = arguments.string("file") {
        // A relative file path starts at `path`, not at the server root.
        let file = server.resolve_confined_path("file", Path::new(file), Some(&path))?;
        OutlineResult {
            files: vec![outline_file(&file)?],
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
    Ok(ToolOutput::data(serde_json::to_value(&result)?))
}

pub(super) static GET_SYMBOL: ToolSpec = ToolSpec {
    name: "get_symbol",
    description: "Gives the source, the location, the complexity, the callers, and the callees \
        of one symbol. \
        `call_resolutions` shows the unique, ambiguous, and unresolved candidates of each call. \
        The edges come from names, and a compiler does not prove them. \
        The graph does not include ambiguous and unresolved calls.",
    params: &[
        Param::string(
            "name",
            "The symbol name. Use a bare name or the qualified form `file:name`.",
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
        PATH,
        COMPACT,
    ],
    paginated: Pagination::Field("callers"),
    handler: get_symbol,
};

fn absolute_files(scope_path: &Path, file_set: &FileSet) -> Vec<PathBuf> {
    file_set.iter().map(|file| scope_path.join(file)).collect()
}

fn get_symbol(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    use crate::symbol::{report_from_index, suggest_symbols, SymbolOptions};

    let scope = server.scope(arguments)?;
    let name = arguments.string("name").unwrap_or_default();
    let options = SymbolOptions {
        include_source: arguments.boolean("include_source").unwrap_or(true),
        max_source_lines: arguments.count("max_source_lines").unwrap_or(200),
    };
    let start_line = arguments.count_u32("start_line");
    let files = absolute_files(&scope.path, &scope.file_set);

    let snapshot = server.load_index(&scope.path, &files)?;
    let index = &snapshot.index;
    let matches: Vec<usize> = index
        .resolve(name)
        .into_iter()
        .filter(|&symbol| start_line.is_none_or(|line| index.symbols[symbol].line == line))
        .collect();
    if matches.len() > 1 {
        let choices: Vec<_> = matches
            .into_iter()
            .map(|symbol| {
                json!({
                    "name": index.symbols[symbol].qualified_name,
                    "start_line": index.symbols[symbol].line,
                    "signature": index.symbols[symbol].signature
                })
            })
            .collect();
        return Err(symbol_ambiguous("get_symbol", name, choices));
    }
    match report_from_index(&scope.path, index, name, &options, start_line) {
        Ok(report) => Ok(ToolOutput::data(serde_json::to_value(&report)?)),
        Err(_) => Err(symbol_not_found(
            "name",
            name,
            suggest_symbols(&scope.path, &files, name)?,
        )),
    }
}

/// The error for a symbol name that agrees with no definition.
fn symbol_not_found(param: &'static str, name: &str, suggestions: Vec<String>) -> ToolError {
    let next = if suggestions.is_empty() {
        "Find the symbol with `outline` or `semantic_search`."
    } else {
        "Use a name from `suggestions`."
    };
    ToolError::new(
        ErrorCode::NotFound,
        format!("No definition agrees with the symbol `{name}`."),
    )
    .with_param(param)
    .with_received(&json!(name))
    .with_suggestions(suggestions)
    .with_next(next)
}

/// The error for a symbol name that agrees with more than one definition.
/// Each choice has a `name` and a `start_line` that select one definition.
fn symbol_ambiguous(tool: &str, name: &str, choices: Vec<Value>) -> ToolError {
    ToolError::new(
        ErrorCode::Ambiguous,
        format!(
            "{} definitions agree with the symbol `{name}`. The tool did not select one.",
            choices.len()
        ),
    )
    .with_param("name")
    .with_expected("a name and a start line that select one definition")
    .with_received(&json!(name))
    .with_suggestions(choices)
    .with_next(format!(
        "Call `{tool}` again with the `name` and the `start_line` of one suggestion."
    ))
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
    let result = crate::task_context::build(&snapshot, &scope.path, name, &options)?;
    if result["ambiguous"] == true {
        let choices: Vec<Value> = result["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|choice| json!({"name": choice["qualified_name"], "start_line": choice["line"]}))
            .collect();
        return Err(symbol_ambiguous("task_context", name, choices));
    }
    if result["found"] == false {
        return Err(symbol_not_found(
            "name",
            name,
            crate::symbol::suggest_symbols(&scope.path, &files, name)?,
        ));
    }
    let truncated = omitted_lists(&result);
    let returned = result["symbols"].as_array().map_or(0, Vec::len);
    let omitted = result["omitted"]["symbols"].as_u64().unwrap_or(0) as usize;
    Ok(ToolOutput::Data(ToolData {
        truncated,
        items: Some(ItemCount {
            total: returned + omitted,
            returned,
        }),
        ..ToolData::new(result)
    }))
}

/// The lists that the byte budget of `task_context` cut, in the sequence of
/// their paths. The counts come from the `omitted` object of the result.
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
        The limit applies to the symbols of all levels.",
    params: &[
        Param::string(
            "symbol",
            "The symbol name. Use a bare name or the qualified form `file:name`.",
        )
        .required(),
        Param::integer("depth", 0, MAX_COUNT, "The maximum depth of the traversal.")
            .with_default(ParamDefault::Integer(2)),
        Param::choice(
            "direction",
            &["callers", "callees", "both"],
            "The direction of the traversal.",
        )
        .with_default(ParamDefault::String("both")),
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
    use crate::analyzers::impact::{analyze, Direction};

    let scope = server.scope(arguments)?;
    let symbol = arguments.string("symbol").unwrap_or_default();
    let depth = arguments.count("depth").unwrap_or(2);
    let direction = match arguments.string("direction") {
        Some("callers") => Direction::Callers,
        Some("callees") => Direction::Callees,
        _ => Direction::Both,
    };
    let files = absolute_files(&scope.path, &scope.file_set);
    let report = analyze(&scope.path, &files, symbol, depth, direction)?;
    if report.resolved.is_empty() {
        return Err(symbol_not_found("symbol", symbol, report.suggestions));
    }
    Ok(ToolOutput::data(serde_json::to_value(&report)?))
}
