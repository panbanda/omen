//! CLI implementation using clap.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Omen - Code analysis CLI for technical debt and complexity metrics.
#[derive(Parser)]
#[command(name = "omen")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to the repository to analyze
    #[arg(short, long, default_value = ".")]
    pub path: PathBuf,

    /// Output format
    #[arg(short, long, value_enum, default_value = "markdown")]
    pub format: OutputFormat,

    /// Configuration file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Number of parallel workers (default: number of CPUs)
    #[arg(short = 'j', long)]
    pub jobs: Option<usize>,

    /// Disable result caching
    #[arg(long)]
    pub no_cache: bool,

    /// Git ref (branch, tag, SHA) for remote repositories
    #[arg(long = "ref")]
    pub git_ref: Option<String>,

    /// Shallow clone (depth=1); disables git history analyzers
    #[arg(long)]
    pub shallow: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Analyze code complexity (cyclomatic and cognitive)
    #[command(alias = "cx")]
    Complexity(AnalyzerArgs),

    /// Detect Self-Admitted Technical Debt
    #[command(alias = "debt")]
    Satd(AnalyzerArgs),

    /// Find dead/unreachable code
    #[command(alias = "dc")]
    Deadcode(AnalyzerArgs),

    /// Analyze code churn from git history
    Churn(ChurnArgs),

    /// Detect code duplicates/clones
    #[command(alias = "dup", alias = "duplicates")]
    Clones(ClonesArgs),

    /// Predict defect-prone files using PMAT
    #[command(alias = "predict")]
    Defect(DefectArgs),

    /// Analyze recent changes (JIT risk)
    #[command(alias = "jit")]
    Changes(ChangesArgs),

    /// Analyze a specific diff (PR review)
    #[command(alias = "pr")]
    Diff(DiffArgs),

    /// Generate Technical Debt Graph
    Tdg(AnalyzerArgs),

    /// Analyze dependency graph structure
    #[command(alias = "dag")]
    Graph(AnalyzerArgs),

    /// Find complexity/churn hotspots
    #[command(alias = "hs")]
    Hotspot(HotspotArgs),

    /// Detect temporally coupled files
    #[command(alias = "tc", visible_alias = "temporal-coupling")]
    Temporal(TemporalArgs),

    /// Analyze code ownership and bus factor
    #[command(alias = "own", alias = "bus-factor")]
    Ownership(OwnershipArgs),

    /// Calculate CK cohesion metrics
    #[command(alias = "ck")]
    Cohesion(AnalyzerArgs),

    /// Generate PageRank-ranked symbol map
    Repomap(RepomapArgs),

    /// Detect architectural smells
    Smells(AnalyzerArgs),

    /// Find and assess feature flags
    #[command(alias = "ff")]
    Flags(FlagsArgs),

    /// Analyze lint violation density
    #[command(alias = "lh")]
    LintHotspot(LintHotspotArgs),

    /// Calculate composite health score
    Score(ScoreCommand),

    /// Start MCP server for LLM integration
    Mcp(McpCommand),

    /// Run all analyzers
    All(AllArgs),

    /// Generate deep context for LLM consumption
    #[command(alias = "ctx")]
    Context(ContextArgs),

    /// Generate and manage HTML health reports
    Report(ReportCommand),

    /// Semantic search over code symbols
    #[command(alias = "s")]
    Search(SearchCommand),
}

#[derive(Args)]
pub struct AnalyzerArgs {
    /// Filter by file glob pattern
    #[arg(short, long)]
    pub glob: Option<String>,

    /// Exclude files matching pattern
    #[arg(short, long)]
    pub exclude: Option<String>,

    /// Minimum threshold for reporting
    #[arg(short, long)]
    pub threshold: Option<f64>,

    /// Maximum number of results
    #[arg(short = 'n', long)]
    pub limit: Option<usize>,

    /// Sort order
    #[arg(long, value_enum)]
    pub sort: Option<SortOrder>,
}

#[derive(Args)]
pub struct ChurnArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Number of days to analyze
    #[arg(long, default_value = "30")]
    pub days: u32,

    /// Git revision range (e.g., main..HEAD)
    #[arg(long)]
    pub range: Option<String>,
}

#[derive(Args)]
pub struct ClonesArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Minimum tokens for clone detection
    #[arg(long, default_value = "50")]
    pub min_tokens: usize,

    /// Similarity threshold (0.0-1.0)
    #[arg(long, default_value = "0.8")]
    pub similarity: f64,
}

#[derive(Args)]
pub struct DefectArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Number of days to analyze
    #[arg(long, default_value = "30")]
    pub days: u32,

    /// PMAT risk threshold
    #[arg(long, default_value = "0.5")]
    pub risk_threshold: f64,
}

#[derive(Args)]
pub struct ChangesArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Commit or range to analyze
    #[arg(long, default_value = "HEAD")]
    pub commit: String,

    /// Number of recent commits
    #[arg(long, default_value = "1")]
    pub count: usize,
}

