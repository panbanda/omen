//! CLI implementation using clap.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

pub use crate::config::GateMode;

/// Omen - Code analysis CLI for technical debt and complexity metrics.
#[derive(Parser)]
#[command(name = "omen")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to the repository to analyze
    #[arg(short, long, global = true, default_value = ".")]
    pub path: PathBuf,

    /// Output format
    #[arg(short, long, global = true, value_enum, default_value = "markdown")]
    pub format: OutputFormat,

    /// Compact JSON output (single line, no indentation; ignored for non-JSON formats)
    #[arg(long, global = true)]
    pub compact: bool,

    /// Configuration file path
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Number of parallel workers (default: number of CPUs)
    #[arg(short = 'j', long, global = true)]
    pub jobs: Option<usize>,

    /// Git ref (branch, tag, SHA) for remote repositories
    #[arg(long = "ref", global = true)]
    pub git_ref: Option<String>,

    /// Shallow clone (depth=1); disables git history analyzers
    #[arg(long, global = true)]
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
    #[command(visible_alias = "debt")]
    Satd(AnalyzerArgs),

    /// Detect incomplete/placeholder implementations (todo!(), NotImplementedError, empty bodies)
    Stubs(StubsArgs),

    /// Find dead/unreachable code
    #[command(alias = "dc")]
    Deadcode(DeadcodeArgs),

    /// Analyze code churn from git history
    Churn(ChurnArgs),

    /// Detect code duplicates/clones
    #[command(alias = "dup", visible_alias = "duplicates")]
    Clones(ClonesArgs),

    /// Predict defect-prone files using PMAT
    #[command(visible_alias = "predict")]
    Defect(DefectArgs),

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
    #[command(alias = "hs", visible_alias = "hotspots")]
    Hotspot(HotspotArgs),

    /// Detect temporally coupled files
    #[command(alias = "tc", visible_alias = "temporal-coupling")]
    Temporal(AnalyzerArgs),

    /// Analyze code ownership and bus factor
    #[command(alias = "own", visible_alias = "bus-factor")]
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

    #[cfg(feature = "mutation")]
    /// Mutation testing for test suite effectiveness
    #[command(alias = "mut")]
    Mutation(Box<MutationCommand>),

    /// Show token-cheap file outline: imports, classes, functions
    #[command(alias = "ol")]
    Outline(OutlineArgs),

    /// Analyze blast radius of a symbol change (callers/callees by BFS depth)
    #[command(visible_alias = "blast")]
    Impact(ImpactArgs),

    /// Compare two omen binaries over a fixed corpus (paired quality benchmark)
    Benchmark(BenchmarkArgs),

    /// Score registered, paired coding-agent outcome records (fail-closed)
    Eval(EvalArgs),

    /// One-call symbol report: source, signature, location, callers/callees, complexity
    #[command(alias = "sym")]
    Symbol(SymbolArgs),
}

/// The `--top` and `--offset` flags of each command that gives a list.
#[derive(Clone, Args)]
pub struct PaginationArgs {
    /// Limit output to the top N results (0 = unlimited)
    #[arg(short = 'n', long)]
    pub top: Option<usize>,

    /// Skip the first N results (use with --top for pagination)
    #[arg(long)]
    pub offset: Option<usize>,
}

/// The `--gate` flag of each gated command.
#[derive(Args)]
pub struct GateArgs {
    /// Gate mode (default: the `[gate]` value in the configuration, or `off`)
    #[arg(id = "gate", long = "gate", value_enum, value_name = "MODE")]
    pub mode: Option<GateMode>,
}

/// The `--output` flag of each command that can write its report to a file.
#[derive(Args)]
pub struct OutputArgs {
    /// Write the report to this file
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

#[derive(Args)]
pub struct AnalyzerArgs {
    /// Filter by file glob pattern
    #[arg(short, long)]
    pub glob: Option<String>,

    /// Exclude files matching pattern
    #[arg(short, long, action = clap::ArgAction::Append)]
    pub exclude: Vec<String>,

    /// Analyze only files changed since the given git ref
    #[arg(long)]
    pub changed_since: Option<String>,

    #[command(flatten)]
    pub pagination: PaginationArgs,
}

#[derive(Args)]
pub struct ClonesArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Include test files and `#[cfg(test)]` code in duplication detection
    /// (excluded by default)
    #[arg(long)]
    pub include_tests: bool,
}

#[derive(Args)]
pub struct DeadcodeArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Runs `cargo check`, which executes build scripts — only use on trusted code
    #[arg(long)]
    pub cargo_check: bool,
}

#[derive(Args)]
pub struct DiffArgs {
    /// Target branch to diff against (default: auto-detect main/master)
    #[arg(short, long)]
    pub target: Option<String>,

    /// Byte budget of the Markdown comment (0 = unlimited)
    #[arg(long, default_value_t = crate::changeset::markdown::DEFAULT_MAX_BYTES)]
    pub max_bytes: usize,

    /// Node cap of the call graph in the Markdown comment
    #[arg(long, default_value_t = crate::changeset::markdown::DEFAULT_MAX_NODES)]
    pub max_nodes: usize,

    // `diff` has no file set to filter, so it takes only the pagination flags
    // and not the full `AnalyzerArgs`.
    #[command(flatten)]
    pub pagination: PaginationArgs,
}

#[derive(Args)]
pub struct ComplexityArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    #[command(flatten)]
    pub gate: GateArgs,

    /// Maximum cyclomatic complexity (default: from config or 20)
    #[arg(long)]
    pub max_cyclomatic: Option<u32>,

    /// Maximum cognitive complexity (default: from config or 30)
    #[arg(long)]
    pub max_cognitive: Option<u32>,
}

#[derive(Args)]
pub struct StubsArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    #[command(flatten)]
    pub gate: GateArgs,

    /// Minimum severity that counts toward the gate (warn/error)
    #[arg(long, value_enum, default_value = "low")]
    pub gate_severity: GateSeverity,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum GateSeverity {
    Low,
    Medium,
    High,
}

/// A churn time window: a relative duration, or all of history.
///
/// Unlike `churn`, these analyzers reject anything else rather than silently
/// widening to all history -- `parse_since_to_days` returns `None` both for
/// "all" and for garbage, so a permissive parse would make `--since
/// 2024-01-01` mean "every commit ever".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChurnWindow {
    Days(u32),
    All,
}

