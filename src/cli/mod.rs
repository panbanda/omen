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
    Complexity(ComplexityArgs),

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
    Clones(AnalyzerArgs),

    /// Predict defect-prone files using PMAT
    #[command(alias = "predict")]
    Defect(AnalyzerArgs),

    /// Analyze recent changes (JIT risk)
    #[command(alias = "jit")]
    Changes(AnalyzerArgs),

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
    Hotspot(AnalyzerArgs),

    /// Detect temporally coupled files
    #[command(alias = "tc", visible_alias = "temporal-coupling")]
    Temporal(AnalyzerArgs),

    /// Analyze code ownership and bus factor
    #[command(alias = "own", alias = "bus-factor")]
    Ownership(AnalyzerArgs),

    /// Calculate CK cohesion metrics
    #[command(alias = "ck")]
    Cohesion(AnalyzerArgs),

    /// Generate PageRank-ranked symbol map
    Repomap(AnalyzerArgs),

    /// Detect architectural smells
    Smells(AnalyzerArgs),

    /// Find and assess feature flags
    #[command(alias = "ff")]
    Flags(FlagsArgs),

    /// Calculate composite health score
    Score(ScoreCommand),

    /// Start MCP server for LLM integration
    Mcp(McpCommand),

    /// Run all analyzers
    All(AnalyzerArgs),

    /// Generate deep context for LLM consumption
    #[command(alias = "ctx")]
    Context(ContextArgs),

    /// Generate and manage HTML health reports
    Report(ReportCommand),

    /// Semantic search over code symbols
    #[command(alias = "s")]
    Search(SearchCommand),

    /// Mutation testing for test suite effectiveness
    #[command(alias = "mut")]
    Mutation(Box<MutationCommand>),
}

#[derive(Args)]
pub struct AnalyzerArgs {
    /// Filter by file glob pattern
    #[arg(short, long)]
    pub glob: Option<String>,

    /// Exclude files matching pattern
    #[arg(short, long)]
    pub exclude: Option<String>,
}

#[derive(Args)]
pub struct DiffArgs {
    /// Target branch to diff against (default: auto-detect main/master)
    #[arg(short, long)]
    pub target: Option<String>,
}

#[derive(Args)]
pub struct ComplexityArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Check mode: fail if any function exceeds thresholds
    #[arg(long)]
    pub check: bool,

    /// Maximum cyclomatic complexity (default: from config or 20)
    #[arg(long)]
    pub max_cyclomatic: Option<u32>,

    /// Maximum cognitive complexity (default: from config or 30)
    #[arg(long)]
    pub max_cognitive: Option<u32>,
}

#[derive(Args)]
pub struct ChurnArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Number of days to analyze
    #[arg(long, default_value = "30")]
    pub days: u32,
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
    /// Check mode: fail if score is below threshold
    #[arg(long)]
    pub check: bool,

    /// Minimum score to pass (default: from config)
    #[arg(long)]
    pub fail_under: Option<f64>,
}

#[derive(Args)]
pub struct ScoreTrendArgs {
    /// Time period (e.g., 3m, 6m, 1y, all)
    #[arg(short, long, default_value = "all")]
    pub since: String,

    /// Aggregation period
    #[arg(short, long, value_enum, default_value = "weekly")]
    pub period: TrendPeriod,

    /// Number of samples (evenly spaced over the time range; overrides --period)
    #[arg(long)]
    pub samples: Option<usize>,
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

/// Context generation for LLM consumption.
#[derive(Args)]
pub struct ContextArgs {
    /// Target file or directory for context
    #[arg(long)]
    pub target: Option<PathBuf>,

    /// Maximum context tokens to generate
    #[arg(long, default_value = "8000")]
    pub max_tokens: usize,

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

    /// Number of samples for trend analysis (evenly spaced over the time range)
    #[arg(long)]
    pub samples: Option<usize>,
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

#[derive(Clone, Subcommand)]
pub enum SearchSubcommand {
    /// Build or update the search index
    Index(SearchIndexArgs),

    /// Search for code symbols
    Query(SearchQueryArgs),
}

#[derive(Clone, Args)]
pub struct SearchIndexArgs {
    /// Force full re-index (ignore cache)
    #[arg(long)]
    pub force: bool,
}

#[derive(Clone, Args)]
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

    /// Include additional project paths for cross-repo search (comma-separated)
    #[arg(long)]
    pub include_project: Option<String>,
}

#[derive(Args)]
pub struct MutationArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Test command to run (auto-detected if omitted)
    #[arg(long)]
    pub test_command: Option<String>,

