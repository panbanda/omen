//! MCP (Model Context Protocol) server implementation.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;
use crate::core::{AnalysisContext, Analyzer, FileSet, Result};

/// MCP Server for LLM tool integration.
pub struct McpServer {
    config: Config,
    root_path: PathBuf,
}

impl McpServer {
    pub fn new(root_path: PathBuf, config: Config) -> Self {
        Self { config, root_path }
    }

    /// Run the MCP server with stdio transport.
    pub fn run_stdio(&self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let reader = BufReader::new(stdin.lock());
        let mut writer = stdout.lock();

        for line in reader.lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<JsonRpcRequest>(&line) {
                Ok(request) => {
                    let response = self.handle_request(request);
                    serde_json::to_writer(&mut writer, &response)?;
                    writeln!(writer)?;
                    writer.flush()?;
                }
                Err(e) => {
                    let error_response = JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32700,
                            message: format!("Parse error: {}", e),
                            data: None,
                        }),
                    };
                    serde_json::to_writer(&mut writer, &error_response)?;
                    writeln!(writer)?;
                    writer.flush()?;
                }
            }
        }

        Ok(())
    }

    fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tool_call(request.params),
            "shutdown" => Ok(json!({})),
            _ => Err(format!("Unknown method: {}", request.method)),
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
                    code: -32603,
                    message: msg,
                    data: None,
                }),
            },
        }
    }

    fn handle_initialize(&self) -> std::result::Result<Value, String> {
        Ok(json!({
            "protocolVersion": "2024-11-05",
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
        Ok(json!({
            "tools": [
                {
                    "name": "complexity",
                    "description": "Analyze code complexity (cyclomatic and cognitive)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"},
                            "threshold": {"type": "number", "description": "Minimum complexity to report"}
                        }
                    }
                },
                {
                    "name": "satd",
                    "description": "Detect Self-Admitted Technical Debt (TODO, FIXME, HACK, etc.)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"}
                        }
                    }
                },
                {
                    "name": "deadcode",
                    "description": "Find dead/unreachable code",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"}
                        }
                    }
                },
                {
                    "name": "churn",
                    "description": "Analyze code churn from git history",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"},
                            "days": {"type": "integer", "description": "Number of days to analyze"}
                        }
                    }
                },
                {
                    "name": "clones",
                    "description": "Detect code duplicates/clones",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"},
                            "min_tokens": {"type": "integer", "description": "Minimum tokens for detection"},
                            "similarity": {"type": "number", "description": "Similarity threshold (0-1)"}
                        }
                    }
                },
                {
                    "name": "defect",
                    "description": "Predict defect-prone files using PMAT metrics",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"},
                            "days": {"type": "integer", "description": "Number of days to analyze"}
                        }
                    }
                },
                {
                    "name": "changes",
                    "description": "Analyze recent changes (JIT risk analysis)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "commit": {"type": "string", "description": "Commit or range to analyze"},
                            "count": {"type": "integer", "description": "Number of commits"}
                        }
                    }
                },
                {
                    "name": "diff",
                    "description": "Analyze a specific diff between commits",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "base": {"type": "string", "description": "Base commit/branch"},
                            "head": {"type": "string", "description": "Head commit/branch"}
                        },
                        "required": ["base"]
                    }
                },
                {
                    "name": "tdg",
                    "description": "Generate Technical Debt Graph",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"}
                        }
                    }
                },
                {
                    "name": "graph",
                    "description": "Analyze dependency graph structure",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"}
                        }
                    }
                },
                {
                    "name": "hotspot",
                    "description": "Find complexity/churn hotspots",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"},
                            "days": {"type": "integer", "description": "Number of days for churn"}
                        }
                    }
                },
                {
                    "name": "temporal",
                    "description": "Detect temporally coupled files",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"},
                            "days": {"type": "integer", "description": "Number of days to analyze"},
                            "min_coupling": {"type": "number", "description": "Minimum coupling strength"}
                        }
                    }
                },
                {
                    "name": "ownership",
                    "description": "Analyze code ownership and bus factor",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"}
                        }
                    }
                },
                {
                    "name": "cohesion",
                    "description": "Calculate CK cohesion metrics (WMC, CBO, RFC, LCOM, DIT, NOC)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"}
                        }
                    }
                },
                {
                    "name": "repomap",
                    "description": "Generate PageRank-ranked symbol map for LLM context",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"},
                            "max_symbols": {"type": "integer", "description": "Maximum symbols to include"}
                        }
                    }
                },
                {
                    "name": "smells",
                    "description": "Detect architectural smells and anti-patterns",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"}
                        }
                    }
                },
                {
                    "name": "flags",
                    "description": "Find and assess feature flags",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"},
                            "provider": {"type": "string", "description": "Flag provider (launchdarkly, split, etc.)"},
                            "stale_days": {"type": "integer", "description": "Days threshold for staleness"}
                        }
                    }
                },
                {
                    "name": "score",
                    "description": "Calculate composite health score",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string", "description": "File or directory path"},
                            "analyzers": {"type": "string", "description": "Analyzers to include (comma-separated)"}
                        }
                    }
                }
            ]
        }))
    }

    fn handle_tool_call(&self, params: Option<Value>) -> std::result::Result<Value, String> {
        let params = params.ok_or("Missing params")?;
        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing tool name")?;
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let path = arguments
            .get("path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.root_path.clone());

        let file_set = FileSet::from_path(&path, &self.config)
            .map_err(|e| format!("Failed to create file set: {}", e))?;

        let ctx = AnalysisContext::new(&file_set, &self.config, Some(&self.root_path));

        let result = match tool_name {
            "complexity" => self.run_analyzer::<crate::analyzers::complexity::Analyzer>(&ctx),
            "satd" => self.run_analyzer::<crate::analyzers::satd::Analyzer>(&ctx),
            "deadcode" => self.run_analyzer::<crate::analyzers::deadcode::Analyzer>(&ctx),
            "churn" => self.run_analyzer::<crate::analyzers::churn::Analyzer>(&ctx),
            "clones" => self.run_analyzer::<crate::analyzers::duplicates::Analyzer>(&ctx),
            "defect" => self.run_analyzer::<crate::analyzers::defect::Analyzer>(&ctx),
            "changes" => self.run_analyzer::<crate::analyzers::changes::Analyzer>(&ctx),
            "tdg" => self.run_analyzer::<crate::analyzers::tdg::Analyzer>(&ctx),
            "graph" => self.run_analyzer::<crate::analyzers::graph::Analyzer>(&ctx),
            "hotspot" => self.run_analyzer::<crate::analyzers::hotspot::Analyzer>(&ctx),
            "temporal" => self.run_analyzer::<crate::analyzers::temporal::Analyzer>(&ctx),
            "ownership" => self.run_analyzer::<crate::analyzers::ownership::Analyzer>(&ctx),
            "cohesion" => self.run_analyzer::<crate::analyzers::cohesion::Analyzer>(&ctx),
            "repomap" => self.run_analyzer::<crate::analyzers::repomap::Analyzer>(&ctx),
            "smells" => self.run_analyzer::<crate::analyzers::smells::Analyzer>(&ctx),
            "flags" => self.run_analyzer::<crate::analyzers::flags::Analyzer>(&ctx),
            _ => Err(format!("Unknown tool: {}", tool_name)),
        }?;

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        }))
    }

    fn run_analyzer<A: Analyzer + Default>(
        &self,
        ctx: &AnalysisContext<'_>,
    ) -> std::result::Result<Value, String> {
        let analyzer = A::default();
        let result = analyzer
            .analyze(ctx)
            .map_err(|e| format!("Analysis failed: {}", e))?;
        serde_json::to_value(result).map_err(|e| format!("Serialization failed: {}", e))
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

    fn create_test_server() -> (McpServer, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();
        let server = McpServer::new(temp_dir.path().to_path_buf(), config);
        (server, temp_dir)
    }

    #[test]
    fn test_mcp_server_new() {
        let (server, _temp_dir) = create_test_server();
        assert!(server.root_path.exists());
    }

    #[test]
    fn test_handle_initialize() {
        let (server, _temp_dir) = create_test_server();
        let result = server.handle_initialize().unwrap();
        assert!(result.get("protocolVersion").is_some());
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
        assert!(response.error.is_some());
        assert!(response.error.unwrap().message.contains("Unknown method"));
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
}
