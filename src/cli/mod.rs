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
    #[arg(short, long, value_enum, default_value = "json")]
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

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Analyze code complexity (cyclomatic and cognitive)
    Complexity(AnalyzerArgs),

    /// Detect Self-Admitted Technical Debt
    Satd(AnalyzerArgs),

    /// Find dead/unreachable code
    Deadcode(AnalyzerArgs),

    /// Analyze code churn from git history
    Churn(ChurnArgs),

    /// Detect code duplicates/clones
    Clones(ClonesArgs),

    /// Predict defect-prone files using PMAT
    Defect(DefectArgs),

    /// Analyze recent changes (JIT risk)
    Changes(ChangesArgs),

    /// Analyze a specific diff
    Diff(DiffArgs),

    /// Generate Technical Debt Graph
    Tdg(AnalyzerArgs),

    /// Analyze dependency graph structure
    Graph(AnalyzerArgs),

    /// Find complexity/churn hotspots
    Hotspot(HotspotArgs),

    /// Detect temporally coupled files
    Temporal(TemporalArgs),

    /// Analyze code ownership and bus factor
    Ownership(OwnershipArgs),

    /// Calculate CK cohesion metrics
    Cohesion(AnalyzerArgs),

    /// Generate PageRank-ranked symbol map
    Repomap(RepomapArgs),

    /// Detect architectural smells
    Smells(AnalyzerArgs),

    /// Find and assess feature flags
    Flags(FlagsArgs),

    /// Calculate composite health score
    Score(ScoreArgs),

    /// Start MCP server for LLM integration
    Mcp(McpArgs),

    /// Run all analyzers
    All(AllArgs),
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
    #[arg(long, default_value = "365")]
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
    #[arg(long, default_value = "365")]
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
    #[arg(long, default_value = "365")]
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
    #[arg(long, default_value = "365")]
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
pub struct ScoreArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Analyzers to include (comma-separated)
    #[arg(long)]
    pub analyzers: Option<String>,

    /// Custom weights file
    #[arg(long)]
    pub weights: Option<PathBuf>,
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
    #[arg(long, default_value = "365")]
    pub days: u32,
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
            assert_eq!(args.days, 365);
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
        if let Command::Mcp(args) = cli.command {
            assert!(matches!(args.transport, McpTransport::Stdio));
        }
    }

    #[test]
    fn test_mcp_transport_sse() {
        let cli = Cli::try_parse_from(["omen", "mcp", "--transport", "sse"]).unwrap();
        if let Command::Mcp(args) = cli.command {
            assert!(matches!(args.transport, McpTransport::Sse));
        }
    }

    #[test]
    fn test_mcp_port() {
        let cli = Cli::try_parse_from(["omen", "mcp", "--port", "8080"]).unwrap();
        if let Command::Mcp(args) = cli.command {
            assert_eq!(args.port, 8080);
        }
    }

    #[test]
    fn test_mcp_host() {
        let cli = Cli::try_parse_from(["omen", "mcp", "--host", "0.0.0.0"]).unwrap();
        if let Command::Mcp(args) = cli.command {
            assert_eq!(args.host, "0.0.0.0");
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
        assert!(matches!(cli.format, OutputFormat::Json));
    }
}