#[derive(Args)]
pub struct DiffArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Base commit/branch
    #[arg(long)]
    pub base: String,

    /// Head commit/branch (default: HEAD)
    #[arg(long, default_value = "HEAD")]
    pub head: String,
}

#[derive(Args)]
pub struct HotspotArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Number of days for churn analysis
    #[arg(long, default_value = "30")]
    pub days: u32,

    /// Weight for complexity (0.0-1.0)
    #[arg(long, default_value = "0.5")]
    pub complexity_weight: f64,
}

#[derive(Args)]
pub struct TemporalArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Number of days to analyze
    #[arg(long, default_value = "30")]
    pub days: u32,

    /// Minimum coupling strength (0.0-1.0)
    #[arg(long, default_value = "0.5")]
    pub min_coupling: f64,
}

#[derive(Args)]
pub struct OwnershipArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Bus factor threshold for high risk
    #[arg(long, default_value = "2")]
    pub bus_factor_threshold: usize,
}

#[derive(Args)]
pub struct RepomapArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Maximum symbols to include
    #[arg(long, default_value = "100")]
    pub max_symbols: usize,

    /// PageRank damping factor
    #[arg(long, default_value = "0.85")]
    pub damping: f64,
}

#[derive(Args)]
pub struct FlagsArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Feature flag provider (launchdarkly, split, etc.)
    #[arg(long)]
    pub provider: Option<String>,

    /// Days threshold for staleness
    #[arg(long, default_value = "90")]
    pub stale_days: u32,
}

#[derive(Args)]
pub struct LintHotspotArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Maximum number of results
    #[arg(long, default_value = "10")]
    pub top: usize,
}

/// Score command with subcommands.
#[derive(Args)]
pub struct ScoreCommand {
    #[command(subcommand)]
    pub subcommand: Option<ScoreSubcommand>,

    #[command(flatten)]
    pub args: ScoreArgs,
}

#[derive(Subcommand)]
pub enum ScoreSubcommand {
    /// Analyze score trends over git history
    #[command(alias = "tr")]
    Trend(ScoreTrendArgs),
}

#[derive(Args)]
pub struct ScoreArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Analyzers to include (comma-separated)
    #[arg(long)]
    pub analyzers: Option<String>,

    /// Custom weights file
    #[arg(long)]
    pub weights: Option<PathBuf>,

    /// Number of days for history
    #[arg(long, default_value = "30")]
    pub days: u32,
}

#[derive(Args)]
pub struct ScoreTrendArgs {
    /// Time period (e.g., 3m, 6m, 1y)
    #[arg(short, long, default_value = "3m")]
    pub since: String,

    /// Aggregation period
    #[arg(short, long, value_enum, default_value = "weekly")]
    pub period: TrendPeriod,

    /// Take snapshot of current score
    #[arg(long)]
    pub snap: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum TrendPeriod {
    Daily,
    Weekly,
    Monthly,
}

/// MCP command with subcommands.
#[derive(Args)]
pub struct McpCommand {
    #[command(subcommand)]
    pub subcommand: Option<McpSubcommand>,

    #[command(flatten)]
    pub args: McpArgs,
}

#[derive(Subcommand)]
pub enum McpSubcommand {
    /// Output MCP manifest JSON
    Manifest,
}

#[derive(Args)]
pub struct McpArgs {
    /// Transport type
    #[arg(long, value_enum, default_value = "stdio")]
    pub transport: McpTransport,

    /// Port for SSE transport
    #[arg(long, default_value = "3000")]
    pub port: u16,

    /// Host for SSE transport
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
}

#[derive(Args)]
pub struct AllArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Skip specific analyzers (comma-separated)
    #[arg(long)]
    pub skip: Option<String>,

    /// Number of days for git-based analyzers
    #[arg(long, default_value = "30")]
    pub days: u32,
}

/// Context generation for LLM consumption.
#[derive(Args)]
pub struct ContextArgs {
    /// Target file or directory for context
    #[arg(long)]
    pub target: Option<PathBuf>,

    /// Maximum context tokens to generate
    #[arg(long, default_value = "8000")]
    pub max_tokens: usize,

    /// Include file contents (not just structure)
    #[arg(long, default_value = "true")]
    pub include_content: bool,

    /// Focus on specific symbol/function
    #[arg(long)]
    pub symbol: Option<String>,

    /// Depth for dependency traversal
    #[arg(long, default_value = "2")]
    pub depth: usize,
}

/// Report command with subcommands.
#[derive(Args)]
pub struct ReportCommand {
    #[command(subcommand)]
    pub subcommand: ReportSubcommand,
}

#[derive(Subcommand)]
pub enum ReportSubcommand {
    /// Run all analyzers and output JSON data files
    Generate(ReportGenerateArgs),

    /// Validate data files against schemas
    Validate(ReportValidateArgs),

