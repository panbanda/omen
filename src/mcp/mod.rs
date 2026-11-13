//! MCP (Model Context Protocol) server implementation.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;
use crate::core::{AnalysisContext, Analyzer, FileSet, Result};
use crate::git::GitRepo;

mod compact;
mod envelope;
mod error;
mod tools;

/// Reference decoder for the `omen.tables.v1` compact encoding.
pub use compact::decode;

use error::{ErrorCode, ToolError};
use tools::{Arguments, HandlerResult, ToolData, ToolOutput};

const MAX_STDIO_LINE_SIZE: usize = 10 * 1024 * 1024;

/// The text that `initialize` gives to an agent. Keep it below 1200 characters.
const INSTRUCTIONS: &str = "\
Omen analyzes the source code of this repository.

Sequence:
1. Call `context` one time for the top symbols and the risks, or `outline` for the structure of a file.
2. Before you edit a symbol, call `get_symbol` to read its source, its callers, and its callees.
3. Before you change a signature, call `impact` to find the callers that the change can break.
4. Use `semantic_search` when you do not know the name of a symbol.

Pagination: `limit` has the default 50, and the value 0 removes the limit. \
When `has_more` is true, send the same call with `offset` set to `next_offset`. \
`truncated` lists each list in `result` that is not complete.

Errors: a result with `isError` true holds `error.code`, `error.param`, `error.expected`, \
and `error.suggestions`. Do what `error.next` tells you. \
The server does not replace an incorrect argument with a default. \
When the code is `ambiguous`, send the call again with the `name` and the `start_line` of one suggestion.

The call graph comes from names. A compiler does not prove it.";

fn discard_until_newline(reader: &mut impl BufRead) -> std::io::Result<()> {
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok(());
        }
        if let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
            reader.consume(position + 1);
            return Ok(());
        }
        let length = buffer.len();
        reader.consume(length);
    }
}

fn write_parse_error(writer: &mut impl Write, detail: &str) -> Result<()> {
    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: None,
        result: None,
        error: Some(JsonRpcError {
            code: -32700,
            message: format!("Parse error: {detail}"),
            data: None,
        }),
    };
    serde_json::to_writer(&mut *writer, &response)?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}

fn text_result(text: String) -> Value {
    json!({"content": [{"type": "text", "text": text}]})
}

/// The path and the files that one tool call analyzes.
struct Scope {
    path: PathBuf,
    file_set: FileSet,
}

/// MCP Server for LLM tool integration.
pub struct McpServer {
    config: Config,
    root_path: PathBuf,
    allow_external_paths: bool,
    index_cache: std::sync::Mutex<crate::task_context::cache::IndexCache>,
    /// The view of the shared index for `get_symbol` and `impact`.
    view_cache: std::sync::Mutex<crate::index::view::ViewCache>,
}

impl McpServer {
    pub fn new(root_path: PathBuf, config: Config) -> Self {
        Self::new_with_external_paths(root_path, config, false)
    }

    pub fn new_with_external_paths(
        root_path: PathBuf,
        config: Config,
        allow_external_paths: bool,
    ) -> Self {
        Self {
            config,
            root_path,
            allow_external_paths,
            index_cache: std::sync::Mutex::new(crate::task_context::cache::IndexCache::default()),
            view_cache: std::sync::Mutex::new(crate::index::view::ViewCache::default()),
        }
    }

    /// Run the MCP server with stdio transport.
    pub fn run_stdio(&self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let reader = BufReader::new(stdin.lock());
        let mut writer = stdout.lock();

        self.run_stream(reader, &mut writer)
    }

    fn run_stream<R: BufRead, W: Write>(&self, mut reader: R, mut writer: W) -> Result<()> {
        loop {
            let mut bytes = Vec::new();
            let read = match (&mut reader)
                .take((MAX_STDIO_LINE_SIZE + 2) as u64)
                .read_until(b'\n', &mut bytes)
            {
                Ok(read) => read,
                Err(error) => {
                    return Err(error.into());
                }
            };
            if read == 0 {
                break;
            }

            let has_newline = bytes.last() == Some(&b'\n');
            let content_len = bytes.len().saturating_sub(usize::from(has_newline));
            if content_len > MAX_STDIO_LINE_SIZE {
                if !has_newline {
                    discard_until_newline(&mut reader)?;
                }
                write_parse_error(&mut writer, "request line exceeds size limit")?;
                continue;
            }

            if has_newline {
                bytes.pop();
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
            }
            if bytes.is_empty() {
                continue;
            }

            let line = match std::str::from_utf8(&bytes) {
                Ok(line) => line,
                Err(error) => {
                    write_parse_error(&mut writer, &format!("invalid UTF-8: {error}"))?;
                    continue;
                }
            };

            match serde_json::from_str::<JsonRpcRequest>(line) {
                Ok(request) => {
                    // JSON-RPC notifications have no `id` field; no response expected.
                    if request.id.is_none() {
                        continue;
                    }
                    let response = self.handle_request(request);
                    serde_json::to_writer(&mut writer, &response)?;
                    writeln!(writer)?;
                    writer.flush()?;
                }
                Err(e) => {
                    write_parse_error(&mut writer, &e.to_string())?;
                }
            }
        }

        Ok(())
    }

