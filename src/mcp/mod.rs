//! MCP (Model Context Protocol) server implementation.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;
use crate::core::{AnalysisContext, Analyzer, FileSet, Result};
use crate::git::GitRepo;

struct ToolDef {
    name: &'static str,
    description: &'static str,
    properties: Vec<(&'static str, serde_json::Value)>,
    required: &'static [&'static str],
}

impl ToolDef {
    fn to_json(&self) -> serde_json::Value {
        let mut props = serde_json::Map::new();
        for (k, v) in &self.properties {
            props.insert(k.to_string(), v.clone());
        }
        // Shared pagination params on every tool
        props.insert(
            "limit".to_string(),
            json!({
                "type": "integer",
                "description": "Max items to return (default: 50, 0 = unlimited)"
            }),
        );
        props.insert(
            "offset".to_string(),
            json!({
                "type": "integer",
                "description": "Item offset for pagination (default: 0)"
            }),
        );
        let mut schema = json!({
            "type": "object",
            "properties": props
        });
        if !self.required.is_empty() {
            schema["required"] = json!(self.required);
        }
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": schema
        })
    }
}

/// Count total items across all top-level arrays in the value.
/// Returns `None` only if there are no arrays at all.
fn count_items(value: &serde_json::Value) -> Option<usize> {
    match value {
        serde_json::Value::Array(arr) => Some(arr.len()),
        serde_json::Value::Object(map) => {
            let mut total = 0usize;
            let mut found_any = false;
            for v in map.values() {
                if let Some(arr) = v.as_array() {
                    total += arr.len();
                    found_any = true;
                }
            }
            if found_any {
                Some(total)
            } else {
                None
            }
        }
        _ => None,
    }
}

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

    fn tool_response(
        &self,
        tool_name: &str,
        mut value: serde_json::Value,
        args: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, String> {
        let limit = args["limit"].as_u64().unwrap_or(50) as usize;
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;

        let total_items = count_items(&value);
        crate::output::truncate_lists(&mut value, limit, offset);
        // Count returned items by summing array lengths AFTER truncation.
        let returned = count_items(&value);

        let envelope = json!({
            "tool": tool_name,
            "total_items": total_items,
            "returned": returned,
            "offset": offset,
            "result": value
        });

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&envelope).unwrap_or_default()
            }]
        }))
    }

    fn handle_tools_list(&self) -> std::result::Result<Value, String> {
        let tools: Vec<serde_json::Value> = vec![
            ToolDef {
                name: "context",
                description: "Use first. Returns top-N PageRank-ranked symbols, risks, language breakdown, and navigation hints. Cheap.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                    ("max_symbols", json!({"type": "integer", "description": "Maximum symbols to include"})),
                    ("max_risks", json!({"type": "integer", "description": "Maximum risks to include"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "outline",
                description: "Token-cheapest file map. Returns imports, classes with methods, and top-level functions for one file or all repo files. Use before reading full source.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "Repository or directory path"})),
                    ("file", json!({"type": "string", "description": "Single file to outline (optional; omit for whole repo)"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "complexity",
                description: "Returns cyclomatic and cognitive complexity per function. Fast.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                    ("threshold", json!({"type": "number", "description": "Minimum complexity to report"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "satd",
                description: "Detects TODO/FIXME/HACK debt comments. Fast.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "deadcode",
                description: "Use when you suspect unused symbols. Finds dead/unreachable code.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "churn",
                description: "Use to find frequently changed files. Analyzes code churn from git history.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                    ("days", json!({"type": "integer", "description": "Number of days to analyze"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "clones",
                description: "Use when looking for duplication or copy-paste debt. Detects code clones via MinHash+LSH.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                    ("min_tokens", json!({"type": "integer", "description": "Minimum tokens for detection"})),
                    ("similarity", json!({"type": "number", "description": "Similarity threshold (0-1)"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "defect",
                description: "Use to identify high-risk files. Predicts defect-prone files using PMAT metrics.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                    ("days", json!({"type": "integer", "description": "Number of days to analyze"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "changes",
                description: "Use to assess recent commit risk. Analyzes recent changes with JIT risk analysis.",
                properties: vec![
                    ("commit", json!({"type": "string", "description": "Commit or range to analyze"})),
                    ("count", json!({"type": "integer", "description": "Number of commits"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "diff",
                description: "Use to review branch or PR risk. Analyzes diff between commits; auto-detects target branch.",
                properties: vec![
                    ("base", json!({"type": "string", "description": "Base commit/branch"})),
                    ("head", json!({"type": "string", "description": "Head commit/branch"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "tdg",
                description: "Use to understand technical debt accumulation. Generates Technical Debt Gradient with critical defect detection.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "graph",
                description: "Use to visualize module dependencies. Generates Mermaid dependency graph.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "hotspot",
                description: "Use to find the riskiest files. Combines high churn + high complexity.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                    ("days", json!({"type": "integer", "description": "Number of days for churn"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "temporal",
                description: "Use to find files that always change together. Detects temporal coupling from git history.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                    ("days", json!({"type": "integer", "description": "Number of days to analyze"})),
                    ("min_coupling", json!({"type": "number", "description": "Minimum coupling strength"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "ownership",
                description: "Use to assess bus factor and knowledge silos. Analyzes code ownership from git blame.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "cohesion",
                description: "Use for OO design quality assessment. Calculates CK metrics: WMC, CBO, RFC, LCOM4, DIT, NOC.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "repomap",
                description: "Returns PageRank-ranked symbol call graph. Use for understanding code structure.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                    ("max_symbols", json!({"type": "integer", "description": "Maximum symbols to include"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "smells",
                description: "Use to find architectural anti-patterns. Detects cycles and smells via Tarjan SCC.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "flags",
                description: "Use to audit feature flags. Finds stale and active feature flags across providers.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                    ("provider", json!({"type": "string", "description": "Flag provider (launchdarkly, split, etc.)"})),
                    ("stale_days", json!({"type": "integer", "description": "Days threshold for staleness"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "score",
                description: "Use for an overall health summary. Calculates composite repository health score.",
                properties: vec![
                    ("path", json!({"type": "string", "description": "File or directory path"})),
                    ("analyzers", json!({"type": "string", "description": "Analyzers to include (comma-separated)"})),
                ],
                required: &[],
            },
            ToolDef {
                name: "semantic_search",
                description: "Semantic symbol search via TF-IDF. Use when you know what you're looking for conceptually.",
                properties: vec![
                    ("query", json!({"type": "string", "description": "Natural language search query"})),
                    ("top_k", json!({"type": "integer", "description": "Maximum number of results (default: 10)"})),
                    ("min_score", json!({"type": "number", "description": "Minimum similarity score 0-1 (default: 0.3)"})),
                    ("files", json!({"type": "string", "description": "Comma-separated file paths to search within"})),
                    ("max_complexity", json!({"type": "integer", "description": "Exclude symbols with cyclomatic complexity above this value"})),
                    ("include_projects", json!({"type": "string", "description": "Comma-separated paths to additional project roots for cross-repo search"})),
                ],
                required: &["query"],
            },
            ToolDef {
                name: "get_symbol",
                description: "One-call symbol report: exact source slice, signature, location, direct callers/callees, and complexity. Use instead of reading whole files.",
                properties: vec![
                    ("name", json!({"type": "string", "description": "Symbol name to look up (bare name or qualified file:name)"})),
                    ("include_source", json!({"type": "boolean", "description": "Whether to include source code (default: true)"})),
                    ("max_source_lines", json!({"type": "integer", "description": "Maximum source lines to return (default: 200)"})),
                    ("path", json!({"type": "string", "description": "Repository root path"})),
                ],
                required: &["name"],
            },
            ToolDef {
                name: "impact",
                description: "Use to understand blast radius before changing a symbol. Returns transitive callers and callees by BFS depth, plus affected files.",
                properties: vec![
                    ("symbol", json!({"type": "string", "description": "Symbol name to analyze (bare name or qualified file:name)"})),
                    ("depth", json!({"type": "integer", "description": "BFS depth for traversal (default: 2)"})),
                    ("direction", json!({"type": "string", "enum": ["callers", "callees", "both"], "description": "Direction of traversal (default: both)"})),
                    ("path", json!({"type": "string", "description": "Repository root path"})),
                ],
                required: &["symbol"],
            },
            ToolDef {
                name: "semantic_search_hyde",
                description: "Search using hypothetical code. Write a code snippet resembling what you want to find. This is matched against the index, producing better results than keyword queries.",
                properties: vec![
                    ("hypothetical_document", json!({"type": "string", "description": "A code snippet resembling the code you want to find"})),
                    ("query", json!({"type": "string", "description": "Original query for display purposes"})),
                    ("top_k", json!({"type": "integer", "description": "Maximum number of results (default: 10)"})),
                    ("min_score", json!({"type": "number", "description": "Minimum similarity score 0-1 (default: 0.3)"})),
                    ("files", json!({"type": "string", "description": "Comma-separated file paths to search within"})),
                    ("max_complexity", json!({"type": "integer", "description": "Exclude symbols with cyclomatic complexity above this value"})),
                    ("include_projects", json!({"type": "string", "description": "Comma-separated paths to additional project roots for cross-repo search"})),
                ],
                required: &["hypothetical_document"],
            },
        ]
        .into_iter()
        .map(|t| t.to_json())
        .collect();

        Ok(json!({ "tools": tools }))
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

        let mut ctx = AnalysisContext::new(&file_set, &self.config, Some(&path));
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
            "context" => {
                return self.handle_context(&path, &file_set, &arguments);
            }
            "diff" => {
                return self.handle_diff(&path, &arguments);
            }
            "semantic_search" => {
                return self.handle_semantic_search(&arguments);
            }
            "semantic_search_hyde" => {
                return self.handle_semantic_search_hyde(&arguments);
            }
            "outline" => {
                return self.handle_outline(&path, &arguments);
            }
            "impact" => {
                return self.handle_impact(&path, &file_set, &arguments);
            }
            "get_symbol" => {
                return self.handle_get_symbol(&path, &file_set, &arguments);
            }
            _ => Err(format!("Unknown tool: {}", tool_name)),
        }?;

        self.tool_response(tool_name, result, &arguments)
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

    fn handle_context(
        &self,
        path: &std::path::Path,
        file_set: &FileSet,
        arguments: &Value,
    ) -> std::result::Result<Value, String> {
        let max_symbols = arguments
            .get("max_symbols")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let max_risks = arguments
            .get("max_risks")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let context =
            crate::context::build_context(path, file_set, &self.config, max_symbols, max_risks)
                .map_err(|e| format!("Context failed: {}", e))?;

        let value =
            serde_json::to_value(&context).map_err(|e| format!("Serialization failed: {}", e))?;
        self.tool_response("context", value, arguments)
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

        self.tool_response("diff", value, arguments)
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

        self.tool_response("semantic_search", result, arguments)
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

        self.tool_response("semantic_search_hyde", result, arguments)
    }

    fn handle_outline(
        &self,
        repo_path: &std::path::Path,
        arguments: &Value,
    ) -> std::result::Result<Value, String> {
        use crate::analyzers::outline::{outline_file, Analyzer as OutlineAnalyzer, OutlineResult};
        use crate::core::Analyzer as AnalyzerTrait;

        let result = if let Some(file_str) = arguments.get("file").and_then(|v| v.as_str()) {
            // Single-file mode
            let file_path = std::path::PathBuf::from(file_str);
            let file_outline =
                outline_file(&file_path).map_err(|e| format!("Outline failed: {}", e))?;
            OutlineResult {
                files: vec![file_outline],
            }
        } else {
            // Repo mode
            let file_set = FileSet::from_path(repo_path, &self.config)
                .map_err(|e| format!("Failed to create file set: {}", e))?;
            let git_root = GitRepo::open(repo_path)
                .ok()
                .map(|r| r.root().to_path_buf());
            let mut ctx = AnalysisContext::new(&file_set, &self.config, Some(repo_path));
            if let Some(ref git_path) = git_root {
                ctx = ctx.with_git_path(git_path);
            }
            let analyzer = OutlineAnalyzer;
            analyzer
                .analyze(&ctx)
                .map_err(|e| format!("Outline analysis failed: {}", e))?
        };

        let value =
            serde_json::to_value(&result).map_err(|e| format!("Serialization failed: {}", e))?;

        self.tool_response("outline", value, arguments)
    }

    fn handle_impact(
        &self,
        repo_path: &std::path::Path,
        file_set: &FileSet,
        arguments: &Value,
    ) -> std::result::Result<Value, String> {
        use crate::analyzers::impact::{analyze, Direction};

        let symbol = arguments
            .get("symbol")
            .and_then(|v| v.as_str())
            .ok_or("Missing required 'symbol' parameter")?;

        let depth = arguments
            .get("depth")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(2);

        let direction = match arguments
            .get("direction")
            .and_then(|v| v.as_str())
            .unwrap_or("both")
        {
            "callers" => Direction::Callers,
            "callees" => Direction::Callees,
            _ => Direction::Both,
        };

        let files: Vec<std::path::PathBuf> = file_set.iter().map(|p| repo_path.join(p)).collect();

        let report = analyze(repo_path, &files, symbol, depth, direction)
            .map_err(|e| format!("Impact analysis failed: {}", e))?;

        let value =
            serde_json::to_value(&report).map_err(|e| format!("Serialization failed: {}", e))?;

        self.tool_response("impact", value, arguments)
    }

    fn handle_get_symbol(
        &self,
        repo_path: &std::path::Path,
        file_set: &FileSet,
        arguments: &Value,
    ) -> std::result::Result<Value, String> {
        use crate::symbol::{get_symbol, SymbolOptions};

        let name = arguments
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required 'name' parameter")?;

        let include_source = arguments
            .get("include_source")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let max_source_lines = arguments
            .get("max_source_lines")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(200);

        let files: Vec<std::path::PathBuf> = file_set.iter().map(|p| repo_path.join(p)).collect();

        let opts = SymbolOptions {
            include_source,
            max_source_lines,
        };

        let report = get_symbol(repo_path, &files, name, &opts)
            .map_err(|e| format!("Symbol lookup failed: {}", e))?;

        let value =
            serde_json::to_value(&report).map_err(|e| format!("Serialization failed: {}", e))?;

        self.tool_response("get_symbol", value, arguments)
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
        let (server, _server_root) = create_test_server();
        let target_dir = TempDir::new().unwrap();
        std::fs::write(
            target_dir.path().join("target.rs"),
            "fn target_function() {}\n",
        )
        .unwrap();

        let params = json!({
            "name": "complexity",
            "arguments": {"path": target_dir.path().to_str().unwrap()}
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
        assert!(text.contains("mcp_entrypoint"));
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

    #[test]
    fn test_tool_response_envelope_fields() {
        let (server, _temp_dir) = create_test_server();
        let value = json!({"items": [1, 2, 3, 4, 5]});
        let args = json!({});
        let resp = server.tool_response("test_tool", value, &args).unwrap();
        let text = resp["content"][0]["text"].as_str().unwrap();
        let envelope: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(envelope["tool"].as_str().unwrap(), "test_tool");
        assert!(envelope.get("total_items").is_some());
        assert!(envelope.get("returned").is_some());
        assert!(envelope.get("offset").is_some());
        assert!(envelope.get("result").is_some());
    }

    #[test]
    fn test_tool_response_is_compact() {
        let (server, _temp_dir) = create_test_server();
        let value = json!({"items": [1, 2, 3]});
        let resp = server.tool_response("test", value, &json!({})).unwrap();
        let text = resp["content"][0]["text"].as_str().unwrap();
        // Compact: only one line (no internal newlines)
        assert_eq!(text.trim().lines().count(), 1);
    }

    #[test]
    fn test_tool_response_default_limit_50() {
        let (server, _temp_dir) = create_test_server();
        let items: Vec<i64> = (0..100).collect();
        let value = json!({"items": items});
        let resp = server.tool_response("test", value, &json!({})).unwrap();
        let text = resp["content"][0]["text"].as_str().unwrap();
        let envelope: serde_json::Value = serde_json::from_str(text).unwrap();
        let returned = envelope["returned"].as_u64().unwrap();
        assert_eq!(returned, 50);
        assert_eq!(envelope["total_items"].as_u64().unwrap(), 100);
    }

    #[test]
    fn test_tool_response_zero_limit_unlimited() {
        let (server, _temp_dir) = create_test_server();
        let items: Vec<i64> = (0..100).collect();
        let value = json!({"items": items});
        let resp = server
            .tool_response("test", value, &json!({"limit": 0}))
            .unwrap();
        let text = resp["content"][0]["text"].as_str().unwrap();
        let envelope: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(envelope["returned"].as_u64().unwrap(), 100);
    }

    #[test]
    fn test_tool_response_offset_pagination() {
        let (server, _temp_dir) = create_test_server();
        let items: Vec<i64> = (0..20).collect();
        let value = json!({"items": items});
        let resp = server
            .tool_response("test", value, &json!({"limit": 5, "offset": 10}))
            .unwrap();
        let text = resp["content"][0]["text"].as_str().unwrap();
        let envelope: serde_json::Value = serde_json::from_str(text).unwrap();
        // Items 10-14 returned
        assert_eq!(envelope["returned"].as_u64().unwrap(), 5);
        assert_eq!(envelope["offset"].as_u64().unwrap(), 10);
    }

    #[test]
    fn test_tool_response_multi_array_counts() {
        // Bug: count_items only counted first array; truncate_lists truncates ALL arrays.
        // With {"components": [101 items], "smells": [10 items]} and limit=5:
        //   total_items must be 111 (101+10), returned must be 10 (5+5).
        let (server, _temp_dir) = create_test_server();
        let components: Vec<i64> = (0..101).collect();
        let smells: Vec<i64> = (0..10).collect();
        let value = json!({"components": components, "smells": smells});
        let resp = server
            .tool_response("test", value, &json!({"limit": 5}))
            .unwrap();
        let text = resp["content"][0]["text"].as_str().unwrap();
        let envelope: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            envelope["total_items"].as_u64().unwrap(),
            111,
            "total_items should sum all arrays"
        );
        assert_eq!(
            envelope["returned"].as_u64().unwrap(),
            10,
            "returned should be sum of items in all arrays after truncation (5+5)"
        );
    }

    #[test]
    fn test_tool_response_single_array_counts_unchanged() {
        // Existing semantics: single array of 60 items, limit 50 → total 60, returned 50.
        let (server, _temp_dir) = create_test_server();
        let items: Vec<i64> = (0..60).collect();
        let value = json!({"items": items});
        let resp = server
            .tool_response("test", value, &json!({"limit": 50}))
            .unwrap();
        let text = resp["content"][0]["text"].as_str().unwrap();
        let envelope: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(envelope["total_items"].as_u64().unwrap(), 60);
        assert_eq!(envelope["returned"].as_u64().unwrap(), 50);
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
        let (server, _temp_dir) = create_test_server();
        let fixture =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.rs");
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
        assert!(result.is_err(), "impact without symbol should fail");
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
        assert!(result.is_err(), "get_symbol without name should fail");
    }
}