impl ChurnWindow {
    /// Resolve `--since` (canonical) over `--days` (alias) over the config.
    pub fn resolve(
        since: Option<&str>,
        days: Option<u32>,
        config_days: u32,
    ) -> std::result::Result<Self, String> {
        if let Some(since) = since {
            return since.parse();
        }
        match days.unwrap_or(config_days) {
            0 => Err("a window of 0 days contains no history".to_string()),
            days => Ok(Self::Days(days)),
        }
    }

    /// Days for analyzer builders; `u32::MAX` is the all-history sentinel.
    pub fn as_days(self) -> u32 {
        match self {
            Self::Days(days) => days,
            Self::All => u32::MAX,
        }
    }
}

impl std::str::FromStr for ChurnWindow {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        if crate::git::is_since_all(value) {
            return Ok(Self::All);
        }
        match crate::git::parse_since_to_days(value) {
            Some(0) | None => Err(format!(
                "invalid time window {value:?}: expected a duration such as 30d, 3m, 1y, or \"all\""
            )),
            Some(days) => Ok(Self::Days(days)),
        }
    }
}

/// The `--since`/`--days` pair shared by `defect` and `hotspot`.
#[derive(Args)]
pub struct ChurnWindowArgs {
    /// Time period to measure churn over (e.g. 30d, 3m, 1y, all). The window
    /// ends at the analyzed revision's own date, not at the current time.
    /// Canonical flag; --days is an alias. If both are given, --since wins.
    #[arg(long)]
    pub since: Option<String>,

    /// Days of history to measure churn over (alias for --since)
    #[arg(long)]
    pub days: Option<u32>,
}

#[derive(Args)]
pub struct DefectArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    #[command(flatten)]
    pub window: ChurnWindowArgs,
}

#[derive(Args)]
pub struct HotspotArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    #[command(flatten)]
    pub window: ChurnWindowArgs,
}

#[derive(Args)]
pub struct ChurnArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Time period to analyze (e.g., 3m, 6m, 1y, 2y, all). Canonical time window
    /// flag; --days is a documented alias for the same window. If both are given,
    /// --since wins.
    #[arg(long)]
    pub since: Option<String>,

    /// Number of days to analyze (alias for --since)
    #[arg(long)]
    pub days: Option<u32>,
}

#[derive(Args)]
pub struct FlagsArgs {
    #[command(flatten)]
    pub common: AnalyzerArgs,

    /// Feature flag provider (launchdarkly, split, etc.)
    #[arg(long)]
    pub provider: Option<String>,

    /// The age in days after which a flag is stale. The default is
    /// `feature_flags.stale_days` in the configuration, or 14.
    // The flag has no clap default. A default here has priority over the
    // configuration, and the configured value then has no effect.
    #[arg(long)]
    pub stale_days: Option<u32>,
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
    pub gate: GateArgs,

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
    #[arg(long, value_enum, default_value = "weekly")]
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

    /// Allow tools to access paths outside the configured repository root
    #[arg(long)]
    pub allow_external_paths: bool,
}

/// Context generation for LLM consumption.
#[derive(Args)]
pub struct ContextArgs {
    /// Maximum context tokens to generate
    #[arg(long, default_value = "8000")]
    pub max_tokens: usize,
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

    /// Combine data and insights into one HTML file (default: `.omen/report.html`)
    Render(ReportRenderArgs),

    /// Serve HTML with live re-render on request
    Serve(ReportServeArgs),
}

#[derive(Args)]
pub struct ReportGenerateArgs {
    /// Data directory that receives the JSON files
    #[arg(short, long, default_value = ".omen/data")]
    pub data: PathBuf,

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

    // The default file is `.omen/report.html`.
    #[command(flatten)]
    pub output: OutputArgs,

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
    #[arg(long, default_value = "8080")]
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

    // An absent `--top` gives 10 results.
    #[command(flatten)]
    pub pagination: PaginationArgs,

    /// Minimum similarity score (0.0-1.0)
    #[arg(long, default_value = "0.0")]
    pub min_score: f32,

    /// Limit search to specific files (comma-separated)
    #[arg(long)]
    pub files: Option<String>,

    /// Include additional project paths for cross-repo search (comma-separated)
    #[arg(long)]
    pub include_project: Option<String>,
}

#[derive(Args)]
#[cfg(feature = "mutation")]
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

    #[command(flatten)]
    pub gate: GateArgs,

    /// Minimum mutation score (0.0-1.0)
    #[arg(long, default_value = "0.8")]
    pub min_score: f64,

    /// Generate mutants only, don't execute tests
    #[arg(long)]
    pub dry_run: bool,

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

    #[command(flatten)]
    pub output: OutputArgs,

    /// Record results to history file for model training
    #[arg(long)]
    pub record: bool,

    /// Model file. A relative path is relative to the working directory.
    /// The default is `.omen/mutation-model.json` in the repository.
    #[arg(long)]
    pub model: Option<PathBuf>,

    /// Skip mutants predicted to be killed above this threshold (0.0-1.0)
    #[arg(long, value_name = "THRESHOLD")]
    pub skip_predicted: Option<f64>,

    /// Allow mutation testing on files with uncommitted changes
    #[arg(long)]
    pub allow_dirty: bool,
}

/// Mutation testing mode.
#[derive(Clone, Copy, Debug, ValueEnum)]
#[cfg(feature = "mutation")]
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
#[cfg(feature = "mutation")]
pub struct MutationCommand {
    #[command(subcommand)]
    pub subcommand: Option<MutationSubcommand>,

    #[command(flatten)]
    pub args: MutationArgs,
}

#[derive(Subcommand)]
#[cfg(feature = "mutation")]
pub enum MutationSubcommand {
    /// Train ML predictor from historical mutation results
    Train(MutationTrainArgs),
}

/// Arguments for the outline command.
#[derive(Args)]
pub struct OutlineArgs {
    /// Analyze a single file instead of the whole repo
    #[arg(long)]
    pub file: Option<PathBuf>,

    #[command(flatten)]
    pub common: AnalyzerArgs,
}

/// Reads the name of a basis, for example `name_only`.
fn parse_min_basis(name: &str) -> Result<crate::index::model::ResolutionBasis, String> {
    use crate::index::model::ResolutionBasis;
    ResolutionBasis::from_name(name).ok_or_else(|| {
        let names: Vec<&str> = ResolutionBasis::RANKED
            .iter()
            .map(|basis| basis.name())
            .collect();
        format!("one of: {}", names.join(", "))
    })
}

/// Direction for impact analysis.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ImpactDirection {
    Callers,
    Callees,
    Both,
}