    /// Combine data + insights into self-contained HTML
    Render(ReportRenderArgs),

    /// Serve HTML with live re-render on request
    Serve(ReportServeArgs),
}

#[derive(Args)]
pub struct ReportGenerateArgs {
    /// Output directory for JSON files
    #[arg(short, long, default_value = ".omen/data")]
    pub output: PathBuf,

    /// Skip specific analyzers (comma-separated)
    #[arg(long)]
    pub skip: Option<String>,

    /// Time period for analysis (e.g., 1m, 3m, 6m, 1y, 2y, all)
    #[arg(long, default_value = "1y")]
    pub since: String,

    /// Number of days for git-based analyzers (alternative to --since)
    #[arg(long)]
    pub days: Option<u32>,
}

#[derive(Args)]
pub struct ReportValidateArgs {
    /// Data directory to validate
    #[arg(short, long, default_value = ".omen/data")]
    pub data: PathBuf,

    /// Schema directory (embedded by default)
    #[arg(long)]
    pub schema: Option<PathBuf>,
}

#[derive(Args)]
pub struct ReportRenderArgs {
    /// Data directory with JSON files
    #[arg(short, long, default_value = ".omen/data")]
    pub data: PathBuf,

    /// Output HTML file
    #[arg(short, long, default_value = ".omen/report.html")]
    pub output: PathBuf,

    /// Insights file (optional)
    #[arg(long)]
    pub insights: Option<PathBuf>,
}

#[derive(Args)]
pub struct ReportServeArgs {
    /// Data directory with JSON files
    #[arg(short, long, default_value = ".omen/data")]
    pub data: PathBuf,

    /// Port to serve on
    #[arg(short, long, default_value = "8080")]
    pub port: u16,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
}

/// Search command with subcommands.
#[derive(Args)]
pub struct SearchCommand {
    #[command(subcommand)]
    pub subcommand: SearchSubcommand,
}

#[derive(Subcommand)]
pub enum SearchSubcommand {
    /// Build or update the search index
    Index(SearchIndexArgs),

    /// Search for code symbols
    Query(SearchQueryArgs),
}