    /// Makes `requested` absolute and makes sure that it is in the MCP root.
    /// `param` is the name of the argument that gave the path.
    fn resolve_confined_path(
        &self,
        param: &'static str,
        requested: &Path,
        relative_to: Option<&Path>,
    ) -> std::result::Result<PathBuf, ToolError> {
        let candidate = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            relative_to.unwrap_or(&self.root_path).join(requested)
        };
        let received = json!(requested.to_string_lossy());
        let canonical = candidate.canonicalize().map_err(|error| {
            // Only the search call that made a label can change it back into
            // a path. Thus tell the caller which fields of the result to use.
            let has_label = requested
                .to_str()
                .is_some_and(|text| text.starts_with('[') && text.contains("] "));
            let next = if has_label {
                "For a search result with a `[label] `, give `project` as `path` and `project_file` as the file."
            } else {
                "Give a path that exists."
            };
            ToolError::new(
                ErrorCode::NotFound,
                format!(
                    "The server cannot find the path '{}': {error}",
                    candidate.display()
                ),
            )
            .with_param(param)
            .with_received(&received)
            .with_next(next)
        })?;
        if self.allow_external_paths {
            return Ok(canonical);
        }
        let root = self.root_path.canonicalize().map_err(|error| {
            ToolError::internal(format!(
                "The server cannot find the MCP root '{}': {error}",
                self.root_path.display()
            ))
        })?;
        if canonical.starts_with(&root) {
            Ok(canonical)
        } else {
            Err(ToolError::new(
                ErrorCode::PathOutsideRoot,
                format!(
                    "The path '{}' is not in the MCP root '{}'.",
                    canonical.display(),
                    root.display()
                ),
            )
            .with_param(param)
            .with_expected(format!("a path in '{}'", root.display()))
            .with_received(&received)
            .with_next(
                "Give a path in the MCP root, or start the server with `--allow-external-paths`.",
            ))
        }
    }

    fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let (result, error_code) = match request.method.as_str() {
            "initialize" => (self.handle_initialize(), -32603),
            "tools/list" => (self.handle_tools_list(), -32603),
            "tools/call" => (self.handle_tool_call(request.params), -32602),
            "shutdown" => (Ok(json!({})), -32603),
            _ => (Err(format!("Unknown method: {}", request.method)), -32601),
        };

        match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(value),
                error: None,
            },
            Err(msg) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: error_code,
                    message: msg,
                    data: None,
                }),
            },
        }
    }

    fn handle_initialize(&self) -> std::result::Result<Value, String> {
        Ok(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "omen",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": INSTRUCTIONS
        }))
    }

    fn handle_tools_list(&self) -> std::result::Result<Value, String> {
        let tools: Vec<Value> = tools::all()
            .iter()
            .map(|spec| tools::schema::tool_definition(spec))
            .collect();
        Ok(json!({ "tools": tools }))
    }

    /// The names of all MCP tools, in the sequence of `tools/list`.
    ///
    /// The names come from the tool specifications in `src/mcp/tools/`. The
    /// manifest in `src/main.rs` reads this list.
    pub fn tool_names() -> &'static [&'static str] {
        tools::names()
    }

    fn handle_tool_call(&self, params: Option<Value>) -> std::result::Result<Value, String> {
        let params = params.ok_or("Missing params")?;
        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing tool name")?;
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let spec = tools::find(tool_name).ok_or_else(|| format!("Unknown tool: {tool_name}"))?;
        let arguments = match tools::validate::validate(spec, &arguments) {
            Ok(arguments) => arguments,
            Err(problems) => return Ok(error::error_result(&problems)),
        };
        Ok(match (spec.handler)(self, &arguments) {
            Ok(ToolOutput::Text(text)) => text_result(text),
            Ok(ToolOutput::Data(data)) => {
                let paginated = !matches!(spec.paginated, tools::Pagination::None);
                let page = envelope::Page {
                    limit: paginated
                        .then(|| arguments.count("limit"))
                        .flatten()
                        .unwrap_or(0),
                    offset: paginated
                        .then(|| arguments.count("offset"))
                        .flatten()
                        .unwrap_or(0),
                };
                let compact = spec.parameter("compact").is_some()
                    && arguments.boolean("compact").unwrap_or(false);
                let envelope = envelope::build(spec.name, spec.paginated, data, page, compact);
                text_result(envelope.to_string())
            }
            Err(error) => error.to_result(),
        })
    }

    /// The path that the `path` argument selects. The default is the MCP root.
    fn requested_path(&self, arguments: &Arguments<'_>) -> std::result::Result<PathBuf, ToolError> {
        let requested = arguments
            .string("path")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.root_path.clone());
        self.resolve_confined_path("path", &requested, Some(&self.root_path))
    }

    fn scope(&self, arguments: &Arguments<'_>) -> std::result::Result<Scope, ToolError> {
        let path = self.requested_path(arguments)?;
        // A file set that starts at a file holds one empty relative path,
        // and no analyzer can read it. Start at the directory of the file.
        if let (true, Some(directory), Some(name)) =
            (path.is_file(), path.parent(), path.file_name())
        {
            let file_set = FileSet::from_files(directory.to_path_buf(), vec![PathBuf::from(name)]);
            return Ok(Scope {
                path: directory.to_path_buf(),
                file_set,
            });
        }
        let file_set = FileSet::from_path(&path, &self.config)?;
        Ok(Scope { path, file_set })
    }

    fn analyze<A: Analyzer>(&self, arguments: &Arguments<'_>, analyzer: A) -> HandlerResult {
        self.analyze_with_config(arguments, &self.config, analyzer)
    }

    fn analyze_with_config<A: Analyzer>(
        &self,
        arguments: &Arguments<'_>,
        config: &Config,
        analyzer: A,
    ) -> HandlerResult {
        let scope = self.scope(arguments)?;
        let (git_root, git_skipped_reason) = match GitRepo::open(&scope.path) {
            Ok(repository) => (Some(repository.root().to_path_buf()), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let mut context = AnalysisContext::new(&scope.file_set, config, Some(&scope.path));
        if let Some(git_root) = &git_root {
            context = context.with_git_path(git_root);
        }
        let result = analyzer.analyze(&context)?;
        Ok(ToolOutput::Data(ToolData {
            git_skipped_reason,
            ..ToolData::new(serde_json::to_value(result)?)
        }))
    }

    fn load_index(
        &self,
        root: &Path,
        files: &[PathBuf],
    ) -> std::result::Result<crate::task_context::cache::Snapshot, ToolError> {
        let mut cache = self
            .index_cache
            .lock()
            .map_err(|_| ToolError::internal("The lock of the index cache is poisoned."))?;
        Ok(cache.load(root, files)?)
    }

    fn load_view(
        &self,
        root: &Path,
        files: &[PathBuf],
    ) -> std::result::Result<std::sync::Arc<crate::index::view::CallView>, ToolError> {
        let mut cache = self
            .view_cache
            .lock()
            .map_err(|_| ToolError::internal("The lock of the view cache is poisoned."))?;
        Ok(cache.load(root, files)?)
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct AlwaysErrorReader {
        attempts: usize,
    }

    impl Read for AlwaysErrorReader {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("persistent read failure"))
        }
    }

    impl BufRead for AlwaysErrorReader {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            self.attempts += 1;
            assert_eq!(self.attempts, 1, "run_stream retried a failed reader");
            Err(std::io::Error::other("persistent read failure"))
        }

        fn consume(&mut self, _amount: usize) {}
    }

    fn create_test_server() -> (McpServer, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();
        let server = McpServer::new(temp_dir.path().to_path_buf(), config);
        (server, temp_dir)
    }

    #[test]
    fn ambiguous_lookup_requires_definition_selection() {
        let (server, dir) = create_test_server();
        std::fs::write(
            dir.path().join("same.rb"),
            "class A\n  def helper\n    1\n  end\nend\nclass B\n  def helper\n    2\n  end\nend\n",
        )
        .unwrap();
        let call = |extra: Value| {
            let mut args = json!({"path": dir.path(), "name": "same.rb:helper"});
            args.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let response = server
                .handle_tool_call(Some(json!({"name": "get_symbol", "arguments": args})))
                .unwrap();
            serde_json::from_str::<Value>(response["content"][0]["text"].as_str().unwrap()).unwrap()
        };
        let ambiguous = call(json!({}));
        assert_eq!(ambiguous["error"]["code"], "ambiguous");
        assert_eq!(ambiguous["error"]["param"], "name");
        assert_eq!(
            ambiguous["error"]["suggestions"],
            json!([
                {"name": "same.rb#A.helper", "start_line": 2, "signature": "def helper"},
                {"name": "same.rb#B.helper", "start_line": 7, "signature": "def helper"}
            ])
        );
        assert!(ambiguous.get("result").is_none());
        let selected = call(json!({"start_line": 7}));
        assert_eq!(selected["result"]["start_line"], 7);
        assert!(selected["result"]["source"].as_str().unwrap().contains('2'));
        assert_eq!(
            call(json!({"start_line": 99}))["error"]["code"],
            "not_found"
        );
    }

    #[test]
    fn task_context_gives_ambiguous_and_not_found_errors() {
        let (server, dir) = create_test_server();
        std::fs::write(
            dir.path().join("same.rb"),
            "class A\n  def helper\n    1\n  end\nend\nclass B\n  def helper\n    2\n  end\nend\n",
        )
        .unwrap();
        let ambiguous = error_of(&server, "task_context", json!({"name": "same.rb:helper"}));
        assert_eq!(ambiguous["error"]["code"], "ambiguous");
        assert_eq!(
            ambiguous["error"]["suggestions"],
            json!([
                {"name": "same.rb:helper", "start_line": 2},
                {"name": "same.rb:helper", "start_line": 7}
            ])
        );
        let selected = call(
            &server,
            "task_context",
            json!({"name": "same.rb:helper", "start_line": 7}),
        );
        assert!(selected.get("isError").is_none(), "{selected}");

        let absent = error_of(&server, "task_context", json!({"name": "helpe"}));
        assert_eq!(absent["error"]["code"], "not_found");
        assert_eq!(absent["error"]["param"], "name");
        assert_eq!(absent["error"]["received"], "helpe");
        assert_eq!(absent["error"]["suggestions"], json!(["helper"]));
    }

    /// Calls a tool and gives the envelope of a result without an error.
    fn envelope_of(server: &McpServer, tool: &str, arguments: Value) -> Value {
        let result = call(server, tool, arguments);
        assert!(result.get("isError").is_none(), "{result}");
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    const ENVELOPE_KEYS: [&str; 8] = [
        "has_more",
        "next_offset",
        "offset",
        "result",
        "returned",
        "tool",
        "total_items",
        "truncated",
    ];

    fn write_call_chain(directory: &Path, callers: usize) {
        let mut source = String::from("pub fn target() -> u32 {\n    1\n}\n");
        for caller in 0..callers {
            source.push_str(&format!(
                "pub fn caller_with_a_long_name_{caller}() -> u32 {{\n    target()\n}}\n"
            ));
        }
        std::fs::write(directory.join("chain.rs"), source).unwrap();
    }

    #[test]
    fn task_context_gives_the_same_envelope_as_the_other_tools() {
        let (server, dir) = create_test_server();
        write_call_chain(dir.path(), 3);
        let task_context = envelope_of(&server, "task_context", json!({"name": "target"}));
        let complexity = envelope_of(&server, "complexity", json!({}));
        let keys = |envelope: &Value| -> Vec<String> {
            envelope.as_object().unwrap().keys().cloned().collect()
        };
        assert_eq!(keys(&task_context), ENVELOPE_KEYS);
        // The test directory is not a git repository.
        let mut complexity_keys = keys(&complexity);
        complexity_keys.retain(|key| key != "git_skipped_reason");
        assert_eq!(complexity_keys, ENVELOPE_KEYS);
        assert_eq!(task_context["tool"], "task_context");
        assert_eq!(task_context["offset"], 0);
        assert_eq!(task_context["total_items"], 4);
        assert_eq!(task_context["returned"], 4);
        assert_eq!(task_context["has_more"], false);
        assert!(task_context["next_offset"].is_null());
        assert_eq!(task_context["truncated"], json!([]));
    }

    #[test]
    fn task_context_reports_the_lists_that_its_byte_budget_cut() {
        let (server, dir) = create_test_server();
        write_call_chain(dir.path(), 60);
        let envelope = envelope_of(
            &server,
            "task_context",
            json!({"name": "target", "max_bytes": 2048}),
        );
        let symbols = envelope["result"]["symbols"].as_array().unwrap().len();
        let omitted = envelope["result"]["omitted"]["symbols"].as_u64().unwrap() as usize;
        assert!(omitted > 0, "the budget must cut the symbols: {envelope}");
        assert_eq!(envelope["returned"], symbols);
        // The count is after the cut of the tool. `truncated` has the total before it.
        assert_eq!(envelope["total_items"], symbols);
        assert_eq!(envelope["has_more"], false);
        let truncated = envelope["truncated"].as_array().unwrap();
        assert!(truncated.contains(
            &json!({"path": "/symbols", "total": symbols + omitted, "returned": symbols})
        ));
        let paths: Vec<&str> = truncated
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted);
    }

    #[test]
    fn task_context_reports_the_syntax_facts_that_its_byte_budget_removed() {
        let (server, dir) = create_test_server();
        write_call_chain(dir.path(), 60);
        let envelope = envelope_of(
            &server,
            "task_context",
            json!({"name": "target", "max_bytes": 2048}),
        );
        assert!(envelope["result"]["syntax_facts"].is_null(), "{envelope}");
        assert_eq!(envelope["result"]["omitted"]["syntax_facts"], 1);
        assert!(envelope["truncated"]
            .as_array()
            .unwrap()
            .contains(&json!({"path": "/syntax_facts", "total": 1, "returned": 0})));

        let complete = envelope_of(
            &server,
            "task_context",
            json!({"name": "target", "max_bytes": 1048576}),
        );
        assert!(complete["result"]["syntax_facts"].is_object());
        assert_eq!(complete["truncated"], json!([]));
    }

    #[test]
    fn get_symbol_reports_the_source_lines_that_it_cut() {
        let (server, dir) = create_test_server();
        std::fs::write(
            dir.path().join("long.rs"),
            "fn long_function() -> u32 {\n    let a = 1;\n    let b = 2;\n    a + b\n}\n",
        )
        .unwrap();
        let cut = envelope_of(
            &server,
            "get_symbol",
            json!({"name": "long_function", "max_source_lines": 2}),
        );
        assert_eq!(cut["result"]["source_truncated"], true);
        assert_eq!(
            cut["truncated"],
            json!([{"path": "/source", "total": 5, "returned": 2}])
        );
        let complete = envelope_of(&server, "get_symbol", json!({"name": "long_function"}));
        assert_eq!(complete["truncated"], json!([]));
    }

    #[test]
    fn context_reports_the_lists_that_max_symbols_and_max_risks_cut() {
        let (server, dir) = create_test_server();
        write_call_chain(dir.path(), 6);
        std::fs::write(
            dir.path().join("notes.rs"),
            "// TODO: first\n// TODO: second\n// TODO: third\nfn noted() {}\n",
        )
        .unwrap();
        let whole = envelope_of(
            &server,
            "context",
            json!({"max_symbols": 1000, "max_risks": 1000, "max_tokens": 1_000_000, "limit": 0}),
        );
        assert_eq!(whole["truncated"], json!([]));
        let symbols = whole["result"]["top_symbols"].as_array().unwrap().len();
        let risks = whole["result"]["risks"].as_array().unwrap().len();
        assert!(symbols > 2 && risks > 1, "{whole}");
        let cut = envelope_of(
            &server,
            "context",
            json!({"max_symbols": 2, "max_risks": 1, "max_tokens": 1_000_000, "limit": 0}),
        );
        assert_eq!(
            cut["truncated"],
            json!([
                {"path": "/risks", "total": risks, "returned": 1},
                {"path": "/top_symbols", "total": symbols, "returned": 2}
            ])
        );
    }

    #[test]
    fn context_reports_the_lists_that_its_token_budget_cut() {
        let (server, dir) = create_test_server();
        for directory in 0..12 {
            let path = dir.path().join(format!("module_{directory}"));
            std::fs::create_dir(&path).unwrap();
            write_call_chain(&path, 4);
        }
        let lists = ["entry_points", "hints", "risks", "top_symbols", "tree"];
        let lengths = |envelope: &Value| -> Vec<usize> {
            lists
                .iter()
                .map(|list| envelope["result"][*list].as_array().unwrap().len())
                .collect()
        };
        let arguments =
            |max_tokens: u64| json!({"max_symbols": 1000, "max_tokens": max_tokens, "limit": 0});
        let complete = envelope_of(&server, "context", arguments(1_000_000));
        assert_eq!(complete["truncated"], json!([]));
        let cut = envelope_of(&server, "context", arguments(300));
        let expected: Vec<Value> = lists
            .iter()
            .zip(lengths(&complete).into_iter().zip(lengths(&cut)))
            .filter(|(_, (total, returned))| returned < total)
            .map(|(list, (total, returned))| {
                json!({"path": format!("/{list}"), "total": total, "returned": returned})
            })
            .collect();
        assert!(!expected.is_empty(), "the budget must cut a list: {cut}");
        assert_eq!(cut["truncated"], json!(expected));
        // The counts describe the list after the budget. `truncated` has the total before it.
        assert_eq!(
            cut["total_items"],
            cut["result"]["top_symbols"].as_array().unwrap().len()
        );
    }

    /// Writes two functions that agree with the query `parse config`. The
    /// function with the higher complexity agrees better.
    fn write_search_fixture(directory: &Path) {
        std::fs::write(
            directory.join("parse.rs"),
            "pub fn first_reader(parse: u32, config: u32) -> u32 {\n    if parse > config { if parse > 2 { if config > 3 { if parse > 4 { return config; } return parse; } return config; } return parse; }\n    0\n}\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("simple.rs"),
            "pub fn second_reader(parse: u32, config: u32) -> u32 {\n    let width = 1;\n    let height = 2;\n    let depth = 3;\n    let colour = 4;\n    parse + config + width + height + depth + colour\n}\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("other.rs"),
            "pub fn unrelated_helper() -> u32 {\n    7\n}\n",
        )
        .unwrap();
    }

    fn search_tools() -> [(&'static str, &'static str); 2] {
        [
            ("semantic_search", "query"),
            ("semantic_search_hyde", "hypothetical_document"),
        ]
    }

    fn search_results(server: &McpServer, tool: &str, arguments: Value) -> Value {
        envelope_of(server, tool, arguments)
    }

    fn with_arguments(mut base: Value, extra: Value) -> Value {
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    }

    fn symbol_names(envelope: &Value) -> Vec<&str> {
        envelope["result"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|result| result["symbol_name"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn search_applies_max_complexity_before_top_k_on_each_path() {
        let (server, dir) = create_test_server();
        write_search_fixture(dir.path());
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        for (tool, text) in search_tools() {
            let base = json!({text: "parse config", "min_score": 0.0});
            let ranked = search_results(&server, tool, base.clone());
            let results = ranked["result"]["results"].as_array().unwrap();
            assert_eq!(results[0]["symbol_name"], "first_reader");
            assert_eq!(results[1]["symbol_name"], "second_reader");
            let complex = results[0]["cyclomatic_complexity"].as_u64().unwrap();
            let simple = results[1]["cyclomatic_complexity"].as_u64().unwrap();
            assert!(simple < complex, "the fixture needs two complexities");

            let filter = json!({"top_k": 1, "max_complexity": simple});
            for (path, extra) in [
                ("root", json!({})),
                ("files", json!({"files": ["parse.rs", "simple.rs"]})),
                ("projects", json!({"include_projects": ["sub"]})),
            ] {
                let arguments = with_arguments(with_arguments(base.clone(), filter.clone()), extra);
                let found = search_results(&server, tool, arguments);
                assert_eq!(
                    symbol_names(&found),
                    ["second_reader"],
                    "{tool} on the {path} path"
                );
                assert_eq!(found["total_items"], 1, "{tool} on the {path} path");
                assert_eq!(found["returned"], 1);
                assert_eq!(found["has_more"], false);
            }
        }
    }

    #[test]
    fn search_applies_min_score_when_files_are_given() {
        let (server, dir) = create_test_server();
        write_search_fixture(dir.path());
        for (tool, text) in search_tools() {
            let base =
                json!({text: "parse config", "files": ["parse.rs", "simple.rs", "other.rs"]});
            let all = search_results(
                &server,
                tool,
                with_arguments(base.clone(), json!({"min_score": 0.0})),
            );
            let scores: Vec<f64> = all["result"]["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|result| result["score"].as_f64().unwrap())
                .collect();
            assert!(scores.len() >= 2, "{tool}: {all}");
            let threshold = (scores[0] + scores[1]) / 2.0;
            let found = search_results(
                &server,
                tool,
                with_arguments(base.clone(), json!({"min_score": threshold})),
            );
            assert_eq!(symbol_names(&found), ["first_reader"], "{tool}");

            let default = search_results(&server, tool, base);
            for result in default["result"]["results"].as_array().unwrap() {
                assert!(result["score"].as_f64().unwrap() >= 0.3, "{tool}: {result}");
            }
        }
    }

    #[test]
    fn search_applies_files_together_with_include_projects() {
        let (server, dir) = create_test_server();
        write_search_fixture(dir.path());
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        for (tool, text) in search_tools() {
            let base = json!({text: "parse config", "min_score": 0.0, "files": ["simple.rs"]});
            let only_files = search_results(&server, tool, base.clone());
            assert_eq!(symbol_names(&only_files), ["second_reader"]);

            let empty = search_results(
                &server,
                tool,
                with_arguments(base.clone(), json!({"include_projects": []})),
            );
            assert_eq!(empty["result"], only_files["result"], "{tool}");

            let both = search_results(
                &server,
                tool,
                with_arguments(base, json!({"include_projects": ["sub"]})),
            );
            assert_eq!(symbol_names(&both), ["second_reader"], "{tool}");
        }
    }

    #[test]
    fn search_reports_the_results_that_top_k_cut() {
        let (server, dir) = create_test_server();
        write_search_fixture(dir.path());
        let all = search_results(
            &server,
            "semantic_search",
            json!({"query": "parse config", "min_score": 0.0, "top_k": 100}),
        );
        let total = all["result"]["results"].as_array().unwrap().len();
        assert!(total >= 2);
        assert_eq!(all["truncated"], json!([]));
        let cut = search_results(
            &server,
            "semantic_search",
            json!({"query": "parse config", "min_score": 0.0, "top_k": 1}),
        );
        assert_eq!(cut["returned"], 1);
        assert_eq!(cut["total_items"], 1);
        assert_eq!(cut["has_more"], false);
        assert_eq!(
            cut["truncated"],
            json!([{"path": "/results", "total": total, "returned": 1}])
        );
    }

    /// Makes the project `sub` in the root, with its own search index.
    fn write_indexed_project(root: &Path) {
        let project = root.join("sub");
        std::fs::create_dir(&project).unwrap();
        write_search_fixture(&project);
        let search = crate::semantic::SemanticSearch::new(
            &crate::semantic::SearchConfig::default(),
            &project,
        )
        .unwrap();
        search.index(&Config::default()).unwrap();
    }

    fn result_files(envelope: &Value) -> Vec<&str> {
        envelope["result"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|result| result["file"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn search_accepts_in_files_the_file_of_a_result_from_an_included_project() {
        let (server, dir) = create_test_server();
        write_search_fixture(dir.path());
        write_indexed_project(dir.path());
        for (tool, text) in search_tools() {
            let base = json!({
                text: "parse config", "min_score": 0.0, "top_k": 100,
                "include_projects": ["sub"]
            });
            let all = search_results(&server, tool, base.clone());
            let files = result_files(&all);
            assert!(files.contains(&"[sub] simple.rs"), "{tool}: {files:?}");
            assert!(files.len() > 2);

            // The labelled form selects the file of one project.
            let labelled = search_results(
                &server,
                tool,
                with_arguments(base.clone(), json!({"files": ["[sub] simple.rs"]})),
            );
            assert_eq!(result_files(&labelled), ["[sub] simple.rs"], "{tool}");

            // Each file that the tool gives is a valid value of `files`.
            for file in &files {
                let again = search_results(
                    &server,
                    tool,
                    with_arguments(base.clone(), json!({"files": [file]})),
                );
                let found = result_files(&again);
                assert!(!found.is_empty(), "{tool}: {file}");
                assert!(found.iter().all(|found| found == file), "{tool}: {file}");
            }

            // The plain form selects the path in each project.
            let plain = search_results(
                &server,
                tool,
                with_arguments(base.clone(), json!({"files": ["simple.rs"]})),
            );
            let mut found = result_files(&plain);
            found.sort_unstable();
            let root_file = files
                .iter()
                .find(|file| file.ends_with("] simple.rs") && **file != "[sub] simple.rs")
                .expect("the file of the root project");
            let mut expected = ["[sub] simple.rs", root_file];
            expected.sort_unstable();
            assert_eq!(found, expected, "{tool}");
        }
    }

    #[test]
    fn the_other_tools_accept_the_project_and_the_file_of_a_labelled_search_result() {
        let (server, dir) = create_test_server();
        write_search_fixture(dir.path());
        write_indexed_project(dir.path());
        let search = search_results(
            &server,
            "semantic_search",
            json!({
                "query": "parse config", "min_score": 0.0, "top_k": 100,
                "include_projects": ["sub"]
            }),
        );
        let results = search["result"]["results"].as_array().unwrap();
        let projects: std::collections::BTreeSet<&str> = results
            .iter()
            .map(|result| result["project"].as_str().expect("project"))
            .collect();
        assert_eq!(projects.into_iter().collect::<Vec<_>>(), [".", "sub"]);

        for result in results {
            let project = result["project"].as_str().unwrap();
            let file = result["project_file"].as_str().expect("project_file");
            let labelled = result["file"].as_str().unwrap();
            assert!(labelled.ends_with(&format!("] {file}")), "{labelled}");

            let outline = envelope_of(&server, "outline", json!({"path": project, "file": file}));
            assert_eq!(outline["result"]["files"][0]["file"], file);

            let name = format!("{file}:{}", result["symbol_name"].as_str().unwrap());
            let symbol = envelope_of(
                &server,
                "get_symbol",
                json!({"path": project, "name": name, "start_line": result["start_line"]}),
            );
            assert_eq!(symbol["result"]["name"], result["symbol_name"]);
            assert_eq!(symbol["result"]["file"], file);

            let complexity = envelope_of(
                &server,
                "complexity",
                json!({"path": format!("{project}/{file}")}),
            );
            assert!(
                complexity["total_items"].as_u64().unwrap() > 0,
                "{labelled}"
            );
        }

        // A result of a search in one project has no label, thus it has no
        // project fields.
        let single = search_results(
            &server,
            "semantic_search",
            json!({"query": "parse config", "min_score": 0.0}),
        );
        let first = &single["result"]["results"][0];
        assert!(first.get("project").is_none());
        assert!(first.get("project_file").is_none());
    }

    #[test]
    fn search_rejects_a_file_label_that_is_not_an_included_project() {
        let (server, dir) = create_test_server();
        write_search_fixture(dir.path());
        write_indexed_project(dir.path());
        let root_label = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for (tool, text) in search_tools() {
            let body = error_of(
                &server,
                tool,
                json!({
                    text: "parse config", "include_projects": ["sub"],
                    "files": ["simple.rs", "[sob] simple.rs"]
                }),
            );
            assert_eq!(body["error"]["code"], "invalid_argument", "{tool}");
            assert_eq!(body["error"]["param"], "files");
            assert_eq!(body["error"]["received"], "[sob] simple.rs");
            assert_eq!(body["error"]["suggestions"], json!([root_label, "sub"]));
        }
    }

    #[test]
    fn search_without_included_projects_reads_a_bracket_as_a_part_of_the_path() {
        let (server, dir) = create_test_server();
        std::fs::create_dir(dir.path().join("[id] page")).unwrap();
        write_search_fixture(&dir.path().join("[id] page"));
        let found = search_results(
            &server,
            "semantic_search",
            json!({"query": "parse config", "min_score": 0.0, "files": ["[id] page/simple.rs"]}),
        );
        assert_eq!(result_files(&found), ["[id] page/simple.rs"]);
    }

    #[test]
    fn a_tool_accepts_the_identifiers_that_the_other_tools_give() {
        let (server, dir) = create_test_server();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();
        write_call_chain(&source, 2);
        write_search_fixture(&source);
        let ok = |tool: &str, arguments: Value| envelope_of(&server, tool, arguments);

        // `outline` file -> `outline`, `complexity`, `context`, `get_symbol`.
        let outline = ok("outline", json!({}));
        let file = outline["result"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["file"].as_str().unwrap().ends_with("chain.rs"))
            .expect("chain.rs in the outline");
        let file_path = file["file"].as_str().unwrap();
        assert_eq!(file_path, "src/chain.rs");
        let by_path = ok("outline", json!({"path": file_path}));
        assert_eq!(by_path["result"]["files"][0]["file"], "chain.rs");
        let one = ok("outline", json!({"file": file_path}));
        assert_eq!(one["result"]["files"][0]["file"], file_path);
        let complexity = ok("complexity", json!({"path": file_path}));
        assert_eq!(complexity["total_items"], 3);
        ok("context", json!({"path": file_path}));
        let function = file["functions"][0]["name"].as_str().unwrap();
        let qualified = format!("{file_path}:{function}");
        let symbol = ok("get_symbol", json!({"name": qualified}));
        assert_eq!(symbol["result"]["file"], file_path);

        // `get_symbol` identifier -> `get_symbol`, `impact`, `task_context`.
        let identifier = symbol["result"]["qualified_name"].as_str().unwrap();
        // The identifier is the key of the symbol in the shared index.
        assert_eq!(identifier, format!("{file_path}#{function}"));
        for (tool, param) in [
            ("get_symbol", "name"),
            ("impact", "symbol"),
            ("task_context", "name"),
        ] {
            ok(tool, json!({param: identifier}));
        }

        // Caller of `get_symbol`, symbol of `impact`, `repomap`, and
        // `task_context` -> `get_symbol`.
        let target = ok("get_symbol", json!({"name": "target"}));
        let caller = target["result"]["callers"][0]["qualified_name"]
            .as_str()
            .unwrap();
        let impact = ok("impact", json!({"symbol": "target"}));
        let affected = impact["result"]["callers"][0]["symbols"][0]["qualified_name"]
            .as_str()
            .unwrap();
        let ranked = ok("repomap", json!({}));
        let ranked = ranked["result"]["symbols"][0]["qualified_name"]
            .as_str()
            .unwrap();
        let context = ok("task_context", json!({"name": "target"}));
        let related = context["result"]["symbols"][1]["location"]["qualified_name"]
            .as_str()
            .unwrap();
        // An identifier has the form `file:name` or the form of a key,
        // `file#Container.name`. The two forms name one definition.
        let identity = |identifier: &str| -> String {
            let (file, path) = identifier
                .split_once('#')
                .or_else(|| identifier.rsplit_once(':'))
                .unwrap();
            format!("{file}:{}", path.rsplit('.').next().unwrap())
        };
        for identifier in [caller, affected, ranked, related] {
            let found = ok("get_symbol", json!({"name": identifier}));
            let found = found["result"]["qualified_name"].as_str().unwrap();
            assert_eq!(identity(found), identity(identifier));
        }
        let affected_file = impact["result"]["files_affected"][0].as_str().unwrap();
        ok("outline", json!({"file": affected_file}));

        // Search result -> `files` of the search, `outline`, `get_symbol`.
        let search = ok(
            "semantic_search",
            json!({"query": "parse config", "min_score": 0.0}),
        );
        let result = &search["result"]["results"][0];
        let result_file = result["file"].as_str().unwrap();
        let again = ok(
            "semantic_search",
            json!({"query": "parse config", "min_score": 0.0, "files": [result_file]}),
        );
        assert!(result_files(&again).iter().all(|file| *file == result_file));
        assert!(!result_files(&again).is_empty());
        ok("outline", json!({"file": result_file}));
        let name = format!("{result_file}:{}", result["symbol_name"].as_str().unwrap());
        let found = ok(
            "get_symbol",
            json!({"name": name, "start_line": result["start_line"]}),
        );
        let found = found["result"]["qualified_name"].as_str().unwrap();
        assert_eq!(identity(found), name);
    }

    #[test]
    fn a_cut_page_gives_the_offset_of_the_next_page() {
        let (server, dir) = create_test_server();
        write_call_chain(dir.path(), 5);
        let first = envelope_of(&server, "complexity", json!({"limit": 4}));
        assert_eq!(first["total_items"], 6);
        assert_eq!(first["returned"], 4);
        assert_eq!(first["has_more"], true);
        assert_eq!(first["next_offset"], 4);
        assert_eq!(
            first["truncated"],
            json!([{"path": "/files/*/functions", "total": 6, "returned": 4}])
        );
        let second = envelope_of(&server, "complexity", json!({"limit": 4, "offset": 4}));
        assert_eq!(second["returned"], 2);
        assert_eq!(second["has_more"], false);
        assert!(second["next_offset"].is_null());
    }

    #[test]
    fn score_with_limit_1_gives_one_component_and_each_envelope_field() {
        let (server, dir) = create_test_server();
        write_call_chain(dir.path(), 3);
        let full = envelope_of(&server, "score", json!({"limit": 0}));
        let names: Vec<String> = full["result"]["components"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert!(names.len() > 1, "the result must have many components");
        assert_eq!(full["total_items"], names.len());
        assert_eq!(full["returned"], names.len());
        assert_eq!(full["has_more"], false);
        assert_eq!(full["truncated"], json!([]));

        let first = envelope_of(&server, "score", json!({"limit": 1}));
        assert_eq!(first["tool"], "score");
        assert_eq!(first["total_items"], names.len());
        assert_eq!(first["returned"], 1);
        assert_eq!(first["offset"], 0);
        assert_eq!(first["has_more"], true);
        assert_eq!(first["next_offset"], 1);
        assert_eq!(
            first["truncated"],
            json!([{"path": "/components", "total": names.len(), "returned": 1}])
        );
        let components = first["result"]["components"].as_object().unwrap();
        assert_eq!(components.keys().collect::<Vec<_>>(), [&names[0]]);
        assert_eq!(
            first["result"]["skipped_components"],
            full["result"]["skipped_components"]
        );
        assert_eq!(
            first["result"]["overall_score"],
            full["result"]["overall_score"]
        );
    }

    /// Calls a tool one page at a time until `has_more` is false, and makes
    /// sure that the pages together hold the items of the call without a limit.
    fn assert_pages_give_all_items(
        server: &McpServer,
        tool: &str,
        arguments: Value,
        limit: usize,
        items: fn(&Value) -> Vec<Value>,
    ) {
        let with = |extra: Value| {
            let mut all = arguments.clone();
            all.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            envelope_of(server, tool, all)
        };
        let expected = items(&with(json!({"limit": 0}))["result"]);
        assert!(expected.len() > limit, "{tool}: the fixture is too small");
        let mut collected = Vec::new();
        let mut offset = 0;
        loop {
            let page = with(json!({"limit": limit, "offset": offset}));
            let page_items = items(&page["result"]);
            assert_eq!(page["returned"], page_items.len(), "{tool}");
            assert_eq!(page["total_items"], expected.len(), "{tool}");
            collected.extend(page_items);
            if page["has_more"] == false {
                assert!(page["next_offset"].is_null());
                break;
            }
            let next = page["next_offset"].as_u64().unwrap() as usize;
            assert!(next > offset, "{tool}: the offset must increase");
            offset = next;
        }
        assert_eq!(collected, expected, "{tool}");
    }

    fn nested_items(result: &Value, parents: &[&str], child: &str) -> Vec<Value> {
        parents
            .iter()
            .filter_map(|parent| result[*parent].as_array())
            .flatten()
            .filter_map(|element| element[child].as_array())
            .flatten()
            .cloned()
            .collect()
    }

    #[test]
    fn the_pages_of_the_tools_with_a_nested_primary_list_give_all_items() {
        let (server, dir) = create_test_server();
        write_call_chain(dir.path(), 7);
        std::fs::write(
            dir.path().join("second.rs"),
            "fn middle() { bottom(); }\nfn top() { middle(); }\nfn bottom() {}\n",
        )
        .unwrap();
        assert_pages_give_all_items(&server, "complexity", json!({}), 3, |result| {
            nested_items(result, &["files"], "functions")
        });
        // `middle` has one caller and one callee, thus the pages go from the
        // `callers` list to the `callees` list.
        assert_pages_give_all_items(
            &server,
            "impact",
            json!({"symbol": "middle"}),
            1,
            |result| nested_items(result, &["callers", "callees"], "symbols"),
        );
        assert_pages_give_all_items(
            &server,
            "impact",
            json!({"symbol": "target"}),
            2,
            |result| nested_items(result, &["callers", "callees"], "symbols"),
        );
        assert_pages_give_all_items(&server, "score", json!({}), 2, |result| {
            result["components"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(name, component)| json!([name, component]))
                .collect()
        });
    }

    fn write_symbols(directory: &Path) {
        std::fs::write(
            directory.join("symbols.rs"),
            "pub fn alpha_symbol() { beta_symbol(); }\npub fn beta_symbol() { gamma_symbol(); }\npub fn gamma_symbol() {}\n",
        )
        .unwrap();
    }

    fn markdown_symbols(server: &McpServer, extra: Value) -> Vec<&'static str> {
        let mut arguments = json!({"format": "markdown"});
        arguments
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let result = call(server, "context", arguments);
        assert!(result.get("isError").is_none(), "{result}");
        let text = result["content"][0]["text"].as_str().unwrap().to_string();
        ["alpha_symbol", "beta_symbol", "gamma_symbol"]
            .into_iter()
            .filter(|name| text.contains(name))
            .collect()
    }

    #[test]
    fn context_markdown_applies_limit_and_offset_to_the_symbols() {
        let (server, dir) = create_test_server();
        write_symbols(dir.path());
        let json_order: Vec<String> = envelope_of(&server, "context", json!({}))["result"]
            ["top_symbols"]
            .as_array()
            .unwrap()
            .iter()
            .map(|symbol| symbol["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(json_order.len(), 3);
        assert_eq!(markdown_symbols(&server, json!({})).len(), 3);
        assert_eq!(markdown_symbols(&server, json!({"limit": 0})).len(), 3);
        assert_eq!(
            markdown_symbols(&server, json!({"limit": 1})),
            [json_order[0].as_str()]
        );
        assert_eq!(
            markdown_symbols(&server, json!({"limit": 1, "offset": 1})),
            [json_order[1].as_str()]
        );
        assert!(markdown_symbols(&server, json!({"offset": 3})).is_empty());
    }

    #[test]
    fn satd_finds_the_custom_markers_of_the_configuration() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("marked.rs"),
            "// SHORTCUT: remove this\nfn marked() {}\n",
        )
        .unwrap();
        let mut config = Config::default();
        config.satd.custom_markers = vec!["SHORTCUT".to_string()];
        let with_marker = McpServer::new(dir.path().to_path_buf(), config);
        let without_marker = McpServer::new(dir.path().to_path_buf(), Config::default());
        let found = envelope_of(&with_marker, "satd", json!({}));
        assert_eq!(found["total_items"], 1, "{found}");
        assert_eq!(
            envelope_of(&without_marker, "satd", json!({}))["total_items"],
            0
        );
    }

    #[test]
    fn churn_with_an_invalid_configured_window_is_an_error() {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.churn.since = "nonsense".to_string();
        let server = McpServer::new(dir.path().to_path_buf(), config);
        let body = error_of(&server, "churn", json!({}));
        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains("churn.since"), "{message}");
        assert!(message.contains("\"nonsense\""), "{message}");
        // The argument has priority, thus the configured value is not read.
        let with_days = call(&server, "churn", json!({"days": 30}));
        let text = with_days["content"][0]["text"].as_str().unwrap();
        assert!(!text.contains("churn.since"), "{text}");
    }

    #[test]
    fn initialize_gives_instructions_for_an_agent() {
        let (server, _dir) = create_test_server();
        let result = server.handle_initialize().unwrap();
        let instructions = result["instructions"].as_str().expect("instructions");
        assert!(
            instructions.len() < 1200,
            "{} characters",
            instructions.len()
        );
        for necessary in [
            "`context`",
            "`outline`",
            "`get_symbol`",
            "`impact`",
            "`limit`",
            "`next_offset`",
            "`has_more`",
            "`truncated`",
            "`error.code`",
        ] {
            assert!(instructions.contains(necessary), "{necessary}");
        }
        for name in instructions
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|name| McpServer::tool_names().contains(name))
        {
            assert!(tools::find(name).is_some());
        }
        assert!(!instructions.contains('\''), "no contractions");
    }

    #[test]
    fn a_symbol_name_with_a_spelling_error_gets_the_nearest_names() {
        let (server, dir) = create_test_server();
        std::fs::write(
            dir.path().join("known.rs"),
            "fn known_symbol() {}\nfn other() {}\n",
        )
        .unwrap();
        for tool in ["get_symbol", "task_context"] {
            let body = error_of(&server, tool, json!({"name": "known.rs:known_symbl"}));
            assert_eq!(body["error"]["code"], "not_found", "{tool}");
            assert_eq!(
                body["error"]["suggestions"],
                json!(["known_symbol"]),
                "{tool}"
            );
            assert_eq!(body["error"]["next"], "Use a name from `suggestions`.");
        }
    }

    #[test]
    fn impact_of_an_unknown_symbol_is_a_not_found_error() {
        let (server, dir) = create_test_server();
        std::fs::write(dir.path().join("known.rs"), "fn known_symbol() {}\n").unwrap();
        let body = error_of(&server, "impact", json!({"symbol": "known"}));
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["param"], "symbol");
        assert_eq!(body["error"]["suggestions"], json!(["known_symbol"]));
    }

    #[test]
    fn impact_tells_that_the_depth_cut_the_callers() {
        let (server, dir) = create_test_server();
        std::fs::write(
            dir.path().join("chain.py"),
            "def top():\n    return middle()\n\n\ndef middle():\n    return leaf()\n\n\ndef leaf():\n    return 1\n",
        )
        .unwrap();
        let cut = envelope_of(
            &server,
            "impact",
            json!({"symbol": "leaf", "depth": 1, "direction": "callers"}),
        );
        assert_eq!(cut["result"]["total_callers"], 1);
        assert_eq!(
            cut["truncated"],
            json!([{"path": "/callers", "total": 2, "returned": 1}])
        );
        let whole = envelope_of(
            &server,
            "impact",
            json!({"symbol": "leaf", "depth": 4, "direction": "callers"}),
        );
        assert_eq!(whole["result"]["total_callers"], 2);
        assert_eq!(whole["truncated"], json!([]));
    }

    #[test]
    fn a_path_that_does_not_exist_is_a_not_found_error() {
        let (server, _dir) = create_test_server();
        let body = error_of(&server, "complexity", json!({"path": "no/such/directory"}));
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["param"], "path");
        assert_eq!(body["error"]["received"], "no/such/directory");
        assert_eq!(body["error"]["next"], "Give a path that exists.");
    }

    #[test]
    fn a_labelled_search_file_as_a_path_gives_the_fields_to_use() {
        let (server, _dir) = create_test_server();
        for (tool, param) in [("outline", "file"), ("complexity", "path")] {
            let body = error_of(&server, tool, json!({param: "[sub] simple.rs"}));
            assert_eq!(body["error"]["code"], "not_found", "{tool}");
            assert_eq!(body["error"]["param"], param);
            assert_eq!(
                body["error"]["next"],
                "For a search result with a `[label] `, give `project` as `path` and `project_file` as the file."
            );
        }
    }

    #[test]
    fn a_path_outside_the_root_is_a_path_outside_root_error() {
        let (server, root) = create_test_server();
        let outside = TempDir::new().unwrap();
        std::fs::write(outside.path().join("secret.rs"), "fn secret() {}\n").unwrap();

        let body = error_of(&server, "complexity", json!({"path": outside.path()}));
        assert_eq!(body["error"]["code"], "path_outside_root");
        assert_eq!(body["error"]["param"], "path");

        let file = outside.path().join("secret.rs");
        let body = error_of(&server, "outline", json!({"file": file}));
        assert_eq!(body["error"]["code"], "path_outside_root");
        assert_eq!(body["error"]["param"], "file");

        let body = error_of(
            &server,
            "semantic_search",
            json!({"query": "secret", "include_projects": [outside.path()]}),
        );
        assert_eq!(body["error"]["code"], "path_outside_root");
        assert_eq!(body["error"]["param"], "include_projects");
        assert!(root.path().exists());
    }

    #[test]
    fn a_git_tool_outside_a_repository_is_a_git_unavailable_error() {
        let (server, dir) = create_test_server();
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        for tool in ["hotspot", "ownership", "defect"] {
            let body = error_of(&server, tool, json!({}));
            assert_eq!(body["error"]["code"], "git_unavailable", "{tool}: {body}");
            assert!(body["error"]["next"].is_string());
        }
    }

    #[test]
    fn symbol_line_selector_rejects_invalid_values() {
        let (server, dir) = create_test_server();
        std::fs::write(dir.path().join("main.rs"), "fn helper() {}\n").unwrap();
        for line in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("7"),
            json!(4294967296_u64),
        ] {
            let result = server.handle_tool_call(Some(json!({
                "name": "get_symbol", "arguments": {
                    "path": dir.path(), "name": "helper", "start_line": line
                }
            })));
            let response = result.unwrap();
            assert_eq!(response["isError"], true);
            assert!(response["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("start_line"));
        }
    }

    #[test]
    fn test_mcp_server_new() {
        let (server, _temp_dir) = create_test_server();
        assert!(server.root_path.exists());
    }

    #[test]
    fn test_requested_path_outside_root_is_rejected_by_default() {
        let (server, _root) = create_test_server();
        let outside = TempDir::new().unwrap();

        let result = server.resolve_confined_path("path", outside.path(), None);

        assert!(result.is_err());
    }

    /// Calls a tool and gives the MCP tool result.
    fn call(server: &McpServer, tool: &str, arguments: Value) -> Value {
        server
            .handle_tool_call(Some(json!({"name": tool, "arguments": arguments})))
            .expect("a tool result, not a protocol error")
    }

    #[test]
    fn test_requested_path_outside_root_is_allowed_with_flag() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let server =
            McpServer::new_with_external_paths(root.path().to_path_buf(), Config::default(), true);

        assert_eq!(
            server
                .resolve_confined_path("path", outside.path(), None)
                .unwrap(),
            outside.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn test_outline_rejects_parent_traversal() {
        let (server, root) = create_test_server();
        let outside = root.path().parent().unwrap().join("outside.rs");
        std::fs::write(&outside, "fn secret() {}\n").unwrap();

        let result = call(&server, "outline", json!({"file": "../outside.rs"}));

        assert_eq!(result["isError"], true);
        std::fs::remove_file(outside).unwrap();
    }

    #[test]
    fn test_semantic_search_rejects_external_include_project() {
        let (server, _root) = create_test_server();
        let outside = TempDir::new().unwrap();

        let result = call(
            &server,
            "semantic_search",
            json!({"query": "test", "include_projects": [outside.path()]}),
        );

        assert_eq!(result["isError"], true);
    }

    #[test]
    fn test_semantic_search_hyde_rejects_external_include_project() {
        let (server, _root) = create_test_server();
        let outside = TempDir::new().unwrap();

        let result = call(
            &server,
            "semantic_search_hyde",
            json!({"hypothetical_document": "test", "include_projects": [outside.path()]}),
        );

        assert_eq!(result["isError"], true);
    }

    #[test]
    fn test_stdio_invalid_utf8_does_not_stop_following_request() {
        let (server, _root) = create_test_server();
        let mut input = vec![0xff, b'\n'];
        input.extend_from_slice(br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
        input.push(b'\n');
        let mut output = Vec::new();

        server.run_stream(input.as_slice(), &mut output).unwrap();

        let responses: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["error"]["code"], -32700);
        assert_eq!(responses[1]["id"], 1);
    }

    #[test]
    fn test_stdio_processes_final_request_without_trailing_newline() {
        let (server, _root) = create_test_server();
        let input = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let mut output = Vec::new();

        server.run_stream(input.as_slice(), &mut output).unwrap();

        let response: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(response["id"], 1);
        assert!(response.get("result").is_some());
        assert!(response.get("error").is_none());
    }

    #[test]
    fn test_stdio_stops_after_persistent_read_error() {
        let (server, _root) = create_test_server();
        let reader = AlwaysErrorReader { attempts: 0 };
        let mut output = Vec::new();

        let error = server.run_stream(reader, &mut output).unwrap_err();

        assert!(error.to_string().contains("persistent read failure"));
        assert!(output.is_empty());
    }

    #[test]
    fn test_stdio_rejects_oversized_line_and_continues() {
        let (server, _root) = create_test_server();
        let mut input = vec![b'x'; MAX_STDIO_LINE_SIZE + 1];
        input.push(b'\n');
        input.extend_from_slice(br#"{"jsonrpc":"2.0","id":2,"method":"initialize"}"#);
        input.push(b'\n');
        let mut output = Vec::new();

        server.run_stream(input.as_slice(), &mut output).unwrap();

        let responses: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["error"]["code"], -32700);
        assert_eq!(responses[1]["id"], 2);
    }

    /// B9: tool_names() must exactly match the names produced by handle_tools_list,
    /// keeping the manifest and the server implementation in sync.
    #[test]
    fn test_tool_names_matches_handle_tools_list() {
        let (server, _temp_dir) = create_test_server();
        let tools_response = server.handle_tools_list().unwrap();
        let listed: Vec<String> = tools_response["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .filter_map(|t| t["name"].as_str().map(|s| s.to_string()))
            .collect();

        let canonical: Vec<&str> = McpServer::tool_names().to_vec();

        // Both slices must contain exactly the same names (order-insensitive).
        let mut listed_sorted = listed.clone();
        listed_sorted.sort();
        let mut canonical_sorted: Vec<String> = canonical.iter().map(|s| s.to_string()).collect();
        canonical_sorted.sort();

        assert_eq!(
            listed_sorted,
            canonical_sorted,
            "tool_names() must match handle_tools_list() tool names.\n\
             In tool_names but not in handle_tools_list: {:?}\n\
             In handle_tools_list but not in tool_names: {:?}",
            canonical_sorted
                .iter()
                .filter(|n| !listed_sorted.contains(n))
                .collect::<Vec<_>>(),
            listed_sorted
                .iter()
                .filter(|n| !canonical_sorted.contains(n))
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn test_handle_initialize() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_initialize().unwrap();
        assert_eq!(result["protocolVersion"], "2025-11-25");
        assert!(result.get("capabilities").is_some());
        assert!(result.get("serverInfo").is_some());
    }

    #[test]
    fn test_handle_initialize_has_tools_capability() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_initialize().unwrap();
        let capabilities = result.get("capabilities").unwrap();
        assert!(capabilities.get("tools").is_some());
    }

    #[test]
    fn test_handle_initialize_server_info() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_initialize().unwrap();
        let server_info = result.get("serverInfo").unwrap();
        assert_eq!(server_info.get("name").unwrap(), "omen");
    }

    #[test]
    fn test_handle_tools_list() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        assert!(!tools.is_empty());
    }

    #[test]
    fn test_handle_tools_list_has_complexity() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_complexity = tools.iter().any(|t| t.get("name").unwrap() == "complexity");
        assert!(has_complexity);
    }

    #[test]
    fn test_handle_tools_list_has_satd() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_satd = tools.iter().any(|t| t.get("name").unwrap() == "satd");
        assert!(has_satd);
    }

    #[test]
    fn test_handle_tools_list_has_stubs() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_stubs = tools.iter().any(|t| t.get("name").unwrap() == "stubs");
        assert!(has_stubs);
    }

    #[test]
    fn test_handle_tools_list_has_deadcode() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_deadcode = tools.iter().any(|t| t.get("name").unwrap() == "deadcode");
        assert!(has_deadcode);
    }

    #[test]
    fn test_handle_tools_list_has_churn() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_churn = tools.iter().any(|t| t.get("name").unwrap() == "churn");
        assert!(has_churn);
    }

    #[test]
    fn test_handle_tools_list_has_context() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_context = tools.iter().any(|t| t.get("name").unwrap() == "context");
        assert!(has_context);
    }

    #[test]
    fn test_handle_tool_call_missing_params() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tool_call(None);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_tool_call_missing_name() {
        let (server, _temp_dir) = create_test_server();
        let params = json!({"arguments": {}});
        let result = server.handle_tool_call(Some(params));
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_tool_call_unknown_tool() {
        let (server, _temp_dir) = create_test_server();
        let params = json!({
            "name": "unknown_tool",
            "arguments": {}
        });
        let result = server.handle_tool_call(Some(params));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown tool"));
    }

    #[test]
    fn test_handle_tool_call_complexity() {
        let (server, temp_dir) = create_test_server();
        // Create a test file
        std::fs::write(temp_dir.path().join("test.rs"), "fn main() {}").unwrap();

        let params = json!({
            "name": "complexity",
            "arguments": {"path": temp_dir.path().to_str().unwrap()}
        });
        let result = server.handle_tool_call(Some(params));
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.get("content").is_some());
    }

    #[test]
    fn test_handle_tool_call_uses_requested_path_as_analysis_root() {
        let (server, server_root) = create_test_server();
        let target_dir = server_root.path().join("nested");
        std::fs::create_dir(&target_dir).unwrap();
        std::fs::write(target_dir.join("target.rs"), "fn target_function() {}\n").unwrap();

        let params = json!({
            "name": "complexity",
            "arguments": {"path": target_dir.to_str().unwrap()}
        });
        let response = server.handle_tool_call(Some(params)).unwrap();
        let text = response["content"][0]["text"]
            .as_str()
            .expect("tool response text should be a string");

        assert!(
            text.contains("target_function"),
            "expected MCP analysis to read files from requested path, got {text}"
        );
    }

    #[test]
    fn test_handle_tool_call_context() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(
            temp_dir.path().join("test.rs"),
            "pub fn mcp_entrypoint() {}\n// TODO: remove shortcut\n",
        )
        .unwrap();

        let params = json!({
            "name": "context",
            "arguments": {"path": temp_dir.path().to_str().unwrap()}
        });
        let response = server.handle_tool_call(Some(params)).unwrap();
        let text = response["content"][0]["text"].as_str().unwrap();

        assert!(text.contains("hints"));
        assert!(
            text.contains("mcp_entrypoint"),
            "expected context to contain requested symbol, got {text}"
        );
    }

    #[test]
    fn test_handle_tool_call_context_markdown_returns_text_directly() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(
            temp_dir.path().join("main.rs"),
            "pub fn markdown_symbol() {}\n",
        )
        .unwrap();

        let params = json!({
            "name": "context",
            "arguments": {
                "path": temp_dir.path().to_str().unwrap(),
                "format": "markdown"
            }
        });
        let response = server.handle_tool_call(Some(params)).unwrap();
        let text = response["content"][0]["text"].as_str().unwrap();

        // Markdown mode returns text starting with "# Repository:" not JSON
        assert!(
            text.starts_with("# Repository:"),
            "markdown mode should start with '# Repository:' header, got: {text}"
        );
        assert!(text.contains("markdown_symbol"), "should include symbol");
    }

    #[test]
    fn test_handle_tool_call_context_max_tokens_trims_output() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(
            temp_dir.path().join("test.rs"),
            "pub fn token_fn() {}\n// TODO: trim me\n",
        )
        .unwrap();

        let params = json!({
            "name": "context",
            "arguments": {
                "path": temp_dir.path().to_str().unwrap(),
                "max_tokens": 500
            }
        });
        let response = server.handle_tool_call(Some(params)).unwrap();
        let text = response["content"][0]["text"].as_str().unwrap();
        let byte_budget = 500 * 4;
        // The JSON envelope wraps the result so the text can be bigger than the
        // inner context, but the token budget should have been applied.
        assert!(
            text.len() < byte_budget + 1000,
            "with max_tokens=500 output should be small, got {} bytes",
            text.len()
        );
    }

    #[test]
    fn test_handle_tool_call_satd() {
        let (server, temp_dir) = create_test_server();
        // Create a test file with SATD
        std::fs::write(
            temp_dir.path().join("test.rs"),
            "// TODO: fix this\nfn main() {}",
        )
        .unwrap();

        let params = json!({
            "name": "satd",
            "arguments": {"path": temp_dir.path().to_str().unwrap()}
        });
        let result = server.handle_tool_call(Some(params));
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_tool_call_stubs() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(
            temp_dir.path().join("test.rs"),
            "fn f() {\n    todo!()\n}\n",
        )
        .unwrap();

        let params = json!({
            "name": "stubs",
            "arguments": {"path": temp_dir.path().to_str().unwrap()}
        });
        let response = server.handle_tool_call(Some(params)).unwrap();
        let text = response["content"][0]["text"]
            .as_str()
            .expect("tool response text should be a string");
        assert!(
            text.contains("not_implemented"),
            "stubs should find todo!(), got: {text}"
        );
    }

    #[test]
    fn test_handle_tool_call_stubs_uses_requested_path() {
        // stubs tool should also analyze the requested path, not the server root
        let (server, server_root) = create_test_server();
        std::fs::write(
            server_root.path().join("server_root.rs"),
            "fn f() {\n    todo!()\n}\n",
        )
        .unwrap();

        let target_dir = server_root.path().join("target");
        std::fs::create_dir(&target_dir).unwrap();
        std::fs::write(
            target_dir.join("target.py"),
            "def f():\n    raise NotImplementedError\n",
        )
        .unwrap();

        let params = json!({
            "name": "stubs",
            "arguments": {"path": target_dir.to_str().unwrap()}
        });
        let response = server.handle_tool_call(Some(params)).unwrap();
        let text = response["content"][0]["text"]
            .as_str()
            .expect("tool response text should be a string");

        assert!(
            text.contains("target.py"),
            "stubs should find the stub in the requested path, got: {text}"
        );
        assert!(
            !text.contains("server_root.rs"),
            "stubs should not analyze the server root, got: {text}"
        );
    }

    #[test]
    fn test_handle_tool_call_rejects_path_outside_server_root() {
        let (server, _server_root) = create_test_server();
        let target_dir = TempDir::new().unwrap();
        let params = json!({
            "name": "complexity",
            "arguments": {"path": target_dir.path().to_str().unwrap()}
        });

        // A path outside the server root is rejected as a tool-level error result
        // (isError) rather than analyzed, so the agent can see and recover.
        let response = server.handle_tool_call(Some(params)).unwrap();
        assert_eq!(response["isError"], true);
    }

    #[test]
    fn test_handle_tool_call_allows_outside_path_with_flag() {
        let server_root = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();
        std::fs::write(
            target_dir.path().join("target.rs"),
            "fn target_function() {}\n",
        )
        .unwrap();
        let server = McpServer::new_with_external_paths(
            server_root.path().to_path_buf(),
            Config::default(),
            true,
        );
        let params = json!({
            "name": "complexity",
            "arguments": {"path": target_dir.path().to_str().unwrap()}
        });

        let response = server.handle_tool_call(Some(params)).unwrap();
        let text = response["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("target_function"));
    }

    #[test]
    fn test_handle_tool_call_satd_uses_requested_path() {
        // satd tool should also analyze the requested path, not the server root
        let (server, server_root) = create_test_server();
        std::fs::write(
            server_root.path().join("server_root.rs"),
            "// FIXME: this is in the server root\nfn root_fn() {}\n",
        )
        .unwrap();

        let target_dir = server_root.path().join("target");
        std::fs::create_dir(&target_dir).unwrap();
        std::fs::write(
            target_dir.join("target.py"),
            "# TODO: unique_target_todo\ndef target_func(): pass\n",
        )
        .unwrap();

        let params = json!({
            "name": "satd",
            "arguments": {"path": target_dir.to_str().unwrap()}
        });
        let response = server.handle_tool_call(Some(params)).unwrap();
        let text = response["content"][0]["text"]
            .as_str()
            .expect("tool response text should be a string");

        assert!(
            text.contains("unique_target_todo"),
            "satd should find TODO in the requested path, got: {text}"
        );
    }

    #[test]
    fn test_handle_request_initialize() {
        let (server, _temp_dir) = create_test_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: None,
        };
        let response = server.handle_request(request);
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_handle_request_tools_list() {
        let (server, _temp_dir) = create_test_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "tools/list".to_string(),
            params: None,
        };
        let response = server.handle_request(request);
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_handle_request_shutdown() {
        let (server, _temp_dir) = create_test_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "shutdown".to_string(),
            params: None,
        };
        let response = server.handle_request(request);
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_handle_request_unknown_method() {
        let (server, _temp_dir) = create_test_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "unknown/method".to_string(),
            params: None,
        };
        let response = server.handle_request(request);
        assert!(response.result.is_none());
        let error = response.error.unwrap();
        assert_eq!(error.code, -32601);
        assert!(error.message.contains("Unknown method"));
    }

    #[test]
    fn test_handle_request_protocol_faults_of_a_tool_call_use_invalid_params_code() {
        let (server, _temp_dir) = create_test_server();
        for params in [
            None,
            Some(json!({"arguments": {}})),
            Some(json!({"name": "no_such_tool", "arguments": {}})),
        ] {
            let response = server.handle_request(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(json!(1)),
                method: "tools/call".to_string(),
                params,
            });
            assert!(response.result.is_none());
            assert_eq!(response.error.unwrap().code, -32602);
        }
    }

    /// Calls a tool and gives the error object of the result.
    fn error_of(server: &McpServer, tool: &str, arguments: Value) -> Value {
        let result = call(server, tool, arguments);
        assert_eq!(result["isError"], true, "{result}");
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, result["structuredContent"]);
        text
    }

    #[test]
    fn a_missing_argument_is_a_tool_error_and_not_a_protocol_error() {
        let (server, _temp_dir) = create_test_server();
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "tools/call".to_string(),
            params: Some(json!({"name": "impact", "arguments": {}})),
        });
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        let error = &result["structuredContent"]["error"];
        assert_eq!(error["code"], "missing_argument");
        assert_eq!(error["param"], "symbol");
    }

    #[test]
    fn a_number_of_the_wrong_type_is_an_error_and_not_the_default() {
        let (server, temp_dir) = create_git_test_server();
        let body = error_of(
            &server,
            "churn",
            json!({"path": temp_dir.path(), "days": "3650"}),
        );
        assert_eq!(body["error"]["code"], "invalid_argument");
        assert_eq!(body["error"]["param"], "days");
        assert_eq!(body["error"]["expected"], "an integer from 1 to 36500");
        assert_eq!(body["error"]["received"], "3650");
    }

    #[test]
    fn a_value_that_is_not_in_the_enum_is_an_error_and_not_the_default() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(temp_dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        let body = error_of(&server, "impact", json!({"symbol": "a", "direction": "up"}));
        assert_eq!(body["error"]["code"], "invalid_argument");
        assert_eq!(body["error"]["param"], "direction");
        assert_eq!(body["error"]["expected"], "one of: callers, callees, both");

        let body = error_of(&server, "context", json!({"format": "yaml"}));
        assert_eq!(body["error"]["param"], "format");
    }

    #[test]
    fn an_unknown_argument_is_an_error_with_suggestions() {
        let (server, _temp_dir) = create_test_server();
        let body = error_of(&server, "complexity", json!({"pth": "."}));
        assert_eq!(body["error"]["code"], "unknown_argument");
        assert_eq!(body["error"]["param"], "pth");
        assert_eq!(body["error"]["suggestions"], json!(["path"]));
    }

    #[test]
    fn two_problems_in_one_call_are_two_errors() {
        let (server, _temp_dir) = create_test_server();
        let body = error_of(&server, "impact", json!({"depth": "2", "symbl": "run"}));
        let codes: Vec<&str> = body["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|error| error["code"].as_str().unwrap())
            .collect();
        assert_eq!(
            codes,
            ["missing_argument", "invalid_argument", "unknown_argument"]
        );
        assert_eq!(body["error"], body["errors"][0]);
        assert_eq!(body["errors"][2]["suggestions"], json!(["symbol"]));
    }

    #[test]
    fn arguments_that_are_not_an_object_are_a_tool_error() {
        let (server, _temp_dir) = create_test_server();
        let body = error_of(&server, "complexity", json!("src"));
        assert_eq!(body["error"]["code"], "invalid_argument");
        assert_eq!(body["error"]["param"], Value::Null);
    }

    #[test]
    fn a_call_without_arguments_uses_the_defaults() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(temp_dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        let result = server
            .handle_tool_call(Some(json!({"name": "complexity"})))
            .unwrap();
        assert!(result.get("isError").is_none());
    }

    #[test]
    fn test_handle_request_tool_execution_failure_is_tool_error_result() {
        let (server, _temp_dir) = create_test_server();
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "complexity",
                "arguments": {"path": "/definitely/not/a/repository"}
            })),
        });
        assert!(response.error.is_none());
        assert_eq!(response.result.unwrap()["isError"], true);
    }

    #[test]
    fn test_handle_request_preserves_id() {
        let (server, _temp_dir) = create_test_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(42)),
            method: "initialize".to_string(),
            params: None,
        };
        let response = server.handle_request(request);
        assert_eq!(response.id, Some(json!(42)));
    }

    #[test]
    fn test_json_rpc_response_serialization() {
        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            result: Some(json!({"status": "ok"})),
            error: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"status\":\"ok\""));
        assert!(!json.contains("error"));
    }

    #[test]
    fn test_json_rpc_error_serialization() {
        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request".to_string(),
                data: None,
            }),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"code\":-32600"));
        assert!(json.contains("Invalid Request"));
        assert!(!json.contains("result"));
    }

    #[test]
    fn test_tools_have_input_schema() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        for tool in tools {
            assert!(
                tool.get("inputSchema").is_some(),
                "Tool {} missing inputSchema",
                tool.get("name").unwrap()
            );
        }
    }

    #[test]
    fn test_tools_have_descriptions() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        for tool in tools {
            assert!(
                tool.get("description").is_some(),
                "Tool {} missing description",
                tool.get("name").unwrap()
            );
        }
    }

    fn create_git_test_server() -> (McpServer, TempDir) {
        use std::process::Command;

        let temp_dir = TempDir::new().unwrap();

        // Initialize git repo
        Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .output()
            .expect("Failed to init git repo");

        // Configure git user for commits
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(temp_dir.path())
            .output()
            .expect("Failed to configure git email");

        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(temp_dir.path())
            .output()
            .expect("Failed to configure git name");

        // Create and commit a test file
        std::fs::write(temp_dir.path().join("test.rs"), "fn main() {}").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .expect("Failed to git add");

        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(temp_dir.path())
            .output()
            .expect("Failed to git commit");

        let config = Config::default();
        let server = McpServer::new(temp_dir.path().to_path_buf(), config);
        (server, temp_dir)
    }

    #[test]
    fn test_handle_tool_call_ownership_with_git() {
        let (server, temp_dir) = create_git_test_server();

        let params = json!({
            "name": "ownership",
            "arguments": {"path": temp_dir.path().to_str().unwrap()}
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "ownership tool should succeed with git history: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_handle_tool_call_churn_with_git() {
        let (server, temp_dir) = create_git_test_server();

        let params = json!({
            "name": "churn",
            "arguments": {"path": temp_dir.path().to_str().unwrap()}
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "churn tool should succeed with git history: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_handle_tool_call_churn_honors_days() {
        let (server, temp_dir) = create_git_test_server();
        let response = server
            .handle_tool_call(Some(json!({
                "name": "churn",
                "arguments": {"path": temp_dir.path(), "days": 3650}
            })))
            .unwrap();
        let envelope: Value =
            serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(envelope["result"]["period_days"], 3650);
    }

    #[test]
    fn test_handle_tool_call_repomap_honors_max_symbols() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(
            temp_dir.path().join("symbols.rs"),
            "fn one() {}\nfn two() {}\nfn three() {}\n",
        )
        .unwrap();
        let response = server
            .handle_tool_call(Some(json!({
                "name": "repomap",
                "arguments": {"path": temp_dir.path(), "max_symbols": 1, "limit": 0}
            })))
            .unwrap();
        let envelope: Value =
            serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(envelope["result"]["symbols"].as_array().unwrap().len() <= 1);
        // The envelope tells that `max_symbols` cut the list.
        assert_eq!(
            envelope["truncated"],
            json!([{"path": "/symbols", "total": 3, "returned": 1}])
        );
        let whole = envelope_of(&server, "repomap", json!({"max_symbols": 3, "limit": 0}));
        assert_eq!(whole["truncated"], json!([]));
    }

    #[test]
    fn test_handle_tool_call_clones_honors_thresholds() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(
            temp_dir.path().join("clones.rs"),
            "fn a() { let x = 1; let y = 2; }\nfn b() { let x = 1; let y = 2; }\n",
        )
        .unwrap();
        let response = server
            .handle_tool_call(Some(json!({
                "name": "clones",
                "arguments": {"path": temp_dir.path(), "min_tokens": 8, "similarity": 0.42, "limit": 0}
            })))
            .unwrap();
        let envelope: Value =
            serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(envelope["result"]["min_lines"], 1);
        assert_eq!(envelope["result"]["threshold"], 0.42);
    }

    #[test]
    fn test_handle_tool_call_temporal_honors_days() {
        let (server, temp_dir) = create_git_test_server();
        let response = server
            .handle_tool_call(Some(json!({
                "name": "temporal",
                "arguments": {"path": temp_dir.path(), "days": 321}
            })))
            .unwrap();
        let envelope: Value =
            serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(envelope["result"]["period_days"], 321);
    }

    #[test]
    fn test_handle_tool_call_flags_honors_provider() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(
            temp_dir.path().join("flags.rs"),
            "fn f() { if std::env::var(\"FEATURE_ALPHA\").is_ok() {} }\n",
        )
        .unwrap();
        let response = server
            .handle_tool_call(Some(json!({
                "name": "flags",
                "arguments": {"path": temp_dir.path(), "provider": "nonexistent", "limit": 0}
            })))
            .unwrap();
        let envelope: Value =
            serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(envelope["result"]["summary"]["total_flags"], 0);
    }

    #[test]
    fn test_handle_tool_call_hotspot_with_git() {
        let (server, temp_dir) = create_git_test_server();

        let params = json!({
            "name": "hotspot",
            "arguments": {"path": temp_dir.path().to_str().unwrap()}
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "hotspot tool should succeed with git history: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_handle_tool_call_temporal_with_git() {
        let (server, temp_dir) = create_git_test_server();

        let params = json!({
            "name": "temporal",
            "arguments": {"path": temp_dir.path().to_str().unwrap()}
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "temporal tool should succeed with git history: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_handle_tools_list_has_semantic_search() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_semantic_search = tools
            .iter()
            .any(|t| t.get("name").unwrap() == "semantic_search");
        assert!(has_semantic_search);
    }

    #[test]
    fn test_semantic_search_missing_query() {
        let (server, _temp_dir) = create_test_server();
        let params = json!({
            "name": "semantic_search",
            "arguments": {}
        });
        let result = server.handle_tool_call(Some(params));
        let error = &result.unwrap()["structuredContent"]["error"];
        assert_eq!(error["code"], "missing_argument");
        assert_eq!(error["param"], "query");
    }

    #[test]
    fn test_handle_tools_list_has_semantic_search_hyde() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_hyde = tools
            .iter()
            .any(|t| t.get("name").unwrap() == "semantic_search_hyde");
        assert!(has_hyde);
    }

    #[test]
    fn test_semantic_search_hyde_missing_document() {
        let (server, _temp_dir) = create_test_server();
        let params = json!({
            "name": "semantic_search_hyde",
            "arguments": {}
        });
        let result = server.handle_tool_call(Some(params));
        let error = &result.unwrap()["structuredContent"]["error"];
        assert_eq!(error["code"], "missing_argument");
        assert_eq!(error["param"], "hypothetical_document");
    }

    #[test]
    fn test_handle_tool_call_score() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(temp_dir.path().join("test.rs"), "fn main() {}").unwrap();

        let params = json!({
            "name": "score",
            "arguments": {"path": temp_dir.path().to_str().unwrap()}
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "score tool should succeed: {:?}",
            result.err()
        );
        let response = result.unwrap();
        assert!(response.get("content").is_some());
    }

    #[test]
    fn test_handle_tool_call_diff() {
        let (server, temp_dir) = create_git_test_server();

        // Create a second commit so there is something to diff
        std::fs::write(temp_dir.path().join("new.rs"), "fn foo() {}").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "second"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        let params = json!({
            "name": "diff",
            "arguments": {
                "path": temp_dir.path().to_str().unwrap(),
                "base": "HEAD~1"
            }
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "diff tool should succeed: {:?}",
            result.err()
        );
        let response = result.unwrap();
        assert!(response.get("content").is_some());
    }

    #[test]
    fn test_all_listed_tools_have_handlers() {
        let (server, temp_dir) = create_test_server();
        let tools_result = server.handle_tools_list().unwrap();
        let tools = tools_result.get("tools").unwrap().as_array().unwrap();

        for tool in tools {
            let name = tool.get("name").unwrap().as_str().unwrap();
            let params = json!({
                "name": name,
                "arguments": {"path": temp_dir.path().to_str().unwrap()}
            });
            let result = server.handle_tool_call(Some(params));
            // The tool may fail due to missing git repo or other env issues,
            // but it must NOT fail with "Unknown tool".
            if let Err(ref msg) = result {
                assert!(
                    !msg.contains("Unknown tool"),
                    "Tool '{}' is listed but has no handler",
                    name
                );
            }
        }
    }

    #[test]
    fn test_semantic_search_with_max_complexity() {
        let (server, temp_dir) = create_test_server();

        // Create a Rust file with a simple function so the index is non-empty
        std::fs::write(temp_dir.path().join("test.rs"), "fn simple() { return; }\n").unwrap();

        let params = json!({
            "name": "semantic_search",
            "arguments": {
                "query": "simple",
                "max_complexity": 5
            }
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "semantic_search with max_complexity should succeed: {result:?}"
        );
        let response = result.unwrap();
        assert!(response.get("content").is_some());
    }

    #[test]
    fn test_semantic_search_hyde_with_max_complexity() {
        let (server, temp_dir) = create_test_server();

        std::fs::write(temp_dir.path().join("test.rs"), "fn simple() { return; }\n").unwrap();

        let params = json!({
            "name": "semantic_search_hyde",
            "arguments": {
                "hypothetical_document": "fn simple() { return; }",
                "max_complexity": 5
            }
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "semantic_search_hyde with max_complexity should succeed: {result:?}"
        );
        let response = result.unwrap();
        assert!(response.get("content").is_some());
    }

    #[test]
    fn test_semantic_search_max_complexity_schema_exposed() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();

        for tool_name in ["semantic_search", "semantic_search_hyde"] {
            let tool = tools
                .iter()
                .find(|t| t.get("name").unwrap() == tool_name)
                .unwrap_or_else(|| panic!("tool {tool_name} not found"));
            let props = tool.get("inputSchema").unwrap().get("properties").unwrap();
            assert!(
                props.get("max_complexity").is_some(),
                "{tool_name} should expose max_complexity parameter"
            );
        }
    }

    #[test]
    fn test_notification_no_response() {
        // Notifications are JSON-RPC requests without an `id` field.
        // run_stdio skips them, so we test the logic directly: a request
        // with id=None should not be routed to handle_request.
        let request_json = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let parsed: JsonRpcRequest = serde_json::from_str(request_json).unwrap();
        assert!(parsed.id.is_none(), "A notification must have no id field");
    }

    #[test]
    fn test_all_tools_have_limit_and_offset_params() {
        let (server, _temp_dir) = create_test_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "tools/list".to_string(),
            params: None,
        };
        let response = server.handle_request(request);
        let tools = response.result.unwrap();
        let tools = tools["tools"].as_array().unwrap();
        for tool in tools {
            let props = &tool["inputSchema"]["properties"];
            if tool["name"] == "task_context" {
                assert!(props.get("max_bytes").is_some());
                assert!(props.get("limit").is_none());
                continue;
            }
            assert!(
                props.get("limit").is_some(),
                "Tool {} missing limit",
                tool["name"]
            );
            assert!(
                props.get("offset").is_some(),
                "Tool {} missing offset",
                tool["name"]
            );
        }
    }

    #[test]
    fn test_handle_tools_list_has_outline() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_outline = tools.iter().any(|t| t.get("name").unwrap() == "outline");
        assert!(has_outline, "tools list should include 'outline'");
    }

    #[test]
    fn test_handle_tool_call_outline_file() {
        let (server, temp_dir) = create_test_server();
        let fixture = temp_dir.path().join("sample.rs");
        std::fs::write(&fixture, "fn sample() {}\n").unwrap();
        let params = json!({
            "name": "outline",
            "arguments": {"file": fixture.to_str().unwrap()}
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "outline tool call should succeed: {:?}",
            result.err()
        );
        let response = result.unwrap();
        let text = response["content"][0]["text"].as_str().unwrap();
        // Verify the response contains expected content
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(
            value["result"].get("files").is_some(),
            "envelope result should have 'files' field"
        );
    }

    #[test]
    fn test_handle_tools_list_has_impact() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_impact = tools.iter().any(|t| t.get("name").unwrap() == "impact");
        assert!(has_impact, "tools list should include 'impact'");
    }

    #[test]
    fn test_handle_tool_call_impact() {
        let (server, temp_dir) = create_test_server();
        // Create a simple a→b chain
        std::fs::write(temp_dir.path().join("a.rs"), "fn a() { b(); }\n").unwrap();
        std::fs::write(temp_dir.path().join("b.rs"), "fn b() {}\n").unwrap();

        let params = json!({
            "name": "impact",
            "arguments": {
                "symbol": "b",
                "depth": 1,
                "direction": "callers",
                "path": temp_dir.path().to_str().unwrap()
            }
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "impact tool call should succeed: {:?}",
            result.err()
        );
        let response = result.unwrap();
        let text = response["content"][0]["text"].as_str().unwrap();
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(
            value["result"].get("symbol").is_some(),
            "envelope result should have 'symbol' field"
        );
        assert_eq!(value["result"]["symbol"].as_str().unwrap(), "b");
    }

    #[test]
    fn test_handle_tool_call_impact_missing_symbol() {
        let (server, temp_dir) = create_test_server();
        let params = json!({
            "name": "impact",
            "arguments": {
                "path": temp_dir.path().to_str().unwrap()
            }
        });
        let result = server.handle_tool_call(Some(params));
        // Should return an error (missing symbol)
        assert_eq!(
            result.unwrap()["structuredContent"]["error"]["param"],
            "symbol"
        );
    }

    #[test]
    fn test_handle_tools_list_has_get_symbol() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_tools_list().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        let has_get_symbol = tools.iter().any(|t| t.get("name").unwrap() == "get_symbol");
        assert!(has_get_symbol, "tools list should include 'get_symbol'");
    }

    #[test]
    fn test_handle_tool_call_get_symbol() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(
            temp_dir.path().join("a.rs"),
            "fn a_function() {\n    let x = 1;\n}\n",
        )
        .unwrap();

        let params = json!({
            "name": "get_symbol",
            "arguments": {
                "name": "a_function",
                "path": temp_dir.path().to_str().unwrap()
            }
        });
        let result = server.handle_tool_call(Some(params));
        assert!(
            result.is_ok(),
            "get_symbol tool call should succeed: {:?}",
            result.err()
        );
        let response = result.unwrap();
        let text = response["content"][0]["text"].as_str().unwrap();
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(
            value["result"].get("name").is_some(),
            "envelope result should have 'name' field"
        );
        assert_eq!(value["result"]["name"].as_str().unwrap(), "a_function");
    }

    #[test]
    fn test_handle_tool_call_get_symbol_missing_name() {
        let (server, temp_dir) = create_test_server();
        let params = json!({
            "name": "get_symbol",
            "arguments": {
                "path": temp_dir.path().to_str().unwrap()
            }
        });
        let result = server.handle_tool_call(Some(params));
        assert_eq!(
            result.unwrap()["structuredContent"]["error"]["param"],
            "name"
        );
    }

    #[test]
    fn test_handle_tool_call_get_symbol_unknown_returns_structured_suggestions() {
        let (server, temp_dir) = create_test_server();
        std::fs::write(temp_dir.path().join("known.rs"), "fn known_symbol() {}\n").unwrap();
        let response = server
            .handle_tool_call(Some(json!({
                "name": "get_symbol",
                "arguments": {"name": "known", "path": temp_dir.path()}
            })))
            .unwrap();
        assert_eq!(response["isError"], true);
        let error = &response["structuredContent"]["error"];
        assert_eq!(error["code"], "not_found");
        assert_eq!(error["param"], "name");
        assert_eq!(error["received"], "known");
        assert_eq!(error["suggestions"], json!(["known_symbol"]));
    }
}