    /// Timeout per mutant in seconds
    #[arg(long, default_value = "30")]
    pub timeout: u64,

    /// Mutation operators to use (comma-separated: CRR,ROR,AOR)
    #[arg(long, default_value = "CRR,ROR,AOR")]
    pub operators: String,

    /// Check mode: fail if mutation score below threshold
    #[arg(long)]
    pub check: bool,

    /// Minimum mutation score (0.0-1.0)
    #[arg(long, default_value = "0.8")]
    pub min_score: f64,

    /// Generate mutants only, don't execute tests
    #[arg(long)]
    pub dry_run: bool,

    /// Number of parallel workers (0 = num_cpus)
    #[arg(long, default_value = "0")]
    pub jobs: usize,

    /// Path to coverage JSON file
    #[arg(long)]
    pub coverage: Option<PathBuf>,

    /// Only test mutants in changed files (incremental mode)
    #[arg(long)]
    pub incremental: bool,

    /// Skip likely-equivalent mutants
    #[arg(long)]
    pub skip_equivalent: bool,

    /// Mutation mode: all, fast, thorough
    #[arg(long, value_enum, default_value = "all")]
    pub mode: MutationMode,

    /// Write surviving mutants to file
    #[arg(long)]
    pub output_survivors: Option<PathBuf>,

    /// Record results to history file for model training
    #[arg(long)]
    pub record: bool,

    /// Path to ML model file (default: .omen/mutation-model.json)
    #[arg(long)]
    pub model: Option<PathBuf>,

    /// Skip mutants predicted to be killed above this threshold (0.0-1.0)
    #[arg(long, value_name = "THRESHOLD")]
    pub skip_predicted: Option<f64>,
}

/// Mutation testing mode.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum MutationMode {
    /// All operators, all mutants
    All,
    /// Skip equivalent, limit per-location
    Fast,
    /// All + language-specific operators
    Thorough,
}

/// Mutation testing command with subcommands.
#[derive(Args)]
pub struct MutationCommand {
    #[command(subcommand)]
    pub subcommand: Option<MutationSubcommand>,

    #[command(flatten)]
    pub args: MutationArgs,
}

#[derive(Subcommand)]
pub enum MutationSubcommand {
    /// Train ML predictor from historical mutation results
    Train(MutationTrainArgs),
}

/// Arguments for mutation train command.
#[derive(Args)]
pub struct MutationTrainArgs {
    /// Path to analyze (default: current directory)
    #[arg(short, long, default_value = ".")]
    pub path: PathBuf,

    /// Path to history file (default: .omen/mutation-history.jsonl)
    #[arg(long)]
    pub history: Option<PathBuf>,

    /// Path to output model file (default: .omen/mutation-model.json)
    #[arg(long)]
    pub model: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Json,
    Markdown,
    Text,
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