#[derive(Args)]
pub struct SearchIndexArgs {
    /// Force full re-index (ignore cache)
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct SearchQueryArgs {
    /// Natural language query
    pub query: String,

    /// Maximum number of results
    #[arg(short = 'k', long, default_value = "10")]
    pub top_k: usize,

    /// Minimum similarity score (0.0-1.0)
    #[arg(long, default_value = "0.3")]
    pub min_score: f32,

    /// Limit search to specific files (comma-separated)
    #[arg(long)]
    pub files: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Json,
    Markdown,
    Text,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum McpTransport {
    Stdio,
    Sse,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_verify() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_cli_default_path() {
        let cli = Cli::try_parse_from(["omen", "complexity"]).unwrap();
        assert_eq!(cli.path, std::path::PathBuf::from("."));
    }

    #[test]
    fn test_cli_custom_path() {
        let cli = Cli::try_parse_from(["omen", "-p", "/tmp", "complexity"]).unwrap();
        assert_eq!(cli.path, std::path::PathBuf::from("/tmp"));
    }

    #[test]
    fn test_cli_format_json() {
        let cli = Cli::try_parse_from(["omen", "-f", "json", "complexity"]).unwrap();
        assert!(matches!(cli.format, OutputFormat::Json));
    }

    #[test]
    fn test_cli_format_markdown() {
        let cli = Cli::try_parse_from(["omen", "-f", "markdown", "complexity"]).unwrap();
        assert!(matches!(cli.format, OutputFormat::Markdown));
    }

    #[test]
    fn test_cli_format_text() {
        let cli = Cli::try_parse_from(["omen", "-f", "text", "complexity"]).unwrap();
        assert!(matches!(cli.format, OutputFormat::Text));
    }

    #[test]
    fn test_cli_config_flag() {
        let cli = Cli::try_parse_from(["omen", "-c", "config.toml", "complexity"]).unwrap();
        assert_eq!(cli.config, Some(std::path::PathBuf::from("config.toml")));
    }

    #[test]
    fn test_cli_verbose_flag() {
        let cli = Cli::try_parse_from(["omen", "-v", "complexity"]).unwrap();
        assert!(cli.verbose);
    }

    #[test]
    fn test_cli_jobs_flag() {
        let cli = Cli::try_parse_from(["omen", "-j", "4", "complexity"]).unwrap();
        assert_eq!(cli.jobs, Some(4));
    }

    #[test]
    fn test_command_complexity() {
        let cli = Cli::try_parse_from(["omen", "complexity"]).unwrap();
        assert!(matches!(cli.command, Command::Complexity(_)));
    }

    #[test]
    fn test_command_satd() {
        let cli = Cli::try_parse_from(["omen", "satd"]).unwrap();
        assert!(matches!(cli.command, Command::Satd(_)));
    }

    #[test]
    fn test_command_deadcode() {
        let cli = Cli::try_parse_from(["omen", "deadcode"]).unwrap();
        assert!(matches!(cli.command, Command::Deadcode(_)));
    }

    #[test]
    fn test_command_churn() {
        let cli = Cli::try_parse_from(["omen", "churn"]).unwrap();
        assert!(matches!(cli.command, Command::Churn(_)));
    }

    #[test]
    fn test_command_clones() {
        let cli = Cli::try_parse_from(["omen", "clones"]).unwrap();
        assert!(matches!(cli.command, Command::Clones(_)));
    }

    #[test]
    fn test_command_defect() {
        let cli = Cli::try_parse_from(["omen", "defect"]).unwrap();
        assert!(matches!(cli.command, Command::Defect(_)));
    }

    #[test]
    fn test_command_changes() {
        let cli = Cli::try_parse_from(["omen", "changes"]).unwrap();
        assert!(matches!(cli.command, Command::Changes(_)));
    }

    #[test]
    fn test_command_diff() {
        let cli = Cli::try_parse_from(["omen", "diff", "--base", "main"]).unwrap();
        assert!(matches!(cli.command, Command::Diff(_)));
    }

    #[test]
    fn test_command_tdg() {
        let cli = Cli::try_parse_from(["omen", "tdg"]).unwrap();
        assert!(matches!(cli.command, Command::Tdg(_)));
    }

    #[test]
    fn test_command_graph() {
        let cli = Cli::try_parse_from(["omen", "graph"]).unwrap();
        assert!(matches!(cli.command, Command::Graph(_)));
    }

    #[test]
    fn test_command_hotspot() {
        let cli = Cli::try_parse_from(["omen", "hotspot"]).unwrap();
        assert!(matches!(cli.command, Command::Hotspot(_)));
    }

    #[test]
    fn test_command_temporal() {
        let cli = Cli::try_parse_from(["omen", "temporal"]).unwrap();
        assert!(matches!(cli.command, Command::Temporal(_)));
    }

    #[test]
    fn test_command_ownership() {
        let cli = Cli::try_parse_from(["omen", "ownership"]).unwrap();
        assert!(matches!(cli.command, Command::Ownership(_)));
    }

    #[test]
    fn test_command_cohesion() {
        let cli = Cli::try_parse_from(["omen", "cohesion"]).unwrap();
        assert!(matches!(cli.command, Command::Cohesion(_)));
    }

    #[test]
    fn test_command_repomap() {
        let cli = Cli::try_parse_from(["omen", "repomap"]).unwrap();
        assert!(matches!(cli.command, Command::Repomap(_)));
    }

    #[test]
    fn test_command_smells() {
        let cli = Cli::try_parse_from(["omen", "smells"]).unwrap();
        assert!(matches!(cli.command, Command::Smells(_)));
    }

    #[test]
    fn test_command_flags() {
        let cli = Cli::try_parse_from(["omen", "flags"]).unwrap();
        assert!(matches!(cli.command, Command::Flags(_)));
    }

    #[test]
    fn test_command_score() {
        let cli = Cli::try_parse_from(["omen", "score"]).unwrap();
        assert!(matches!(cli.command, Command::Score(_)));
    }

    #[test]
    fn test_command_mcp() {
        let cli = Cli::try_parse_from(["omen", "mcp"]).unwrap();
        assert!(matches!(cli.command, Command::Mcp(_)));
    }

    #[test]
    fn test_command_all() {
        let cli = Cli::try_parse_from(["omen", "all"]).unwrap();
        assert!(matches!(cli.command, Command::All(_)));
    }

    #[test]
    fn test_churn_days_default() {
        let cli = Cli::try_parse_from(["omen", "churn"]).unwrap();
        if let Command::Churn(args) = cli.command {
            assert_eq!(args.days, 30);
        }
    }

    #[test]
    fn test_churn_days_custom() {
        let cli = Cli::try_parse_from(["omen", "churn", "--days", "30"]).unwrap();
        if let Command::Churn(args) = cli.command {
            assert_eq!(args.days, 30);
        }
    }

    #[test]
    fn test_clones_min_tokens() {
        let cli = Cli::try_parse_from(["omen", "clones", "--min-tokens", "100"]).unwrap();
        if let Command::Clones(args) = cli.command {
            assert_eq!(args.min_tokens, 100);
        }
    }

    #[test]
    fn test_clones_similarity() {
        let cli = Cli::try_parse_from(["omen", "clones", "--similarity", "0.9"]).unwrap();
        if let Command::Clones(args) = cli.command {
            assert!((args.similarity - 0.9).abs() < 0.001);
        }
    }

    #[test]
    fn test_defect_risk_threshold() {
        let cli = Cli::try_parse_from(["omen", "defect", "--risk-threshold", "0.7"]).unwrap();
        if let Command::Defect(args) = cli.command {
            assert!((args.risk_threshold - 0.7).abs() < 0.001);
        }
    }

    #[test]
    fn test_diff_base_and_head() {
        let cli =
            Cli::try_parse_from(["omen", "diff", "--base", "main", "--head", "feature"]).unwrap();
        if let Command::Diff(args) = cli.command {
            assert_eq!(args.base, "main");
            assert_eq!(args.head, "feature");
        }
    }

    #[test]
    fn test_hotspot_complexity_weight() {
        let cli = Cli::try_parse_from(["omen", "hotspot", "--complexity-weight", "0.7"]).unwrap();
        if let Command::Hotspot(args) = cli.command {
            assert!((args.complexity_weight - 0.7).abs() < 0.001);
        }
    }

    #[test]
    fn test_temporal_min_coupling() {
        let cli = Cli::try_parse_from(["omen", "temporal", "--min-coupling", "0.6"]).unwrap();
        if let Command::Temporal(args) = cli.command {
            assert!((args.min_coupling - 0.6).abs() < 0.001);
        }
    }

    #[test]
    fn test_ownership_bus_factor_threshold() {
        let cli =
            Cli::try_parse_from(["omen", "ownership", "--bus-factor-threshold", "3"]).unwrap();
        if let Command::Ownership(args) = cli.command {
            assert_eq!(args.bus_factor_threshold, 3);
        }
    }

    #[test]
    fn test_repomap_max_symbols() {
        let cli = Cli::try_parse_from(["omen", "repomap", "--max-symbols", "200"]).unwrap();
        if let Command::Repomap(args) = cli.command {
            assert_eq!(args.max_symbols, 200);
        }
    }

    #[test]
    fn test_repomap_damping() {
        let cli = Cli::try_parse_from(["omen", "repomap", "--damping", "0.9"]).unwrap();
        if let Command::Repomap(args) = cli.command {
            assert!((args.damping - 0.9).abs() < 0.001);
        }
    }

    #[test]
    fn test_flags_provider() {
        let cli = Cli::try_parse_from(["omen", "flags", "--provider", "launchdarkly"]).unwrap();
        if let Command::Flags(args) = cli.command {
            assert_eq!(args.provider, Some("launchdarkly".to_string()));
        }
    }

    #[test]
    fn test_flags_stale_days() {
        let cli = Cli::try_parse_from(["omen", "flags", "--stale-days", "60"]).unwrap();
        if let Command::Flags(args) = cli.command {
            assert_eq!(args.stale_days, 60);
        }
    }

    #[test]
    fn test_mcp_transport_stdio() {
        let cli = Cli::try_parse_from(["omen", "mcp", "--transport", "stdio"]).unwrap();
        if let Command::Mcp(cmd) = cli.command {
            assert!(matches!(cmd.args.transport, McpTransport::Stdio));
        }
    }

    #[test]
    fn test_mcp_transport_sse() {
        let cli = Cli::try_parse_from(["omen", "mcp", "--transport", "sse"]).unwrap();
        if let Command::Mcp(cmd) = cli.command {
            assert!(matches!(cmd.args.transport, McpTransport::Sse));
        }
    }

    #[test]
    fn test_mcp_port() {
        let cli = Cli::try_parse_from(["omen", "mcp", "--port", "8080"]).unwrap();
        if let Command::Mcp(cmd) = cli.command {
            assert_eq!(cmd.args.port, 8080);
        }
    }

    #[test]
    fn test_mcp_host() {
        let cli = Cli::try_parse_from(["omen", "mcp", "--host", "0.0.0.0"]).unwrap();
        if let Command::Mcp(cmd) = cli.command {
            assert_eq!(cmd.args.host, "0.0.0.0");
        }
    }

    #[test]
    fn test_mcp_manifest() {
        let cli = Cli::try_parse_from(["omen", "mcp", "manifest"]).unwrap();
        if let Command::Mcp(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, Some(McpSubcommand::Manifest)));
        }
    }