/// Arguments for the impact command.
#[derive(Args)]
pub struct ImpactArgs {
    /// Symbol name to analyze (bare name or qualified file:name)
    #[arg()]
    pub symbol: String,

    /// BFS depth for traversal
    #[arg(long, default_value = "2")]
    pub depth: usize,

    /// Direction of traversal
    #[arg(long, value_enum, default_value = "both")]
    pub direction: ImpactDirection,

    /// Weakest basis of a call that gives an edge (name_only adds a call
    /// whose one candidate has only the name)
    #[arg(long, default_value = "same_package", value_parser = parse_min_basis)]
    pub min_basis: crate::index::model::ResolutionBasis,

    #[command(flatten)]
    pub common: AnalyzerArgs,
}

/// Arguments for the benchmark command.
#[derive(Args)]
pub struct BenchmarkArgs {
    /// Baseline omen binary
    #[arg(long)]
    pub baseline: PathBuf,

    /// Candidate omen binary
    #[arg(long)]
    pub candidate: PathBuf,

    /// Label recorded for the baseline binary (commit SHA or release)
    #[arg(long)]
    pub baseline_label: String,

    /// Label recorded for the candidate binary (commit SHA or release)
    #[arg(long)]
    pub candidate_label: String,

    /// Corpus manifest
    #[arg(long, default_value = "tests/fixtures/quality/corpus.json")]
    pub corpus: PathBuf,

    /// Observations per case beyond the retained warmup (minimum 2)
    #[arg(long, default_value = "7")]
    pub repetitions: usize,

    /// Per-invocation timeout in seconds
    #[arg(long, default_value = "30")]
    pub timeout: f64,

    /// Parent directory holding the real repositories named by the corpus
    #[arg(long, default_value = "..")]
    pub real_root: PathBuf,

    /// Exit 2 if any case regresses
    #[arg(long)]
    pub enforce: bool,
}

/// Arguments for the eval command.
#[derive(Args)]
pub struct EvalArgs {
    /// Registration JSON: model snapshot, prompt hash, token budget and planned cases
    #[arg(long)]
    pub registration: PathBuf,

    /// JSON array of every registered run, including errors and timeouts
    #[arg(long)]
    pub runs: PathBuf,

    #[command(flatten)]
    pub output: OutputArgs,
}

/// Arguments for the symbol command.
#[derive(Args)]
pub struct SymbolArgs {
    /// Symbol name to look up (bare name or qualified file:name)
    #[arg()]
    pub name: String,

    /// Exclude source code from the report
    #[arg(long)]
    pub no_source: bool,

    /// Maximum source lines to include (excess triggers truncation flag)
    #[arg(long, default_value = "200")]
    pub max_source_lines: usize,

    /// Weakest basis of a call that gives a caller or a callee (name_only
    /// adds a call whose one candidate has only the name)
    #[arg(long, default_value = "same_package", value_parser = parse_min_basis)]
    pub min_basis: crate::index::model::ResolutionBasis,

    #[command(flatten)]
    pub common: AnalyzerArgs,
}

/// Arguments for mutation train command.
#[derive(Args)]
#[cfg(feature = "mutation")]
pub struct MutationTrainArgs {
    /// History file. A relative path is relative to the working directory.
    /// The default is `.omen/mutation-history.jsonl` in the repository.
    #[arg(long)]
    pub history: Option<PathBuf>,