    /// Parse CLI args and return the parsed Cli, panicking on failure.
    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).unwrap()
    }

    /// Asserts that the given CLI input parses to the expected Command variant.
    macro_rules! assert_parses_to {
        ($args:expr, $variant:pat) => {
            let cli = parse($args);
            assert!(matches!(cli.command, $variant));
        };
    }

    /// Extract MutationArgs from a parsed CLI, panicking if the command is wrong.
    fn parse_mutation_args(args: &[&str]) -> MutationArgs {
        let cli = parse(args);
        match cli.command {
            Command::Mutation(cmd) => cmd.args,
            _ => panic!("Expected Mutation command"),
        }
    }

    /// Extract MutationCommand from a parsed CLI, panicking if the command is wrong.
    fn parse_mutation_command(args: &[&str]) -> Box<MutationCommand> {
        let cli = parse(args);
        match cli.command {
            Command::Mutation(cmd) => cmd,
            _ => panic!("Expected Mutation command"),
        }
    }

    /// Extract ComplexityArgs from a parsed CLI, panicking if the command is wrong.
    fn parse_complexity_args(args: &[&str]) -> ComplexityArgs {
        let cli = parse(args);
        match cli.command {
            Command::Complexity(args) => args,
            _ => panic!("Expected Complexity command"),
        }
    }

    /// Extract ReportSubcommand from a parsed CLI, panicking if the command is wrong.
    fn parse_report_subcommand(args: &[&str]) -> ReportSubcommand {
        let cli = parse(args);
        match cli.command {
            Command::Report(cmd) => cmd.subcommand,
            _ => panic!("Expected Report command"),
        }
    }

    /// Extract SearchSubcommand from a parsed CLI, panicking if the command is wrong.
    fn parse_search_subcommand(args: &[&str]) -> SearchSubcommand {
        let cli = parse(args);
        match cli.command {
            Command::Search(cmd) => cmd.subcommand,
            _ => panic!("Expected Search command"),
        }
    }

    #[test]
    fn test_cli_verify() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_cli_default_path() {
        let cli = parse(&["omen", "complexity"]);
        assert_eq!(cli.path, std::path::PathBuf::from("."));
    }

    #[test]
    fn test_cli_custom_path() {
        let cli = parse(&["omen", "-p", "/tmp", "complexity"]);
        assert_eq!(cli.path, std::path::PathBuf::from("/tmp"));
    }

    #[test]
    fn test_cli_format_json() {
        assert!(matches!(
            parse(&["omen", "-f", "json", "complexity"]).format,
            OutputFormat::Json
        ));
    }

    #[test]
    fn test_cli_format_markdown() {
        assert!(matches!(
            parse(&["omen", "-f", "markdown", "complexity"]).format,
            OutputFormat::Markdown
        ));
    }

    #[test]
    fn test_cli_format_text() {
        assert!(matches!(
            parse(&["omen", "-f", "text", "complexity"]).format,
            OutputFormat::Text
        ));
    }

    #[test]
    fn test_cli_config_flag() {
        let cli = parse(&["omen", "-c", "config.toml", "complexity"]);
        assert_eq!(cli.config, Some(std::path::PathBuf::from("config.toml")));
    }

    #[test]
    fn test_cli_verbose_flag() {
        assert!(parse(&["omen", "-v", "complexity"]).verbose);
    }

    #[test]
    fn test_cli_jobs_flag() {
        assert_eq!(parse(&["omen", "-j", "4", "complexity"]).jobs, Some(4));
    }

    // Command recognition tests
    #[test]
    fn test_command_complexity() {
        assert_parses_to!(&["omen", "complexity"], Command::Complexity(_));
    }

    #[test]
    fn test_command_satd() {
        assert_parses_to!(&["omen", "satd"], Command::Satd(_));
    }

    #[test]
    fn test_command_deadcode() {
        assert_parses_to!(&["omen", "deadcode"], Command::Deadcode(_));
    }

    #[test]
    fn test_command_churn() {
        assert_parses_to!(&["omen", "churn"], Command::Churn(_));
    }

    #[test]
    fn test_command_clones() {
        assert_parses_to!(&["omen", "clones"], Command::Clones(_));
    }

    #[test]
    fn test_command_defect() {
        assert_parses_to!(&["omen", "defect"], Command::Defect(_));
    }

    #[test]
    fn test_command_changes() {
        assert_parses_to!(&["omen", "changes"], Command::Changes(_));
    }

    #[test]
    fn test_command_diff() {
        assert_parses_to!(&["omen", "diff"], Command::Diff(_));
    }

    #[test]
    fn test_diff_target_flag() {
        let cli = parse(&["omen", "diff", "--target", "develop"]);
        match cli.command {
            Command::Diff(args) => assert_eq!(args.target, Some("develop".to_string())),
            _ => panic!("expected Diff command"),
        }
    }

    #[test]
    fn test_diff_target_short_flag() {
        let cli = parse(&["omen", "diff", "-t", "release/v2"]);
        match cli.command {
            Command::Diff(args) => assert_eq!(args.target, Some("release/v2".to_string())),
            _ => panic!("expected Diff command"),
        }
    }

    #[test]
    fn test_command_tdg() {
        assert_parses_to!(&["omen", "tdg"], Command::Tdg(_));
    }

    #[test]
    fn test_command_graph() {
        assert_parses_to!(&["omen", "graph"], Command::Graph(_));
    }

    #[test]
    fn test_command_hotspot() {
        assert_parses_to!(&["omen", "hotspot"], Command::Hotspot(_));
    }

    #[test]
    fn test_command_temporal() {
        assert_parses_to!(&["omen", "temporal"], Command::Temporal(_));
    }

    #[test]
    fn test_command_ownership() {
        assert_parses_to!(&["omen", "ownership"], Command::Ownership(_));
    }

    #[test]
    fn test_command_cohesion() {
        assert_parses_to!(&["omen", "cohesion"], Command::Cohesion(_));
    }

    #[test]
    fn test_command_repomap() {
        assert_parses_to!(&["omen", "repomap"], Command::Repomap(_));
    }

    #[test]
    fn test_command_smells() {
        assert_parses_to!(&["omen", "smells"], Command::Smells(_));
    }

    #[test]
    fn test_command_flags() {
        assert_parses_to!(&["omen", "flags"], Command::Flags(_));
    }

    #[test]
    fn test_command_score() {
        assert_parses_to!(&["omen", "score"], Command::Score(_));
    }

    #[test]
    fn test_command_mcp() {
        assert_parses_to!(&["omen", "mcp"], Command::Mcp(_));
    }

    #[test]
    fn test_command_all() {
        assert_parses_to!(&["omen", "all"], Command::All(_));
    }

    #[test]
    fn test_command_context() {
        assert_parses_to!(&["omen", "context"], Command::Context(_));
    }

    #[test]
    fn test_command_mutation() {
        assert_parses_to!(&["omen", "mutation"], Command::Mutation(_));
    }

    // Alias tests
    #[test]
    fn test_alias_cx_for_complexity() {
        assert_parses_to!(&["omen", "cx"], Command::Complexity(_));
    }

    #[test]
    fn test_alias_debt_for_satd() {
        assert_parses_to!(&["omen", "debt"], Command::Satd(_));
    }

    #[test]
    fn test_alias_dc_for_deadcode() {
        assert_parses_to!(&["omen", "dc"], Command::Deadcode(_));
    }

    #[test]
    fn test_alias_dup_for_clones() {
        assert_parses_to!(&["omen", "dup"], Command::Clones(_));
    }

    #[test]
    fn test_alias_duplicates_for_clones() {
        assert_parses_to!(&["omen", "duplicates"], Command::Clones(_));
    }

    #[test]
    fn test_alias_predict_for_defect() {
        assert_parses_to!(&["omen", "predict"], Command::Defect(_));
    }

    #[test]
    fn test_alias_jit_for_changes() {
        assert_parses_to!(&["omen", "jit"], Command::Changes(_));
    }

    #[test]
    fn test_alias_pr_for_diff() {
        assert_parses_to!(&["omen", "pr"], Command::Diff(_));
    }

    #[test]
    fn test_alias_dag_for_graph() {
        assert_parses_to!(&["omen", "dag"], Command::Graph(_));
    }

    #[test]
    fn test_alias_hs_for_hotspot() {
        assert_parses_to!(&["omen", "hs"], Command::Hotspot(_));
    }

    #[test]
    fn test_alias_tc_for_temporal() {
        assert_parses_to!(&["omen", "tc"], Command::Temporal(_));
    }

    #[test]
    fn test_alias_temporal_coupling_for_temporal() {
        assert_parses_to!(&["omen", "temporal-coupling"], Command::Temporal(_));
    }

    #[test]
    fn test_alias_own_for_ownership() {
        assert_parses_to!(&["omen", "own"], Command::Ownership(_));
    }

    #[test]
    fn test_alias_bus_factor_for_ownership() {
        assert_parses_to!(&["omen", "bus-factor"], Command::Ownership(_));
    }

    #[test]
    fn test_alias_ck_for_cohesion() {
        assert_parses_to!(&["omen", "ck"], Command::Cohesion(_));
    }

    #[test]
    fn test_alias_ff_for_flags() {
        assert_parses_to!(&["omen", "ff"], Command::Flags(_));
    }

    #[test]
    fn test_alias_ctx_for_context() {
        assert_parses_to!(&["omen", "ctx"], Command::Context(_));
    }

    #[test]
    fn test_alias_mut_for_mutation() {
        assert_parses_to!(&["omen", "mut"], Command::Mutation(_));
    }

    #[test]
    fn test_alias_s_for_search() {
        assert_parses_to!(&["omen", "s", "index"], Command::Search(_));
    }

    // Analyzer-specific option tests

    #[test]
    fn test_churn_days_default() {
        let cli = parse(&["omen", "churn"]);
        if let Command::Churn(args) = cli.command {
            assert_eq!(args.days, 30);
        }
    }

    #[test]
    fn test_churn_days_custom() {
        let cli = parse(&["omen", "churn", "--days", "30"]);
        if let Command::Churn(args) = cli.command {
            assert_eq!(args.days, 30);
        }
    }

    #[test]
    fn test_flags_provider() {
        let cli = parse(&["omen", "flags", "--provider", "launchdarkly"]);
        if let Command::Flags(args) = cli.command {
            assert_eq!(args.provider, Some("launchdarkly".to_string()));
        }
    }

    #[test]
    fn test_flags_stale_days() {
        let cli = parse(&["omen", "flags", "--stale-days", "60"]);
        if let Command::Flags(args) = cli.command {
            assert_eq!(args.stale_days, 60);
        }
    }

    #[test]
    fn test_mcp_transport_stdio() {
        let cli = parse(&["omen", "mcp", "--transport", "stdio"]);
        if let Command::Mcp(cmd) = cli.command {
            assert!(matches!(cmd.args.transport, McpTransport::Stdio));
        }
    }

    #[test]
    fn test_mcp_transport_sse() {
        let cli = parse(&["omen", "mcp", "--transport", "sse"]);
        if let Command::Mcp(cmd) = cli.command {
            assert!(matches!(cmd.args.transport, McpTransport::Sse));
        }
    }

    #[test]
    fn test_mcp_port() {
        let cli = parse(&["omen", "mcp", "--port", "8080"]);
        if let Command::Mcp(cmd) = cli.command {
            assert_eq!(cmd.args.port, 8080);
        }
    }

    #[test]
    fn test_mcp_host() {
        let cli = parse(&["omen", "mcp", "--host", "0.0.0.0"]);
        if let Command::Mcp(cmd) = cli.command {
            assert_eq!(cmd.args.host, "0.0.0.0");
        }
    }

    #[test]
    fn test_mcp_manifest() {
        let cli = parse(&["omen", "mcp", "manifest"]);
        if let Command::Mcp(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, Some(McpSubcommand::Manifest)));
        }
    }

    // Common AnalyzerArgs tests (via complexity)

    #[test]
    fn test_analyzer_args_glob() {
        let args = parse_complexity_args(&["omen", "complexity", "-g", "*.rs"]);
        assert_eq!(args.common.glob, Some("*.rs".to_string()));
    }

    #[test]
    fn test_analyzer_args_exclude() {
        let args = parse_complexity_args(&["omen", "complexity", "-e", "test"]);
        assert_eq!(args.common.exclude, Some("test".to_string()));
    }

    #[test]
    fn test_output_format_default() {
        assert!(matches!(
            parse(&["omen", "complexity"]).format,
            OutputFormat::Markdown
        ));
    }

    // Context option tests

    #[test]
    fn test_context_target() {
        let cli = parse(&["omen", "context", "--target", "src/main.rs"]);
        if let Command::Context(args) = cli.command {
            assert_eq!(args.target, Some(PathBuf::from("src/main.rs")));
        }
    }

    #[test]
    fn test_context_max_tokens() {
        let cli = parse(&["omen", "context", "--max-tokens", "4000"]);
        if let Command::Context(args) = cli.command {
            assert_eq!(args.max_tokens, 4000);
        }
    }

    #[test]
    fn test_context_symbol() {
        let cli = parse(&["omen", "context", "--symbol", "main"]);
        if let Command::Context(args) = cli.command {
            assert_eq!(args.symbol, Some("main".to_string()));
        }
    }

    #[test]
    fn test_context_depth() {
        let cli = parse(&["omen", "context", "--depth", "3"]);
        if let Command::Context(args) = cli.command {
            assert_eq!(args.depth, 3);
        }
    }

    // Report command tests

    #[test]
    fn test_command_report_generate() {
        assert!(matches!(
            parse_report_subcommand(&["omen", "report", "generate"]),
            ReportSubcommand::Generate(_)
        ));
    }

    #[test]
    fn test_command_report_validate() {
        assert!(matches!(
            parse_report_subcommand(&["omen", "report", "validate"]),
            ReportSubcommand::Validate(_)
        ));
    }

    #[test]
    fn test_command_report_render() {
        assert!(matches!(
            parse_report_subcommand(&["omen", "report", "render"]),
            ReportSubcommand::Render(_)
        ));
    }

    #[test]
    fn test_command_report_serve() {
        assert!(matches!(
            parse_report_subcommand(&["omen", "report", "serve"]),
            ReportSubcommand::Serve(_)
        ));
    }

    #[test]
    fn test_report_generate_output() {
        if let ReportSubcommand::Generate(args) =
            parse_report_subcommand(&["omen", "report", "generate", "-o", "/tmp/data"])
        {
            assert_eq!(args.output, PathBuf::from("/tmp/data"));
        }
    }

    #[test]
    fn test_report_generate_skip() {
        if let ReportSubcommand::Generate(args) =
            parse_report_subcommand(&["omen", "report", "generate", "--skip", "complexity,satd"])
        {
            assert_eq!(args.skip, Some("complexity,satd".to_string()));
        }
    }

    #[test]
    fn test_report_validate_data() {
        if let ReportSubcommand::Validate(args) =
            parse_report_subcommand(&["omen", "report", "validate", "-d", "/tmp/data"])
        {
            assert_eq!(args.data, PathBuf::from("/tmp/data"));
        }
    }

    #[test]
    fn test_report_render_output() {
        if let ReportSubcommand::Render(args) =
            parse_report_subcommand(&["omen", "report", "render", "-o", "/tmp/report.html"])
        {
            assert_eq!(args.output, PathBuf::from("/tmp/report.html"));
        }
    }

    #[test]
    fn test_report_serve_port() {
        if let ReportSubcommand::Serve(args) =
            parse_report_subcommand(&["omen", "report", "serve", "-p", "3000"])
        {
            assert_eq!(args.port, 3000);
        }
    }

    #[test]
    fn test_report_serve_host() {
        if let ReportSubcommand::Serve(args) =
            parse_report_subcommand(&["omen", "report", "serve", "--host", "0.0.0.0"])
        {
            assert_eq!(args.host, "0.0.0.0");
        }
    }

    #[test]
    fn test_report_generate_since() {
        if let ReportSubcommand::Generate(args) =
            parse_report_subcommand(&["omen", "report", "generate", "--since", "6m"])
        {
            assert_eq!(args.since, "6m");
        }
    }

    #[test]
    fn test_report_generate_days() {
        if let ReportSubcommand::Generate(args) =
            parse_report_subcommand(&["omen", "report", "generate", "--days", "90"])
        {
            assert_eq!(args.days, Some(90));
        }
    }

    #[test]
    fn test_report_generate_default_since_is_one_year() {
        if let ReportSubcommand::Generate(args) =
            parse_report_subcommand(&["omen", "report", "generate"])
        {
            assert_eq!(args.since, "1y");
        }
    }

    #[test]
    fn test_report_generate_samples() {
        if let ReportSubcommand::Generate(args) =
            parse_report_subcommand(&["omen", "report", "generate", "--samples", "24"])
        {
            assert_eq!(args.samples, Some(24));
        }
    }

    #[test]
    fn test_report_generate_samples_default_is_none() {
        if let ReportSubcommand::Generate(args) =
            parse_report_subcommand(&["omen", "report", "generate"])
        {
            assert_eq!(args.samples, None);
        }
    }

    #[test]
    fn test_score_trend_samples() {
        let cli = parse(&["omen", "score", "trend", "--samples", "12"]);
        if let Command::Score(cmd) = cli.command {
            if let Some(ScoreSubcommand::Trend(args)) = cmd.subcommand {
                assert_eq!(args.samples, Some(12));
            }
        }
    }

    // Score command tests

    #[test]
    fn test_score_trend() {
        let cli = parse(&["omen", "score", "trend"]);
        if let Command::Score(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, Some(ScoreSubcommand::Trend(_))));
        }
    }

    #[test]
    fn test_score_trend_alias() {
        let cli = parse(&["omen", "score", "tr"]);
        if let Command::Score(cmd) = cli.command {
            assert!(matches!(cmd.subcommand, Some(ScoreSubcommand::Trend(_))));
        }
    }

    #[test]
    fn test_score_trend_since() {
        let cli = parse(&["omen", "score", "trend", "--since", "6m"]);
        if let Command::Score(cmd) = cli.command {
            if let Some(ScoreSubcommand::Trend(args)) = cmd.subcommand {
                assert_eq!(args.since, "6m");
            }
        }
    }

    #[test]
    fn test_score_trend_period() {
        let cli = parse(&["omen", "score", "trend", "--period", "monthly"]);
        if let Command::Score(cmd) = cli.command {
            if let Some(ScoreSubcommand::Trend(args)) = cmd.subcommand {
                assert!(matches!(args.period, TrendPeriod::Monthly));
            }
        }
    }

    #[test]
    fn test_score_check_flag() {
        let cli = parse(&["omen", "score", "--check"]);
        if let Command::Score(cmd) = cli.command {
            assert!(cmd.args.check);
        } else {
            panic!("expected Score command");
        }
    }

    #[test]
    fn test_score_check_default_false() {
        let cli = parse(&["omen", "score"]);
        if let Command::Score(cmd) = cli.command {
            assert!(!cmd.args.check);
        }
    }

    #[test]
    fn test_score_fail_under() {
        let cli = parse(&["omen", "score", "--check", "--fail-under", "80"]);
        if let Command::Score(cmd) = cli.command {
            assert!(cmd.args.check);
            assert_eq!(cmd.args.fail_under, Some(80.0));
        } else {
            panic!("expected Score command");
        }
    }

    #[test]
    fn test_score_fail_under_default_none() {
        let cli = parse(&["omen", "score", "--check"]);
        if let Command::Score(cmd) = cli.command {
            assert!(cmd.args.fail_under.is_none());
        }
    }

    // Global flag tests

    #[test]
    fn test_no_cache_flag() {
        assert!(parse(&["omen", "--no-cache", "complexity"]).no_cache);
    }

    #[test]
    fn test_git_ref_flag() {
        let cli = parse(&["omen", "--ref", "v1.0.0", "complexity"]);
        assert_eq!(cli.git_ref, Some("v1.0.0".to_string()));
    }

    #[test]
    fn test_shallow_flag() {
        assert!(parse(&["omen", "--shallow", "complexity"]).shallow);
    }

    // Search command tests

    #[test]
    fn test_command_search_index() {
        assert!(matches!(
            parse_search_subcommand(&["omen", "search", "index"]),
            SearchSubcommand::Index(_)
        ));
    }

    #[test]
    fn test_search_index_force() {
        if let SearchSubcommand::Index(args) =
            parse_search_subcommand(&["omen", "search", "index", "--force"])
        {
            assert!(args.force);
        }
    }

    #[test]
    fn test_command_search_query() {
        if let SearchSubcommand::Query(args) =
            parse_search_subcommand(&["omen", "search", "query", "function that handles errors"])
        {
            assert_eq!(args.query, "function that handles errors");
        }
    }

    #[test]
    fn test_search_query_top_k() {
        if let SearchSubcommand::Query(args) =
            parse_search_subcommand(&["omen", "search", "query", "test", "-k", "20"])
        {
            assert_eq!(args.top_k, 20);
        }
    }

    #[test]
    fn test_search_query_min_score() {
        if let SearchSubcommand::Query(args) =
            parse_search_subcommand(&["omen", "search", "query", "test", "--min-score", "0.5"])
        {
            assert!((args.min_score - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn test_search_query_files() {
        if let SearchSubcommand::Query(args) = parse_search_subcommand(&[
            "omen",
            "search",
            "query",
            "test",
            "--files",
            "src/main.rs,src/lib.rs",
        ]) {
            assert_eq!(args.files, Some("src/main.rs,src/lib.rs".to_string()));
        }
    }

    #[test]
    fn test_search_query_include_project() {
        if let SearchSubcommand::Query(args) = parse_search_subcommand(&[
            "omen",
            "search",
            "query",
            "test",
            "--include-project",
            "/tmp/other-repo,/tmp/another",
        ]) {
            assert_eq!(
                args.include_project,
                Some("/tmp/other-repo,/tmp/another".to_string())
            );
        }
    }

    // Complexity command tests

    #[test]
    fn test_complexity_check_flag() {
        assert!(parse_complexity_args(&["omen", "complexity", "--check"]).check);
    }

    #[test]
    fn test_complexity_max_cyclomatic() {
        let args =
            parse_complexity_args(&["omen", "complexity", "--check", "--max-cyclomatic", "15"]);
        assert!(args.check);
        assert_eq!(args.max_cyclomatic, Some(15));
    }

    #[test]
    fn test_complexity_max_cognitive() {
        let args =
            parse_complexity_args(&["omen", "complexity", "--check", "--max-cognitive", "20"]);
        assert!(args.check);
        assert_eq!(args.max_cognitive, Some(20));
    }

    #[test]
    fn test_complexity_both_thresholds() {
        let args = parse_complexity_args(&[
            "omen",
            "complexity",
            "--check",
            "--max-cyclomatic",
            "15",
            "--max-cognitive",
            "15",
        ]);
        assert!(args.check);
        assert_eq!(args.max_cyclomatic, Some(15));
        assert_eq!(args.max_cognitive, Some(15));
    }

    #[test]
    fn test_complexity_check_default_false() {
        assert!(!parse_complexity_args(&["omen", "complexity"]).check);
    }

    // Mutation command tests

    #[test]
    fn test_mutation_defaults() {
        let args = parse_mutation_args(&["omen", "mutation"]);
        assert_eq!(args.timeout, 30);
        assert_eq!(args.operators, "CRR,ROR,AOR");
        assert!(!args.check);
        assert!((args.min_score - 0.8).abs() < 0.001);
        assert!(!args.dry_run);
        assert_eq!(args.jobs, 0);
        assert!(args.coverage.is_none());
        assert!(!args.incremental);
        assert!(!args.skip_equivalent);
        assert!(matches!(args.mode, MutationMode::All));
        assert!(args.output_survivors.is_none());
    }

    #[test]
    fn test_mutation_test_command() {
        let args = parse_mutation_args(&["omen", "mutation", "--test-command", "cargo test"]);
        assert_eq!(args.test_command, Some("cargo test".to_string()));
    }

    #[test]
    fn test_mutation_timeout() {
        assert_eq!(
            parse_mutation_args(&["omen", "mutation", "--timeout", "60"]).timeout,
            60
        );
    }

    #[test]
    fn test_mutation_operators() {
        assert_eq!(
            parse_mutation_args(&["omen", "mutation", "--operators", "CRR,ROR"]).operators,
            "CRR,ROR"
        );
    }

    #[test]
    fn test_mutation_check_mode() {
        assert!(parse_mutation_args(&["omen", "mutation", "--check"]).check);
    }

    #[test]
    fn test_mutation_min_score() {
        let args = parse_mutation_args(&["omen", "mutation", "--min-score", "0.9"]);
        assert!((args.min_score - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_mutation_dry_run() {
        assert!(parse_mutation_args(&["omen", "mutation", "--dry-run"]).dry_run);
    }

    #[test]
    fn test_mutation_jobs() {
        assert_eq!(
            parse_mutation_args(&["omen", "mutation", "--jobs", "4"]).jobs,
            4
        );
    }

    #[test]
    fn test_mutation_coverage() {
        let args = parse_mutation_args(&["omen", "mutation", "--coverage", "coverage.json"]);
        assert_eq!(args.coverage, Some(PathBuf::from("coverage.json")));
    }

    #[test]
    fn test_mutation_incremental() {
        assert!(parse_mutation_args(&["omen", "mutation", "--incremental"]).incremental);
    }

    #[test]
    fn test_mutation_skip_equivalent() {
        assert!(parse_mutation_args(&["omen", "mutation", "--skip-equivalent"]).skip_equivalent);
    }

    #[test]
    fn test_mutation_mode_all() {
        let args = parse_mutation_args(&["omen", "mutation", "--mode", "all"]);
        assert!(matches!(args.mode, MutationMode::All));
    }

    #[test]
    fn test_mutation_mode_fast() {
        let args = parse_mutation_args(&["omen", "mutation", "--mode", "fast"]);
        assert!(matches!(args.mode, MutationMode::Fast));
    }

    #[test]
    fn test_mutation_mode_thorough() {
        let args = parse_mutation_args(&["omen", "mutation", "--mode", "thorough"]);
        assert!(matches!(args.mode, MutationMode::Thorough));
    }

    #[test]
    fn test_mutation_output_survivors() {
        let args =
            parse_mutation_args(&["omen", "mutation", "--output-survivors", "survivors.json"]);
        assert_eq!(args.output_survivors, Some(PathBuf::from("survivors.json")));
    }

    #[test]
    fn test_mutation_combined_options() {
        let args = parse_mutation_args(&[
            "omen",
            "mutation",
            "--check",
            "--mode",
            "fast",
            "--jobs",
            "8",
            "--skip-equivalent",
            "--min-score",
            "0.7",
        ]);
        assert!(args.check);
        assert!(matches!(args.mode, MutationMode::Fast));
        assert_eq!(args.jobs, 8);
        assert!(args.skip_equivalent);
        assert!((args.min_score - 0.7).abs() < 0.001);
    }

    // Mutation train subcommand tests

    #[test]
    fn test_command_mutation_train() {
        let cmd = parse_mutation_command(&["omen", "mutation", "train"]);
        assert!(matches!(cmd.subcommand, Some(MutationSubcommand::Train(_))));
    }

    #[test]
    fn test_mutation_train_with_model_path() {
        let cmd =
            parse_mutation_command(&["omen", "mutation", "train", "--model", "custom-model.json"]);
        if let Some(MutationSubcommand::Train(args)) = cmd.subcommand {
            assert_eq!(args.model, Some(PathBuf::from("custom-model.json")));
        } else {
            panic!("Expected Train subcommand");
        }
    }

    #[test]
    fn test_mutation_train_with_history_path() {
        let cmd =
            parse_mutation_command(&["omen", "mutation", "train", "--history", "history.jsonl"]);
        if let Some(MutationSubcommand::Train(args)) = cmd.subcommand {
            assert_eq!(args.history, Some(PathBuf::from("history.jsonl")));
        } else {
            panic!("Expected Train subcommand");
        }
    }

    #[test]
    fn test_mutation_record_flag() {
        assert!(parse_mutation_args(&["omen", "mutation", "--record"]).record);
    }
}