    #[test]
    fn test_all_skip() {
        let cli = Cli::try_parse_from(["omen", "all", "--skip", "complexity,satd"]).unwrap();
        if let Command::All(args) = cli.command {
            assert_eq!(args.skip, Some("complexity,satd".to_string()));
        }
    }

    #[test]
    fn test_analyzer_args_glob() {
        let cli = Cli::try_parse_from(["omen", "complexity", "-g", "*.rs"]).unwrap();
        if let Command::Complexity(args) = cli.command {
            assert_eq!(args.glob, Some("*.rs".to_string()));
        }
    }

    #[test]
    fn test_analyzer_args_exclude() {
        let cli = Cli::try_parse_from(["omen", "complexity", "-e", "test"]).unwrap();
        if let Command::Complexity(args) = cli.command {
            assert_eq!(args.exclude, Some("test".to_string()));
        }
    }

    #[test]
    fn test_analyzer_args_threshold() {
        let cli = Cli::try_parse_from(["omen", "complexity", "-t", "10.0"]).unwrap();
        if let Command::Complexity(args) = cli.command {
            assert!((args.threshold.unwrap() - 10.0).abs() < 0.001);
        }
    }

    #[test]
    fn test_analyzer_args_limit() {
        let cli = Cli::try_parse_from(["omen", "complexity", "-n", "20"]).unwrap();
        if let Command::Complexity(args) = cli.command {
            assert_eq!(args.limit, Some(20));
        }
    }

