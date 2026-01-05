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
