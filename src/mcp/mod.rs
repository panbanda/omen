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
            ToolError::new(
                ErrorCode::NotFound,
                format!(
                    "The server cannot find the path '{}': {error}",
                    candidate.display()
                ),
            )
            .with_param(param)
            .with_received(&received)
            .with_next("Give a path that exists.")
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
            }
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
                let envelope = envelope::build(
                    spec.name,
                    spec.paginated,
                    data.result,
                    page,
                    data.git_skipped_reason.as_deref(),
                    compact,
                );
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
            result: serde_json::to_value(result)?,
            git_skipped_reason,
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
                {"name": "same.rb:helper", "start_line": 2, "signature": "def helper"},
                {"name": "same.rb:helper", "start_line": 7, "signature": "def helper"}
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
    fn a_path_that_does_not_exist_is_a_not_found_error() {
        let (server, _dir) = create_test_server();
        let body = error_of(&server, "complexity", json!({"path": "no/such/directory"}));
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["param"], "path");
        assert_eq!(body["error"]["received"], "no/such/directory");
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