    #[test]
    fn test_analyzer_args_sort_asc() {
        let cli = Cli::try_parse_from(["omen", "complexity", "--sort", "asc"]).unwrap();
        if let Command::Complexity(args) = cli.command {
            assert!(matches!(args.sort, Some(SortOrder::Asc)));
        }
    }

    #[test]
    fn test_analyzer_args_sort_desc() {
        let cli = Cli::try_parse_from(["omen", "complexity", "--sort", "desc"]).unwrap();
        if let Command::Complexity(args) = cli.command {
            assert!(matches!(args.sort, Some(SortOrder::Desc)));
        }
    }

    #[test]
    fn test_output_format_default() {
        let cli = Cli::try_parse_from(["omen", "complexity"]).unwrap();
        assert!(matches!(cli.format, OutputFormat::Markdown));
    }

    // Alias tests
    #[test]
    fn test_alias_cx_for_complexity() {
        let cli = Cli::try_parse_from(["omen", "cx"]).unwrap();
        assert!(matches!(cli.command, Command::Complexity(_)));
    }

    #[test]
    fn test_alias_debt_for_satd() {
        let cli = Cli::try_parse_from(["omen", "debt"]).unwrap();
        assert!(matches!(cli.command, Command::Satd(_)));
    }

    #[test]
    fn test_alias_dc_for_deadcode() {
        let cli = Cli::try_parse_from(["omen", "dc"]).unwrap();
        assert!(matches!(cli.command, Command::Deadcode(_)));
    }

    #[test]
    fn test_alias_dup_for_clones() {
        let cli = Cli::try_parse_from(["omen", "dup"]).unwrap();
        assert!(matches!(cli.command, Command::Clones(_)));
    }

    #[test]
    fn test_alias_duplicates_for_clones() {
        let cli = Cli::try_parse_from(["omen", "duplicates"]).unwrap();
        assert!(matches!(cli.command, Command::Clones(_)));
    }

    #[test]
    fn test_alias_predict_for_defect() {
        let cli = Cli::try_parse_from(["omen", "predict"]).unwrap();
        assert!(matches!(cli.command, Command::Defect(_)));
    }

    #[test]
    fn test_alias_jit_for_changes() {
        let cli = Cli::try_parse_from(["omen", "jit"]).unwrap();
        assert!(matches!(cli.command, Command::Changes(_)));
    }

    #[test]
    fn test_alias_pr_for_diff() {
        let cli = Cli::try_parse_from(["omen", "pr", "--base", "main"]).unwrap();
        assert!(matches!(cli.command, Command::Diff(_)));
    }

    #[test]
    fn test_alias_dag_for_graph() {
        let cli = Cli::try_parse_from(["omen", "dag"]).unwrap();
        assert!(matches!(cli.command, Command::Graph(_)));
    }

    #[test]
    fn test_alias_hs_for_hotspot() {
        let cli = Cli::try_parse_from(["omen", "hs"]).unwrap();
        assert!(matches!(cli.command, Command::Hotspot(_)));
    }

    #[test]
    fn test_alias_tc_for_temporal() {
        let cli = Cli::try_parse_from(["omen", "tc"]).unwrap();
        assert!(matches!(cli.command, Command::Temporal(_)));
    }

    #[test]
    fn test_alias_temporal_coupling_for_temporal() {
        let cli = Cli::try_parse_from(["omen", "temporal-coupling"]).unwrap();
        assert!(matches!(cli.command, Command::Temporal(_)));
    }

    #[test]
    fn test_alias_own_for_ownership() {
        let cli = Cli::try_parse_from(["omen", "own"]).unwrap();
        assert!(matches!(cli.command, Command::Ownership(_)));
    }

    #[test]
    fn test_alias_bus_factor_for_ownership() {
        let cli = Cli::try_parse_from(["omen", "bus-factor"]).unwrap();
        assert!(matches!(cli.command, Command::Ownership(_)));
    }

    #[test]
    fn test_alias_ck_for_cohesion() {
        let cli = Cli::try_parse_from(["omen", "ck"]).unwrap();
        assert!(matches!(cli.command, Command::Cohesion(_)));
    }

    #[test]
    fn test_alias_ff_for_flags() {
        let cli = Cli::try_parse_from(["omen", "ff"]).unwrap();
        assert!(matches!(cli.command, Command::Flags(_)));
    }

    // Context command tests
    #[test]
    fn test_command_context() {
        let cli = Cli::try_parse_from(["omen", "context"]).unwrap();
        assert!(matches!(cli.command, Command::Context(_)));
    }