    /// Model file to write. A relative path is relative to the working directory.
    /// The default is `.omen/mutation-model.json` in the repository.
    #[arg(long)]
    pub model: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Json,
    Markdown,
    Text,
    Sarif,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum McpTransport {
    Stdio,
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

    #[cfg(feature = "mutation")]
    /// Extract MutationArgs from a parsed CLI, panicking if the command is wrong.
    fn parse_mutation_args(args: &[&str]) -> MutationArgs {
        let cli = parse(args);
        match cli.command {
            Command::Mutation(cmd) => cmd.args,
            _ => panic!("Expected Mutation command"),
        }
    }

    #[cfg(feature = "mutation")]
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
    fn test_cli_format_sarif() {
        assert!(matches!(
            parse(&["omen", "-f", "sarif", "satd"]).format,
            OutputFormat::Sarif
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
    fn test_command_stubs() {
        assert_parses_to!(&["omen", "stubs"], Command::Stubs(_));
    }

    /// Extract StubsArgs from a parsed CLI, panicking if the command is wrong.
    fn parse_stubs_args(args: &[&str]) -> StubsArgs {
        let cli = parse(args);
        match cli.command {
            Command::Stubs(args) => args,
            _ => panic!("Expected Stubs command"),
        }
    }

    #[test]
    fn test_stubs_gate_severity_defaults_to_low() {
        let args = parse_stubs_args(&["omen", "stubs"]);
        assert!(matches!(args.gate_severity, GateSeverity::Low));
    }

    #[test]
    fn test_stubs_gate_error_parses() {
        let args = parse_stubs_args(&["omen", "stubs", "--gate", "error"]);
        assert_eq!(args.gate.mode, Some(GateMode::Error));
    }

    #[test]
    fn test_stubs_gate_warn_parses() {
        let args = parse_stubs_args(&["omen", "stubs", "--gate", "warn"]);
        assert_eq!(args.gate.mode, Some(GateMode::Warn));
    }

    #[test]
    fn test_stubs_gate_severity_high_parses() {
        let args = parse_stubs_args(&["omen", "stubs", "--gate-severity", "high"]);
        assert!(matches!(args.gate_severity, GateSeverity::High));
    }

    #[test]
    fn test_stubs_rejects_invalid_gate_value() {
        assert!(Cli::try_parse_from(["omen", "stubs", "--gate", "bogus"]).is_err());
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
    fn test_command_clones_include_tests_flag() {
        let cli = Cli::try_parse_from(["omen", "clones", "--include-tests"]).unwrap();
        match cli.command {
            Command::Clones(args) => assert!(args.include_tests),
            _ => panic!("expected Command::Clones"),
        }
    }

    #[test]
    fn test_command_clones_include_tests_defaults_false() {
        let cli = Cli::try_parse_from(["omen", "clones"]).unwrap();
        match cli.command {
            Command::Clones(args) => assert!(!args.include_tests),
            _ => panic!("expected Command::Clones"),
        }
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

    #[cfg(feature = "mutation")]
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

    #[cfg(feature = "mutation")]
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
            assert_eq!(args.days, None);
        }
    }

    #[test]
    fn test_churn_days_custom() {
        let cli = parse(&["omen", "churn", "--days", "30"]);
        if let Command::Churn(args) = cli.command {
            assert_eq!(args.days, Some(30));
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
    fn test_flags_stale_days_has_no_default() {
        let cli = parse(&["omen", "flags"]);
        let Command::Flags(args) = cli.command else {
            panic!("Expected Flags command");
        };
        assert_eq!(args.stale_days, None);
    }

    #[test]
    fn test_flags_stale_days() {
        let cli = parse(&["omen", "flags", "--stale-days", "60"]);
        if let Command::Flags(args) = cli.command {
            assert_eq!(args.stale_days, Some(60));
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
    fn test_mcp_external_paths_default_to_denied() {
        let cli = parse(&["omen", "mcp"]);
        if let Command::Mcp(cmd) = cli.command {
            assert!(!cmd.args.allow_external_paths);
        }
    }

    #[test]
    fn test_mcp_allow_external_paths() {
        let cli = parse(&["omen", "mcp", "--allow-external-paths"]);
        if let Command::Mcp(cmd) = cli.command {
            assert!(cmd.args.allow_external_paths);
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
    fn test_analyzer_args_changed_since() {
        let args = parse_complexity_args(&["omen", "complexity", "--changed-since", "main"]);
        assert_eq!(args.common.changed_since, Some("main".to_string()));
    }

    #[test]
    fn test_analyzer_args_exclude_is_repeatable() {
        let args =
            parse_complexity_args(&["omen", "complexity", "-e", "tests/**", "-e", "fixtures/**"]);
        assert_eq!(
            args.common.exclude,
            vec!["tests/**".to_string(), "fixtures/**".to_string()]
        );
    }

    #[test]
    fn test_output_format_default() {
        assert!(matches!(
            parse(&["omen", "complexity"]).format,
            OutputFormat::Markdown
        ));
    }

    #[test]
    fn test_context_max_tokens() {
        let cli = parse(&["omen", "context", "--max-tokens", "4000"]);
        if let Command::Context(args) = cli.command {
            assert_eq!(args.max_tokens, 4000);
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
            parse_report_subcommand(&["omen", "report", "generate", "-d", "/tmp/data"])
        {
            assert_eq!(args.data, PathBuf::from("/tmp/data"));
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
            assert_eq!(args.output.output, Some(PathBuf::from("/tmp/report.html")));
        }
    }

    #[test]
    fn test_report_serve_port() {
        if let ReportSubcommand::Serve(args) =
            parse_report_subcommand(&["omen", "report", "serve", "--port", "3000"])
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
    fn test_score_fail_under() {
        let cli = parse(&["omen", "score", "--gate", "error", "--fail-under", "80"]);
        if let Command::Score(cmd) = cli.command {
            assert_eq!(cmd.args.fail_under, Some(80.0));
        } else {
            panic!("expected Score command");
        }
    }

    #[test]
    fn test_score_fail_under_default_none() {
        let cli = parse(&["omen", "score", "--gate", "error"]);
        if let Command::Score(cmd) = cli.command {
            assert!(cmd.args.fail_under.is_none());
        }
    }

    // Global flag tests

    #[test]
    fn test_git_ref_flag() {
        let cli = parse(&["omen", "--ref", "v1.0.0", "complexity"]);
        assert_eq!(cli.git_ref, Some("v1.0.0".to_string()));
    }

    #[test]
    fn test_ref_and_shallow_parse_after_the_subcommand() {
        let cli = parse(&["omen", "complexity", "--ref", "v1.0.0", "--shallow"]);
        assert_eq!(cli.git_ref, Some("v1.0.0".to_string()));
        assert!(cli.shallow);

        let cli = parse(&["omen", "score", "trend", "--ref", "main"]);
        assert_eq!(cli.git_ref, Some("main".to_string()));
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
    fn test_search_query_min_score_default() {
        if let SearchSubcommand::Query(args) =
            parse_search_subcommand(&["omen", "search", "query", "test"])
        {
            assert_eq!(args.min_score, 0.0);
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
    fn test_complexity_max_cyclomatic() {
        let args = parse_complexity_args(&[
            "omen",
            "complexity",
            "--gate",
            "error",
            "--max-cyclomatic",
            "15",
        ]);
        assert_eq!(args.max_cyclomatic, Some(15));
    }

    #[test]
    fn test_complexity_max_cognitive() {
        let args = parse_complexity_args(&[
            "omen",
            "complexity",
            "--gate",
            "error",
            "--max-cognitive",
            "20",
        ]);
        assert_eq!(args.max_cognitive, Some(20));
    }

    #[test]
    fn test_complexity_both_thresholds() {
        let args = parse_complexity_args(&[
            "omen",
            "complexity",
            "--gate",
            "error",
            "--max-cyclomatic",
            "15",
            "--max-cognitive",
            "15",
        ]);
        assert_eq!(args.max_cyclomatic, Some(15));
        assert_eq!(args.max_cognitive, Some(15));
    }

    #[test]
    fn test_deadcode_cargo_check_default_false() {
        let cli = parse(&["omen", "deadcode"]);
        if let Command::Deadcode(args) = cli.command {
            assert!(!args.cargo_check);
        }
    }

    #[test]
    fn test_deadcode_cargo_check_opt_in() {
        let cli = parse(&["omen", "deadcode", "--cargo-check"]);
        if let Command::Deadcode(args) = cli.command {
            assert!(args.cargo_check);
        }
    }

    // Mutation command tests

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_defaults() {
        let args = parse_mutation_args(&["omen", "mutation"]);
        assert_eq!(args.timeout, 30);
        assert_eq!(args.operators, "CRR,ROR,AOR");
        assert!((args.min_score - 0.8).abs() < 0.001);
        assert!(!args.dry_run);
        assert!(args.coverage.is_none());
        assert!(!args.incremental);
        assert!(!args.skip_equivalent);
        assert!(matches!(args.mode, MutationMode::All));
        assert!(args.output.output.is_none());
        assert!(!args.allow_dirty);
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_allow_dirty() {
        assert!(parse_mutation_args(&["omen", "mutation", "--allow-dirty"]).allow_dirty);
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_test_command() {
        let args = parse_mutation_args(&["omen", "mutation", "--test-command", "cargo test"]);
        assert_eq!(args.test_command, Some("cargo test".to_string()));
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_timeout() {
        assert_eq!(
            parse_mutation_args(&["omen", "mutation", "--timeout", "60"]).timeout,
            60
        );
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_operators() {
        assert_eq!(
            parse_mutation_args(&["omen", "mutation", "--operators", "CRR,ROR"]).operators,
            "CRR,ROR"
        );
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_min_score() {
        let args = parse_mutation_args(&["omen", "mutation", "--min-score", "0.9"]);
        assert!((args.min_score - 0.9).abs() < 0.001);
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_dry_run() {
        assert!(parse_mutation_args(&["omen", "mutation", "--dry-run"]).dry_run);
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_coverage() {
        let args = parse_mutation_args(&["omen", "mutation", "--coverage", "coverage.json"]);
        assert_eq!(args.coverage, Some(PathBuf::from("coverage.json")));
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_incremental() {
        assert!(parse_mutation_args(&["omen", "mutation", "--incremental"]).incremental);
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_skip_equivalent() {
        assert!(parse_mutation_args(&["omen", "mutation", "--skip-equivalent"]).skip_equivalent);
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_mode_all() {
        let args = parse_mutation_args(&["omen", "mutation", "--mode", "all"]);
        assert!(matches!(args.mode, MutationMode::All));
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_mode_fast() {
        let args = parse_mutation_args(&["omen", "mutation", "--mode", "fast"]);
        assert!(matches!(args.mode, MutationMode::Fast));
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_mode_thorough() {
        let args = parse_mutation_args(&["omen", "mutation", "--mode", "thorough"]);
        assert!(matches!(args.mode, MutationMode::Thorough));
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_combined_options() {
        let args = parse_mutation_args(&[
            "omen",
            "mutation",
            "--gate",
            "error",
            "--mode",
            "fast",
            "--skip-equivalent",
            "--min-score",
            "0.7",
        ]);
        assert!(matches!(args.mode, MutationMode::Fast));
        assert!(args.skip_equivalent);
        assert!((args.min_score - 0.7).abs() < 0.001);
    }

    // Mutation train subcommand tests

    #[cfg(feature = "mutation")]
    #[test]
    fn test_command_mutation_train() {
        let cmd = parse_mutation_command(&["omen", "mutation", "train"]);
        assert!(matches!(cmd.subcommand, Some(MutationSubcommand::Train(_))));
    }

    #[cfg(feature = "mutation")]
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

    #[cfg(feature = "mutation")]
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

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_record_flag() {
        assert!(parse_mutation_args(&["omen", "mutation", "--record"]).record);
    }

    #[test]
    fn test_cli_compact_flag() {
        assert!(parse(&["omen", "--compact", "complexity"]).compact);
    }

    #[test]
    fn test_cli_compact_flag_after_subcommand() {
        assert!(parse(&["omen", "complexity", "--compact"]).compact);
    }

    #[test]
    fn test_mcp_rejects_non_stdio_transport() {
        assert!(Cli::try_parse_from(["omen", "mcp", "--transport", "sse"]).is_err());
    }

    #[test]
    fn test_context_rejects_unused_flags() {
        for flag in ["--target", "--symbol", "--depth"] {
            assert!(Cli::try_parse_from(["omen", "context", flag, "value"]).is_err());
        }
    }

    #[test]
    fn test_plugin_fenced_omen_commands_parse() {
        let plugins = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
        let mut markdown_files = Vec::new();
        collect_markdown_files(&plugins, &mut markdown_files);

        let placeholder = regex::Regex::new(r#"\{\{[^}]+\}\}|<[^>]+>"#).unwrap();
        let mut failures = Vec::new();
        for path in markdown_files {
            let content = std::fs::read_to_string(&path).unwrap();
            let mut fenced = false;
            for line in content.lines() {
                if line.trim_start().starts_with("```") {
                    fenced = !fenced;
                    continue;
                }
                let line = line.trim();
                if !fenced || !line.starts_with("omen ") {
                    continue;
                }
                let command = line.split(['|', '>']).next().unwrap().trim();
                let command = placeholder.replace_all(command, "placeholder");
                let args: Vec<&str> = command.split_whitespace().collect();
                if let Err(error) = Cli::try_parse_from(args) {
                    failures.push(format!("{}: `{line}`: {error}", path.display()));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    fn collect_markdown_files(dir: &std::path::Path, files: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_markdown_files(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "md") {
                files.push(path);
            }
        }
    }

    #[test]
    fn test_cli_compact_default_false() {
        assert!(!parse(&["omen", "complexity"]).compact);
    }

    #[test]
    fn test_analyzer_args_top() {
        let args = parse_complexity_args(&["omen", "complexity", "--top", "10"]);
        assert_eq!(args.common.pagination.top, Some(10));
    }

    #[test]
    fn test_analyzer_args_top_default_none() {
        let args = parse_complexity_args(&["omen", "complexity"]);
        assert_eq!(args.common.pagination.top, None);
    }

    #[test]
    fn test_analyzer_args_offset() {
        let args = parse_complexity_args(&["omen", "complexity", "--top", "10", "--offset", "5"]);
        assert_eq!(args.common.pagination.offset, Some(5));
    }

    // Outline command tests

    #[test]
    fn test_command_outline() {
        assert_parses_to!(&["omen", "outline"], Command::Outline(_));
    }

    #[test]
    fn test_command_outline_alias_ol() {
        assert_parses_to!(&["omen", "ol"], Command::Outline(_));
    }

    #[test]
    fn test_outline_file_flag() {
        let cli = parse(&["omen", "outline", "--file", "src/main.rs"]);
        if let Command::Outline(args) = cli.command {
            assert_eq!(args.file, Some(PathBuf::from("src/main.rs")));
        } else {
            panic!("Expected Outline command");
        }
    }

    #[test]
    fn test_outline_no_file_flag() {
        let cli = parse(&["omen", "outline"]);
        if let Command::Outline(args) = cli.command {
            assert_eq!(args.file, None);
        } else {
            panic!("Expected Outline command");
        }
    }

    #[test]
    fn test_outline_common_args() {
        let cli = parse(&["omen", "outline", "-g", "*.rs"]);
        if let Command::Outline(args) = cli.command {
            assert_eq!(args.common.glob, Some("*.rs".to_string()));
        } else {
            panic!("Expected Outline command");
        }
    }

    // Impact command tests

    #[test]
    fn test_command_impact() {
        assert_parses_to!(&["omen", "impact", "my_symbol"], Command::Impact(_));
    }

    #[test]
    fn test_command_impact_alias_blast() {
        assert_parses_to!(&["omen", "blast", "my_symbol"], Command::Impact(_));
    }

    #[test]
    fn test_impact_symbol_positional() {
        let cli = parse(&["omen", "impact", "handle_tool_call"]);
        if let Command::Impact(args) = cli.command {
            assert_eq!(args.symbol, "handle_tool_call");
        } else {
            panic!("Expected Impact command");
        }
    }

    #[test]
    fn test_impact_depth_flag() {
        let cli = parse(&["omen", "impact", "foo", "--depth", "4"]);
        if let Command::Impact(args) = cli.command {
            assert_eq!(args.depth, 4);
        } else {
            panic!("Expected Impact command");
        }
    }

    #[test]
    fn test_impact_direction_callers() {
        let cli = parse(&["omen", "impact", "foo", "--direction", "callers"]);
        if let Command::Impact(args) = cli.command {
            assert!(matches!(args.direction, ImpactDirection::Callers));
        } else {
            panic!("Expected Impact command");
        }
    }

    #[test]
    fn test_impact_direction_callees() {
        let cli = parse(&["omen", "impact", "foo", "--direction", "callees"]);
        if let Command::Impact(args) = cli.command {
            assert!(matches!(args.direction, ImpactDirection::Callees));
        } else {
            panic!("Expected Impact command");
        }
    }

    #[test]
    fn test_impact_direction_default_both() {
        let cli = parse(&["omen", "impact", "foo"]);
        if let Command::Impact(args) = cli.command {
            assert!(matches!(args.direction, ImpactDirection::Both));
        } else {
            panic!("Expected Impact command");
        }
    }

    #[test]
    fn test_impact_depth_default_two() {
        let cli = parse(&["omen", "impact", "foo"]);
        if let Command::Impact(args) = cli.command {
            assert_eq!(args.depth, 2);
        } else {
            panic!("Expected Impact command");
        }
    }

    // Symbol command tests

    #[test]
    fn test_command_symbol() {
        assert_parses_to!(&["omen", "symbol", "my_symbol"], Command::Symbol(_));
    }

    #[test]
    fn test_command_symbol_alias_sym() {
        assert_parses_to!(&["omen", "sym", "my_symbol"], Command::Symbol(_));
    }

    #[test]
    fn test_symbol_name_positional() {
        let cli = parse(&["omen", "symbol", "build_context"]);
        if let Command::Symbol(args) = cli.command {
            assert_eq!(args.name, "build_context");
        } else {
            panic!("Expected Symbol command");
        }
    }

    #[test]
    fn test_symbol_no_source_flag() {
        let cli = parse(&["omen", "symbol", "foo", "--no-source"]);
        if let Command::Symbol(args) = cli.command {
            assert!(args.no_source);
        } else {
            panic!("Expected Symbol command");
        }
    }

    #[test]
    fn test_symbol_max_source_lines_flag() {
        let cli = parse(&["omen", "symbol", "foo", "--max-source-lines", "10"]);
        if let Command::Symbol(args) = cli.command {
            assert_eq!(args.max_source_lines, 10);
        } else {
            panic!("Expected Symbol command");
        }
    }

    #[test]
    fn test_symbol_max_source_lines_default() {
        let cli = parse(&["omen", "symbol", "foo"]);
        if let Command::Symbol(args) = cli.command {
            assert_eq!(args.max_source_lines, 200);
        } else {
            panic!("Expected Symbol command");
        }
    }

    #[test]
    fn test_symbol_no_source_default_false() {
        let cli = parse(&["omen", "symbol", "foo"]);
        if let Command::Symbol(args) = cli.command {
            assert!(!args.no_source);
        } else {
            panic!("Expected Symbol command");
        }
    }

    // -----------------------------------------------------------------
    // Pagination: --top/-n/--offset
    // -----------------------------------------------------------------

    #[test]
    fn test_analyzer_args_top_short_n() {
        let args = parse_complexity_args(&["omen", "complexity", "-n", "5"]);
        assert_eq!(args.common.pagination.top, Some(5));
    }

    #[test]
    fn test_diff_top_offset() {
        let cli = parse(&["omen", "diff", "--top", "3", "--offset", "1"]);
        if let Command::Diff(args) = cli.command {
            assert_eq!(args.pagination.top, Some(3));
            assert_eq!(args.pagination.offset, Some(1));
        } else {
            panic!("Expected Diff command");
        }
    }

    #[test]
    fn test_diff_top_short_n() {
        let cli = parse(&["omen", "diff", "-n", "3"]);
        if let Command::Diff(args) = cli.command {
            assert_eq!(args.pagination.top, Some(3));
        } else {
            panic!("Expected Diff command");
        }
    }

    #[test]
    fn test_diff_rejects_no_op_filter_flags() {
        // diff has no filtered file set to apply glob/exclude/changed-since
        // to, so these must not parse -- advertising flags that silently do
        // nothing is worse than rejecting them outright.
        for args in [
            vec!["omen", "diff", "--glob", "*.rs"],
            vec!["omen", "diff", "--exclude", "tests/**"],
            vec!["omen", "diff", "--changed-since", "main"],
        ] {
            assert!(
                Cli::try_parse_from(&args).is_err(),
                "expected {args:?} to be rejected"
            );
        }
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_diff_comment_flags_have_defaults() {
        let cli = parse(&["omen", "diff"]);
        let Command::Diff(args) = cli.command else {
            panic!("expected Diff command");
        };
        assert_eq!(args.max_bytes, 60000);
        assert_eq!(args.max_nodes, 30);
    }

    #[test]
    fn test_diff_comment_flags_parse() {
        let cli = parse(&["omen", "diff", "--max-bytes", "0", "--max-nodes", "5"]);
        let Command::Diff(args) = cli.command else {
            panic!("expected Diff command");
        };
        assert_eq!(args.max_bytes, 0);
        assert_eq!(args.max_nodes, 5);
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_top_offset() {
        let args = parse_mutation_args(&["omen", "mutation", "--top", "4", "--offset", "2"]);
        assert_eq!(args.common.pagination.top, Some(4));
        assert_eq!(args.common.pagination.offset, Some(2));
    }

    #[test]
    fn test_search_query_top_long_flag_still_works() {
        // --top is now the canonical long flag for search's result limit.
        if let SearchSubcommand::Query(args) =
            parse_search_subcommand(&["omen", "search", "query", "test", "--top", "15"])
        {
            assert_eq!(args.pagination.top, Some(15));
        } else {
            panic!("Expected Query subcommand");
        }
    }

    #[test]
    fn test_search_query_top_short_n_is_primary() {
        // -n is now the canonical short flag for search's result limit,
        // matching -n/--top everywhere else in the CLI.
        if let SearchSubcommand::Query(args) =
            parse_search_subcommand(&["omen", "search", "query", "test", "-n", "15"])
        {
            assert_eq!(args.pagination.top, Some(15));
        } else {
            panic!("Expected Query subcommand");
        }
    }

    // -----------------------------------------------------------------
    // Gate: --gate off|warn|error
    // -----------------------------------------------------------------

    #[test]
    fn test_complexity_gate_defaults_to_none() {
        // Unset (not `Some(GateMode::Off)`) so `resolve_gate_mode` can tell
        // "no --gate at all" apart from an explicit `--gate off`.
        let args = parse_complexity_args(&["omen", "complexity"]);
        assert_eq!(args.gate.mode, None);
    }

    #[test]
    fn test_complexity_gate_error_parses() {
        let args = parse_complexity_args(&["omen", "complexity", "--gate", "error"]);
        assert_eq!(args.gate.mode, Some(GateMode::Error));
    }

    #[test]
    fn test_complexity_gate_warn_parses() {
        let args = parse_complexity_args(&["omen", "complexity", "--gate", "warn"]);
        assert_eq!(args.gate.mode, Some(GateMode::Warn));
    }

    #[test]
    fn test_complexity_gate_off_parses_explicitly() {
        let args = parse_complexity_args(&["omen", "complexity", "--gate", "off"]);
        assert_eq!(args.gate.mode, Some(GateMode::Off));
    }

    #[test]
    fn test_complexity_rejects_invalid_gate_value() {
        assert!(Cli::try_parse_from(["omen", "complexity", "--gate", "bogus"]).is_err());
    }

    #[test]
    fn test_score_gate_defaults_to_none() {
        let cli = parse(&["omen", "score"]);
        if let Command::Score(cmd) = cli.command {
            assert_eq!(cmd.args.gate.mode, None);
        } else {
            panic!("expected Score command");
        }
    }

    #[test]
    fn test_score_gate_error_parses() {
        let cli = parse(&["omen", "score", "--gate", "error"]);
        if let Command::Score(cmd) = cli.command {
            assert_eq!(cmd.args.gate.mode, Some(GateMode::Error));
        } else {
            panic!("expected Score command");
        }
    }

    #[test]
    fn test_score_gate_warn_parses() {
        let cli = parse(&["omen", "score", "--gate", "warn"]);
        if let Command::Score(cmd) = cli.command {
            assert_eq!(cmd.args.gate.mode, Some(GateMode::Warn));
        } else {
            panic!("expected Score command");
        }
    }

    #[test]
    fn test_score_gate_off_parses_explicitly() {
        let cli = parse(&["omen", "score", "--gate", "off"]);
        if let Command::Score(cmd) = cli.command {
            assert_eq!(cmd.args.gate.mode, Some(GateMode::Off));
        } else {
            panic!("expected Score command");
        }
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_gate_defaults_to_none() {
        let args = parse_mutation_args(&["omen", "mutation"]);
        assert_eq!(args.gate.mode, None);
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_gate_error_parses() {
        let args = parse_mutation_args(&["omen", "mutation", "--gate", "error"]);
        assert_eq!(args.gate.mode, Some(GateMode::Error));
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_gate_warn_parses() {
        let args = parse_mutation_args(&["omen", "mutation", "--gate", "warn"]);
        assert_eq!(args.gate.mode, Some(GateMode::Warn));
    }

    #[cfg(feature = "mutation")]
    #[test]
    fn test_mutation_gate_off_parses_explicitly() {
        let args = parse_mutation_args(&["omen", "mutation", "--gate", "off"]);
        assert_eq!(args.gate.mode, Some(GateMode::Off));
    }

    #[test]
    fn test_check_flag_is_rejected() {
        for command in ["complexity", "score", "mutation"] {
            assert!(
                Cli::try_parse_from(["omen", command, "--check"]).is_err(),
                "`{command} --check` must not parse"
            );
        }
    }

    #[test]
    fn test_search_query_rejects_top_k_flags() {
        for flag in ["-k", "--top-k"] {
            assert!(
                Cli::try_parse_from(["omen", "search", "query", "test", flag, "5"]).is_err(),
                "`search query {flag}` must not parse"
            );
        }
    }

    // -----------------------------------------------------------------
    // One CLI shape: gate, pagination, global flags, --output
    // -----------------------------------------------------------------

    #[test]
    fn test_stubs_gate_is_absent_by_default() {
        // An absent flag lets the configuration supply the gate mode.
        assert_eq!(parse_stubs_args(&["omen", "stubs"]).gate.mode, None);
    }

    #[test]
    fn test_all_gated_commands_take_the_same_gate_flag() {
        for command in [
            "complexity",
            "score",
            "stubs",
            #[cfg(feature = "mutation")]
            "mutation",
        ] {
            for mode in ["off", "warn", "error"] {
                assert!(
                    Cli::try_parse_from(["omen", command, "--gate", mode]).is_ok(),
                    "`{command} --gate {mode}` must parse"
                );
            }
        }
    }

    #[test]
    fn test_global_flags_parse_after_the_subcommand() {
        let cli = parse(&[
            "omen",
            "complexity",
            "-p",
            "/tmp",
            "-f",
            "json",
            "-c",
            "config.toml",
            "-v",
            "-j",
            "4",
        ]);
        assert_eq!(cli.path, PathBuf::from("/tmp"));
        assert!(matches!(cli.format, OutputFormat::Json));
        assert_eq!(cli.config, Some(PathBuf::from("config.toml")));
        assert!(cli.verbose);
        assert_eq!(cli.jobs, Some(4));
    }

    #[test]
    fn test_global_flags_parse_after_a_nested_subcommand() {
        let cli = parse(&["omen", "score", "trend", "-p", "/tmp", "--format", "json"]);
        assert_eq!(cli.path, PathBuf::from("/tmp"));
        assert!(matches!(cli.format, OutputFormat::Json));

        #[cfg(feature = "mutation")]
        {
            let cli = parse(&["omen", "mutation", "train", "--path", "/tmp"]);
            assert_eq!(cli.path, PathBuf::from("/tmp"));

            let cli = parse(&["omen", "mutation", "--jobs", "8"]);
            assert_eq!(cli.jobs, Some(8));
        }

        // `-p` is the repository path on each command. It is not the port.
        let cli = parse(&["omen", "report", "serve", "-p", "/tmp", "--port", "3000"]);
        assert_eq!(cli.path, PathBuf::from("/tmp"));
    }

    #[test]
    fn test_search_query_uses_the_shared_pagination_flags() {
        if let SearchSubcommand::Query(args) = parse_search_subcommand(&[
            "omen", "search", "query", "test", "--top", "5", "--offset", "2",
        ]) {
            assert_eq!(args.pagination.top, Some(5));
            assert_eq!(args.pagination.offset, Some(2));
        } else {
            panic!("Expected Query subcommand");
        }
    }

    #[test]
    fn test_output_flag_is_one_optional_file_on_each_command() {
        if let ReportSubcommand::Render(args) =
            parse_report_subcommand(&["omen", "report", "render", "-o", "/tmp/report.html"])
        {
            assert_eq!(args.output.output, Some(PathBuf::from("/tmp/report.html")));
        } else {
            panic!("Expected Render subcommand");
        }

        let cli = parse(&[
            "omen",
            "eval",
            "--registration",
            "plan.json",
            "--runs",
            "runs.json",
            "-o",
            "result.json",
        ]);
        if let Command::Eval(args) = cli.command {
            assert_eq!(args.output.output, Some(PathBuf::from("result.json")));
        } else {
            panic!("Expected Eval command");
        }

        #[cfg(feature = "mutation")]
        {
            let args = parse_mutation_args(&["omen", "mutation", "--output", "mutation.json"]);
            assert_eq!(args.output.output, Some(PathBuf::from("mutation.json")));
        }
    }

    #[test]
    fn test_report_generate_takes_the_data_directory() {
        if let ReportSubcommand::Generate(args) =
            parse_report_subcommand(&["omen", "report", "generate", "-d", "/tmp/data"])
        {
            assert_eq!(args.data, PathBuf::from("/tmp/data"));
        } else {
            panic!("Expected Generate subcommand");
        }
    }

    #[test]
    fn test_replaced_flags_are_rejected() {
        for args in [
            vec!["omen", "report", "generate", "--output", "/tmp/data"],
            vec!["omen", "mutation", "--output-survivors", "survivors.json"],
        ] {
            assert!(
                Cli::try_parse_from(&args).is_err(),
                "expected {args:?} to be rejected"
            );
        }
    }

    // -----------------------------------------------------------------
    // Time window: --since canonical, --days alias (churn)
    // -----------------------------------------------------------------

    #[test]
    fn test_churn_since_flag() {
        let cli = parse(&["omen", "churn", "--since", "6m"]);
        if let Command::Churn(args) = cli.command {
            assert_eq!(args.since, Some("6m".to_string()));
        } else {
            panic!("expected Churn command");
        }
    }

    #[test]
    fn test_churn_since_default_none() {
        let cli = parse(&["omen", "churn"]);
        if let Command::Churn(args) = cli.command {
            assert_eq!(args.since, None);
        } else {
            panic!("expected Churn command");
        }
    }

    #[test]
    fn test_churn_days_still_parses_alongside_since() {
        let cli = parse(&["omen", "churn", "--days", "30", "--since", "6m"]);
        if let Command::Churn(args) = cli.command {
            assert_eq!(args.days, Some(30));
            assert_eq!(args.since, Some("6m".to_string()));
        } else {
            panic!("expected Churn command");
        }
    }

    // -----------------------------------------------------------------
    // Command-name aliases (clones/duplicates, hotspot/hotspots)
    // -----------------------------------------------------------------

    #[test]
    fn test_alias_hotspots_for_hotspot() {
        assert_parses_to!(&["omen", "hotspots"], Command::Hotspot(_));
    }

    #[test]
    fn test_clones_visible_alias_duplicates_shown_in_help() {
        let cmd = Cli::command();
        let clones = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "clones")
            .expect("clones subcommand exists");
        assert!(clones
            .get_visible_aliases()
            .any(|alias| alias == "duplicates"));
    }

    #[test]
    fn test_hotspot_visible_alias_hotspots_shown_in_help() {
        let cmd = Cli::command();
        let hotspot = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "hotspot")
            .expect("hotspot subcommand exists");
        assert!(hotspot
            .get_visible_aliases()
            .any(|alias| alias == "hotspots"));
    }

    // --- Issue #496: churn window flags -----------------------------------

    #[test]
    fn test_churn_window_parses_durations_and_all() {
        use std::str::FromStr;
        assert_eq!(ChurnWindow::from_str("30d"), Ok(ChurnWindow::Days(30)));
        assert_eq!(ChurnWindow::from_str("3m"), Ok(ChurnWindow::Days(90)));
        assert_eq!(ChurnWindow::from_str("1y"), Ok(ChurnWindow::Days(365)));
        assert_eq!(ChurnWindow::from_str("all"), Ok(ChurnWindow::All));
        assert_eq!(ChurnWindow::All.as_days(), u32::MAX);
    }

    #[test]
    fn test_churn_window_rejects_input_it_cannot_measure() {
        use std::str::FromStr;
        // The trap this guards: `parse_since_to_days` returns None for "all"
        // *and* for anything unparseable, so a permissive parse would make an
        // absolute date silently mean "every commit ever".
        for bad in ["2024-01-01", "yesterday", "", "0d"] {
            assert!(
                ChurnWindow::from_str(bad).is_err(),
                "{bad:?} should be rejected, not widened to all history"
            );
        }
    }

    #[test]
    fn test_churn_window_precedence() {
        // --since wins over --days wins over config.
        assert_eq!(
            ChurnWindow::resolve(Some("1y"), Some(7), 30),
            Ok(ChurnWindow::Days(365))
        );
        assert_eq!(
            ChurnWindow::resolve(None, Some(7), 30),
            Ok(ChurnWindow::Days(7))
        );
        assert_eq!(
            ChurnWindow::resolve(None, None, 30),
            Ok(ChurnWindow::Days(30))
        );
        // `--since all` must be a whole branch, not folded into --days.
        assert_eq!(
            ChurnWindow::resolve(Some("all"), Some(7), 30),
            Ok(ChurnWindow::All)
        );
        assert!(ChurnWindow::resolve(None, Some(0), 30).is_err());
    }

    #[test]
    fn test_defect_and_hotspot_accept_window_flags() {
        assert_parses_to!(&["omen", "defect", "--since", "1y"], Command::Defect(_));
        assert_parses_to!(&["omen", "defect", "--days", "90"], Command::Defect(_));
        assert_parses_to!(&["omen", "hotspot", "--since", "all"], Command::Hotspot(_));
        assert_parses_to!(&["omen", "hotspot", "--days", "180"], Command::Hotspot(_));
    }
}
