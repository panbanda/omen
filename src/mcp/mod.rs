//! MCP (Model Context Protocol) server implementation.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;
use crate::core::{AnalysisContext, Analyzer, FileSet, Result};
use crate::git::GitRepo;

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
                },
                {
                    "name": "semantic_search",
                    "description": "Search for code symbols using natural language queries",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string", "description": "Natural language search query"},
                            "top_k": {"type": "integer", "description": "Maximum number of results (default: 10)"},
                            "min_score": {"type": "number", "description": "Minimum similarity score 0-1 (default: 0.3)"},
                            "files": {"type": "string", "description": "Comma-separated file paths to search within"},
                            "max_complexity": {"type": "integer", "description": "Exclude symbols with cyclomatic complexity above this value"},
                            "include_projects": {"type": "string", "description": "Comma-separated paths to additional project roots for cross-repo search"}
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "semantic_search_hyde",
                    "description": "Search using hypothetical code. Write a code snippet resembling what you want to find. This is matched against the index, producing better results than keyword queries.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "hypothetical_document": {"type": "string", "description": "A code snippet resembling the code you want to find"},
                            "query": {"type": "string", "description": "Original query for display purposes"},
                            "top_k": {"type": "integer", "description": "Maximum number of results (default: 10)"},
                            "min_score": {"type": "number", "description": "Minimum similarity score 0-1 (default: 0.3)"},
                            "files": {"type": "string", "description": "Comma-separated file paths to search within"},
                            "max_complexity": {"type": "integer", "description": "Exclude symbols with cyclomatic complexity above this value"},
                            "include_projects": {"type": "string", "description": "Comma-separated paths to additional project roots for cross-repo search"}
                        },
                        "required": ["hypothetical_document"]
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

        // Try to open a git repository at the path
        let git_root = GitRepo::open(&path).ok().map(|r| r.root().to_path_buf());

        let mut ctx = AnalysisContext::new(&file_set, &self.config, Some(&self.root_path));
        if let Some(ref git_path) = git_root {
            ctx = ctx.with_git_path(git_path);
        }

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
            "score" => self.run_analyzer::<crate::score::Analyzer>(&ctx),
            "diff" => {
                return self.handle_diff(&path, &arguments);
            }
            "semantic_search" => {
                return self.handle_semantic_search(&arguments);
            }
            "semantic_search_hyde" => {
                return self.handle_semantic_search_hyde(&arguments);
            }
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

    fn handle_diff(
        &self,
        repo_path: &std::path::Path,
        arguments: &Value,
    ) -> std::result::Result<Value, String> {
        let base = arguments.get("base").and_then(|v| v.as_str());
        let head = arguments.get("head").and_then(|v| v.as_str());

        let analyzer = crate::analyzers::changes::Analyzer::default();

        // analyze_diff takes the repo path and an optional target branch.
        // When both base and head are given, we pass base as the target so the
        // diff is computed against it.  The head parameter is not directly
        // supported by analyze_diff (it always diffs the working tree / current
        // branch against the target), so we use base as the target reference.
        let target = base.or(head);
        let result = analyzer
            .analyze_diff(repo_path, target)
            .map_err(|e| format!("Diff analysis failed: {}", e))?;

        let value =
            serde_json::to_value(&result).map_err(|e| format!("Serialization failed: {}", e))?;

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&value).unwrap_or_default()
            }]
        }))
    }

    fn handle_semantic_search(&self, arguments: &Value) -> std::result::Result<Value, String> {
        use crate::semantic::{SearchConfig, SearchFilters, SemanticSearch};

        let query = arguments
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or("Missing required 'query' parameter")?;

        let top_k = arguments
            .get("top_k")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(10);

        let min_score = arguments
            .get("min_score")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or(0.3);

        let max_complexity = arguments
            .get("max_complexity")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let files: Option<Vec<&str>> = arguments
            .get("files")
            .and_then(|v| v.as_str())
            .map(|s| s.split(',').collect());

        let include_projects: Option<Vec<std::path::PathBuf>> = arguments
            .get("include_projects")
            .and_then(|v| v.as_str())
            .map(|s| {
                s.split(',')
                    .filter(|p| !p.trim().is_empty())
                    .map(|p| std::path::PathBuf::from(p.trim()))
                    .collect()
            });

        let search_config = SearchConfig {
            min_score,
            ..SearchConfig::default()
        };

        let search = SemanticSearch::new(&search_config, &self.root_path)
            .map_err(|e| format!("Failed to initialize semantic search: {}", e))?;

        // Ensure index exists (auto-index if needed)
        search
            .index(&self.config)
            .map_err(|e| format!("Failed to index: {}", e))?;

        let mut output = if let Some(ref extra) = include_projects {
            let mut all_projects: Vec<&std::path::Path> = vec![&self.root_path];
            all_projects.extend(extra.iter().map(|p| p.as_path()));
            let mr = crate::semantic::multi_repo::multi_repo_search(
                &all_projects,
                query,
                top_k,
                min_score,
            )
            .map_err(|e| format!("Multi-repo search failed: {}", e))?;
            crate::semantic::SearchOutput::new(query.to_string(), mr.total_symbols, mr.results)
        } else if let Some(file_paths) = files {
            search
                .search_in_files(query, &file_paths, Some(top_k))
                .map_err(|e| format!("Search failed: {}", e))?
        } else if max_complexity.is_some() {
            let filters = SearchFilters {
                min_score,
                max_complexity,
            };
            search
                .search_filtered(query, Some(top_k), &filters)
                .map_err(|e| format!("Search failed: {}", e))?
        } else {
            search
                .search(query, Some(top_k))
                .map_err(|e| format!("Search failed: {}", e))?
        };

        // Apply max_complexity post-filter (works regardless of search path)
        if let Some(max) = max_complexity {
            output
                .results
                .retain(|r| r.cyclomatic_complexity.is_none_or(|c| c <= max));
        }

        let result =
            serde_json::to_value(&output).map_err(|e| format!("Serialization failed: {}", e))?;

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        }))
    }

    fn handle_semantic_search_hyde(&self, arguments: &Value) -> std::result::Result<Value, String> {
        use crate::semantic::{SearchConfig, SearchFilters, SemanticSearch};

        let hypothetical_document = arguments
            .get("hypothetical_document")
            .and_then(|v| v.as_str())
            .ok_or("Missing required 'hypothetical_document' parameter")?;

        let display_query = arguments
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or(hypothetical_document);

        let top_k = arguments
            .get("top_k")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(10);

        let min_score = arguments
            .get("min_score")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or(0.3);

        let max_complexity = arguments
            .get("max_complexity")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let files: Option<Vec<&str>> = arguments
            .get("files")
            .and_then(|v| v.as_str())
            .map(|s| s.split(',').collect());

        let include_projects: Option<Vec<std::path::PathBuf>> = arguments
            .get("include_projects")
            .and_then(|v| v.as_str())
            .map(|s| {
                s.split(',')
                    .filter(|p| !p.trim().is_empty())
                    .map(|p| std::path::PathBuf::from(p.trim()))
                    .collect()
            });

        let search_config = SearchConfig {
            min_score,
            ..SearchConfig::default()
        };

        let search = SemanticSearch::new(&search_config, &self.root_path)
            .map_err(|e| format!("Failed to initialize semantic search: {}", e))?;

        search
            .index(&self.config)
            .map_err(|e| format!("Failed to index: {}", e))?;

        // Use the hypothetical document as the search query text
        let mut output = if let Some(ref extra) = include_projects {
            let mut all_projects: Vec<&std::path::Path> = vec![&self.root_path];
            all_projects.extend(extra.iter().map(|p| p.as_path()));
            let mr = crate::semantic::multi_repo::multi_repo_search(
                &all_projects,
                hypothetical_document,
                top_k,
                min_score,
            )
            .map_err(|e| format!("Multi-repo search failed: {}", e))?;
            crate::semantic::SearchOutput::new(
                hypothetical_document.to_string(),
                mr.total_symbols,
                mr.results,
            )
        } else if let Some(file_paths) = files {
            search
                .search_in_files(hypothetical_document, &file_paths, Some(top_k))
                .map_err(|e| format!("Search failed: {}", e))?
        } else if max_complexity.is_some() {
            let filters = SearchFilters {
                min_score,
                max_complexity,
            };
            search
                .search_filtered(hypothetical_document, Some(top_k), &filters)
                .map_err(|e| format!("Search failed: {}", e))?
        } else {
            search
                .search(hypothetical_document, Some(top_k))
                .map_err(|e| format!("Search failed: {}", e))?
        };

        // Apply max_complexity post-filter (works regardless of search path)
        if let Some(max) = max_complexity {
            output
                .results
                .retain(|r| r.cyclomatic_complexity.is_none_or(|c| c <= max));
        }

        // Replace the query in output with the display query
        output.query = display_query.to_string();

        let result =
            serde_json::to_value(&output).map_err(|e| format!("Serialization failed: {}", e))?;

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        }))
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
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("query"));
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
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("hypothetical_document"));
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
}