    #[test]
    fn test_alias_ctx_for_context() {
        let cli = Cli::try_parse_from(["omen", "ctx"]).unwrap();
        assert!(matches!(cli.command, Command::Context(_)));
    }

    #[test]
    fn test_context_target() {
        let cli = Cli::try_parse_from(["omen", "context", "--target", "src/main.rs"]).unwrap();
        if let Command::Context(args) = cli.command {
            assert_eq!(args.target, Some(PathBuf::from("src/main.rs")));
        }
    }

    #[test]
    fn test_context_max_tokens() {
        let cli = Cli::try_parse_from(["omen", "context", "--max-tokens", "4000"]).unwrap();
        if let Command::Context(args) = cli.command {
            assert_eq!(args.max_tokens, 4000);
        }
    }

    #[test]
    fn test_context_symbol() {
        let cli = Cli::try_parse_from(["omen", "context", "--symbol", "main"]).unwrap();
        if let Command::Context(args) = cli.command {
            assert_eq!(args.symbol, Some("main".to_string()));
        }
    }

    #[test]
    fn test_context_depth() {
        let cli = Cli::try_parse_from(["omen", "context", "--depth", "3"]).unwrap();
        if let Command::Context(args) = cli.command {
            assert_eq!(args.depth, 3);
        }
    }

    // Report command tests
    #[test]
    fn test_command_report_generate() {
        let cli = Cli::try_parse_from(["omen", "report", "generate"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, ReportSubcommand::Generate(_)));
        }
    }

    #[test]
    fn test_command_report_validate() {
        let cli = Cli::try_parse_from(["omen", "report", "validate"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, ReportSubcommand::Validate(_)));
        }
    }

    #[test]
    fn test_command_report_render() {
        let cli = Cli::try_parse_from(["omen", "report", "render"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, ReportSubcommand::Render(_)));
        }
    }

    #[test]
    fn test_command_report_serve() {
        let cli = Cli::try_parse_from(["omen", "report", "serve"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, ReportSubcommand::Serve(_)));
        }
    }

    #[test]
    fn test_report_generate_output() {
        let cli = Cli::try_parse_from(["omen", "report", "generate", "-o", "/tmp/data"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            if let ReportSubcommand::Generate(args) = cmd.subcommand {
                assert_eq!(args.output, PathBuf::from("/tmp/data"));
            }
        }
    }

    #[test]
    fn test_report_generate_skip() {
        let cli = Cli::try_parse_from(["omen", "report", "generate", "--skip", "complexity,satd"])
            .unwrap();
        if let Command::Report(cmd) = cli.command {
            if let ReportSubcommand::Generate(args) = cmd.subcommand {
                assert_eq!(args.skip, Some("complexity,satd".to_string()));
            }
        }
    }

    #[test]
    fn test_report_validate_data() {
        let cli = Cli::try_parse_from(["omen", "report", "validate", "-d", "/tmp/data"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            if let ReportSubcommand::Validate(args) = cmd.subcommand {
                assert_eq!(args.data, PathBuf::from("/tmp/data"));
            }
        }
    }

    #[test]
    fn test_report_render_output() {
        let cli =
            Cli::try_parse_from(["omen", "report", "render", "-o", "/tmp/report.html"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            if let ReportSubcommand::Render(args) = cmd.subcommand {
                assert_eq!(args.output, PathBuf::from("/tmp/report.html"));
            }
        }
    }

    #[test]
    fn test_report_serve_port() {
        let cli = Cli::try_parse_from(["omen", "report", "serve", "-p", "3000"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            if let ReportSubcommand::Serve(args) = cmd.subcommand {
                assert_eq!(args.port, 3000);
            }
        }
    }

    #[test]
    fn test_report_serve_host() {
        let cli = Cli::try_parse_from(["omen", "report", "serve", "--host", "0.0.0.0"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            if let ReportSubcommand::Serve(args) = cmd.subcommand {
                assert_eq!(args.host, "0.0.0.0");
            }
        }
    }

    #[test]
    fn test_report_generate_since() {
        let cli = Cli::try_parse_from(["omen", "report", "generate", "--since", "6m"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            if let ReportSubcommand::Generate(args) = cmd.subcommand {
                assert_eq!(args.since, "6m");
            }
        }
    }

    #[test]
    fn test_report_generate_days() {
        let cli = Cli::try_parse_from(["omen", "report", "generate", "--days", "90"]).unwrap();
        if let Command::Report(cmd) = cli.command {
            if let ReportSubcommand::Generate(args) = cmd.subcommand {
                assert_eq!(args.days, Some(90));
            }
        }
    }

    // New command tests
    #[test]
    fn test_command_lint_hotspot() {
        let cli = Cli::try_parse_from(["omen", "lint-hotspot"]).unwrap();
        assert!(matches!(cli.command, Command::LintHotspot(_)));
    }

    #[test]
    fn test_alias_lh_for_lint_hotspot() {
        let cli = Cli::try_parse_from(["omen", "lh"]).unwrap();
        assert!(matches!(cli.command, Command::LintHotspot(_)));
    }

    #[test]
    fn test_lint_hotspot_top() {
        let cli = Cli::try_parse_from(["omen", "lint-hotspot", "--top", "20"]).unwrap();
        if let Command::LintHotspot(args) = cli.command {
            assert_eq!(args.top, 20);
        }
    }

    #[test]
    fn test_score_trend() {
        let cli = Cli::try_parse_from(["omen", "score", "trend"]).unwrap();
        if let Command::Score(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, Some(ScoreSubcommand::Trend(_))));
        }
    }

    #[test]
    fn test_score_trend_alias() {
        let cli = Cli::try_parse_from(["omen", "score", "tr"]).unwrap();
        if let Command::Score(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, Some(ScoreSubcommand::Trend(_))));
        }
    }

    #[test]
    fn test_score_trend_since() {
        let cli = Cli::try_parse_from(["omen", "score", "trend", "--since", "6m"]).unwrap();
        if let Command::Score(cmd) = cli.command {
            if let Some(ScoreSubcommand::Trend(args)) = cmd.subcommand {
                assert_eq!(args.since, "6m");
            }
        }
    }

    #[test]
    fn test_score_trend_period() {
        let cli = Cli::try_parse_from(["omen", "score", "trend", "--period", "monthly"]).unwrap();
        if let Command::Score(cmd) = cli.command {
            if let Some(ScoreSubcommand::Trend(args)) = cmd.subcommand {
                assert!(matches!(args.period, TrendPeriod::Monthly));
            }
        }
    }

    #[test]
    fn test_score_trend_snap() {
        let cli = Cli::try_parse_from(["omen", "score", "trend", "--snap"]).unwrap();
        if let Command::Score(cmd) = cli.command {
            if let Some(ScoreSubcommand::Trend(args)) = cmd.subcommand {
                assert!(args.snap);
            }
        }
    }

    // Global flag tests
    #[test]
    fn test_no_cache_flag() {
        let cli = Cli::try_parse_from(["omen", "--no-cache", "complexity"]).unwrap();
        assert!(cli.no_cache);
    }

    #[test]
    fn test_git_ref_flag() {
        let cli = Cli::try_parse_from(["omen", "--ref", "v1.0.0", "complexity"]).unwrap();
        assert_eq!(cli.git_ref, Some("v1.0.0".to_string()));
    }

    #[test]
    fn test_shallow_flag() {
        let cli = Cli::try_parse_from(["omen", "--shallow", "complexity"]).unwrap();
        assert!(cli.shallow);
    }

    // Search command tests
    #[test]
    fn test_command_search_index() {
        let cli = Cli::try_parse_from(["omen", "search", "index"]).unwrap();
        if let Command::Search(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, SearchSubcommand::Index(_)));
        }
    }

    #[test]
    fn test_alias_s_for_search() {
        let cli = Cli::try_parse_from(["omen", "s", "index"]).unwrap();
        assert!(matches!(cli.command, Command::Search(_)));
    }

    #[test]
    fn test_search_index_force() {
        let cli = Cli::try_parse_from(["omen", "search", "index", "--force"]).unwrap();
        if let Command::Search(cmd) = cli.command {
            if let SearchSubcommand::Index(args) = cmd.subcommand {
                assert!(args.force);
            }
        }
    }

    #[test]
    fn test_command_search_query() {
        let cli = Cli::try_parse_from(["omen", "search", "query", "function that handles errors"])
            .unwrap();
        if let Command::Search(cmd) = cli.command {
            if let SearchSubcommand::Query(args) = cmd.subcommand {
                assert_eq!(args.query, "function that handles errors");
            }
        }
    }

    #[test]
    fn test_search_query_top_k() {
        let cli = Cli::try_parse_from(["omen", "search", "query", "test", "-k", "20"]).unwrap();
        if let Command::Search(cmd) = cli.command {
            if let SearchSubcommand::Query(args) = cmd.subcommand {
                assert_eq!(args.top_k, 20);
            }
        }
    }

    #[test]
    fn test_search_query_min_score() {
        let cli =
            Cli::try_parse_from(["omen", "search", "query", "test", "--min-score", "0.5"]).unwrap();
        if let Command::Search(cmd) = cli.command {
            if let SearchSubcommand::Query(args) = cmd.subcommand {
                assert!((args.min_score - 0.5).abs() < 0.001);
            }
        }
    }

    #[test]
    fn test_search_query_files() {
        let cli = Cli::try_parse_from([
            "omen",
            "search",
            "query",
            "test",
            "--files",
            "src/main.rs,src/lib.rs",
        ])
        .unwrap();
        if let Command::Search(cmd) = cli.command {
            if let SearchSubcommand::Query(args) = cmd.subcommand {
                assert_eq!(args.files, Some("src/main.rs,src/lib.rs".to_string()));
            }
        }
    }
}
