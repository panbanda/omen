//! The tools that run one analyzer on a path.

use super::{
    Arguments, HandlerResult, Pagination, Param, ParamDefault, ToolOutput, ToolSpec, COMPACT,
    MAX_COUNT, PATH,
};
use crate::mcp::error::ToolError;
use crate::mcp::McpServer;

/// The largest history window: 100 years.
const MAX_DAYS: u64 = 36_500;

const DAYS_DESCRIPTION: &str = "The number of days of git history to read.";

pub(super) static COMPLEXITY: ToolSpec = ToolSpec {
    name: "complexity",
    description: "Gives the cyclomatic complexity and the cognitive complexity of each function. \
        The limit applies to the functions.",
    params: &[PATH],
    paginated: Pagination::Nested {
        parents: &["files"],
        child: "functions",
    },
    handler: |server, arguments| {
        server.analyze(arguments, crate::analyzers::complexity::Analyzer::default())
    },
};

pub(super) static SATD: ToolSpec = ToolSpec {
    name: "satd",
    description: "Finds debt comments such as TODO, FIXME, and HACK. \
        The tool also finds the markers in `satd.custom_markers` of the configuration.",
    params: &[PATH],
    paginated: Pagination::Field("items"),
    handler: |server, arguments| {
        server.analyze(
            arguments,
            crate::analyzers::satd::Analyzer::new()
                .with_custom_markers(&server.config.satd.custom_markers),
        )
    },
};

pub(super) static DEADCODE: ToolSpec = ToolSpec {
    name: "deadcode",
    description: "Finds symbols that no code uses.",
    params: &[PATH],
    paginated: Pagination::Field("items"),
    handler: |server, arguments| {
        server.analyze(arguments, crate::analyzers::deadcode::Analyzer::default())
    },
};

pub(super) static STUBS: ToolSpec = ToolSpec {
    name: "stubs",
    description: "Finds implementations that are not complete. \
        Examples are a `todo!()` call, a comment that replaces code, and an empty function body. \
        This tool does not apply a gate.",
    params: &[PATH],
    paginated: Pagination::Field("stubs"),
    handler: |server, arguments| server.analyze(arguments, crate::analyzers::stubs::Analyzer),
};

pub(super) static CHURN: ToolSpec = ToolSpec {
    name: "churn",
    description: "Finds the files that change most frequently in the git history.",
    params: &[
        PATH,
        Param::integer("days", 1, MAX_DAYS, DAYS_DESCRIPTION)
            .with_default(ParamDefault::Configuration("churn.since")),
    ],
    paginated: Pagination::Field("files"),
    handler: |server, arguments| {
        // `churn.since` is a window such as `6m` or `all`. Text that is not a
        // window is an error, not all the history.
        let days = match arguments.count_u32("days") {
            Some(days) => days,
            None => crate::git::since_to_days(&server.config.churn.since).map_err(|error| {
                ToolError::internal(format!(
                    "The value of `churn.since` in the configuration is not correct: {error}"
                ))
                .with_param("days")
                .with_next("Correct the configuration, or give `days`.")
            })?,
        };
        server.analyze(
            arguments,
            crate::analyzers::churn::Analyzer::new().with_days(days),
        )
    },
};

pub(super) static CLONES: ToolSpec = ToolSpec {
    name: "clones",
    description: "Finds code clones with MinHash and LSH.",
    params: &[
        PATH,
        Param::integer(
            "min_tokens",
            1,
            MAX_COUNT,
            "The minimum number of tokens in a clone.",
        )
        .with_default(ParamDefault::Configuration("duplicates.min_tokens")),
        Param::number(
            "similarity",
            0.0,
            1.0,
            "The minimum similarity of two fragments.",
        )
        .with_default(ParamDefault::Configuration("duplicates.min_similarity")),
        Param::boolean(
            "include_tests",
            "If true, the tool also reads test code. \
             If false, `duplicates.exclude_tests` of the configuration applies.",
        )
        .with_default(ParamDefault::Boolean(false)),
    ],
    paginated: Pagination::Field("clones"),
    handler: |server, arguments| {
        let configured = &server.config.duplicates;
        let include_tests = arguments.boolean("include_tests").unwrap_or(false);
        server.analyze(
            arguments,
            crate::analyzers::duplicates::Analyzer::new()
                .with_min_tokens(
                    arguments
                        .count("min_tokens")
                        .unwrap_or(configured.min_tokens),
                )
                .with_similarity_threshold(
                    arguments
                        .number("similarity")
                        .unwrap_or(configured.min_similarity),
                )
                .with_exclude_tests(!include_tests && configured.exclude_tests),
        )
    },
};

pub(super) static DEFECT: ToolSpec = ToolSpec {
    name: "defect",
    description: "Gives the defect probability of each file from PMAT metrics.",
    params: &[
        PATH,
        Param::integer("days", 1, MAX_DAYS, DAYS_DESCRIPTION)
            .with_default(ParamDefault::Configuration("defect.churn_days")),
    ],
    paginated: Pagination::Field("files"),
    handler: |server, arguments| {
        let days = arguments
            .count_u32("days")
            .unwrap_or(server.config.defect.churn_days);
        server.analyze(
            arguments,
            crate::analyzers::defect::Analyzer::new().with_churn_days(days),
        )
    },
};

pub(super) static CHANGES: ToolSpec = ToolSpec {
    name: "changes",
    description: "Gives the risk of each recent commit. The tool uses just-in-time risk analysis. \
        `changes.days` of the configuration gives the number of days of history.",
    params: &[PATH],
    paginated: Pagination::Field("commits"),
    handler: |server, arguments| {
        server.analyze(
            arguments,
            crate::analyzers::changes::Analyzer::new().with_days(server.config.changes.days),
        )
    },
};

pub(super) static DIFF: ToolSpec = ToolSpec {
    name: "diff",
    description: "Gives the risk of the changes on the current branch in relation to a target ref. \
        If you give no ref, the tool finds the default branch.",
    params: &[
        PATH,
        Param::string("base", "The target ref, such as a branch or a commit."),
        Param::string(
            "head",
            "The target ref when `base` is absent. \
             The tool always compares the current branch, thus it ignores `head` when you give `base`.",
        ),
    ],
    paginated: Pagination::Field("recommendations"),
    handler: diff,
};

fn diff(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    let scope = server.scope(arguments)?;
    let target = arguments.string("base").or(arguments.string("head"));
    let result =
        crate::analyzers::changes::Analyzer::default().analyze_diff(&scope.path, target)?;
    Ok(ToolOutput::data(serde_json::to_value(&result)?))
}

pub(super) static TDG: ToolSpec = ToolSpec {
    name: "tdg",
    description: "Gives the Technical Debt Gradient of each file and finds critical defects.",
    params: &[PATH],
    paginated: Pagination::Field("files"),
    handler: |server, arguments| {
        server.analyze(arguments, crate::analyzers::tdg::Analyzer::default())
    },
};

pub(super) static GRAPH: ToolSpec = ToolSpec {
    name: "graph",
    description: "Gives the dependency graph of the modules, with a Mermaid diagram. \
        The limit applies to the members of the cycles.",
    params: &[PATH],
    paginated: Pagination::Children("cycles"),
    handler: |server, arguments| {
        server.analyze(arguments, crate::analyzers::graph::Analyzer::default())
    },
};

pub(super) static HOTSPOT: ToolSpec = ToolSpec {
    name: "hotspot",
    description: "Finds the files that have high churn and high complexity.",
    params: &[
        PATH,
        Param::integer("days", 1, MAX_DAYS, DAYS_DESCRIPTION)
            .with_default(ParamDefault::Configuration("hotspot.days")),
    ],
    paginated: Pagination::Field("hotspots"),
    handler: |server, arguments| {
        let days = arguments
            .count_u32("days")
            .unwrap_or(server.config.hotspot.days);
        server.analyze(
            arguments,
            crate::analyzers::hotspot::Analyzer::new().with_days(days),
        )
    },
};

pub(super) static TEMPORAL: ToolSpec = ToolSpec {
    name: "temporal",
    description: "Finds the files that change together in the git history.",
    params: &[
        PATH,
        Param::integer("days", 1, MAX_DAYS, DAYS_DESCRIPTION)
            .with_default(ParamDefault::Integer(30)),
    ],
    paginated: Pagination::Field("couplings"),
    handler: |server, arguments| {
        let days = arguments.count_u32("days").unwrap_or(30);
        server.analyze(
            arguments,
            crate::analyzers::temporal::Analyzer::new().with_days(days),
        )
    },
};

pub(super) static OWNERSHIP: ToolSpec = ToolSpec {
    name: "ownership",
    description: "Gives the owners of each file and the bus factor. The tool uses git blame.",
    params: &[PATH],
    paginated: Pagination::Field("files"),
    handler: |server, arguments| {
        server.analyze(arguments, crate::analyzers::ownership::Analyzer::default())
    },
};

pub(super) static COHESION: ToolSpec = ToolSpec {
    name: "cohesion",
    description: "Gives the CK metrics of each class: WMC, CBO, RFC, LCOM4, DIT, and NOC.",
    params: &[PATH],
    paginated: Pagination::Field("classes"),
    handler: |server, arguments| {
        server.analyze(arguments, crate::analyzers::cohesion::Analyzer::default())
    },
};

pub(super) static REPOMAP: ToolSpec = ToolSpec {
    name: "repomap",
    description: "Gives the symbols of the repository in PageRank sequence, with their call graph.",
    params: &[
        PATH,
        Param::integer(
            "max_symbols",
            0,
            MAX_COUNT,
            "The maximum number of symbols that the analyzer keeps. The value 0 removes the limit.",
        )
        .with_default(ParamDefault::Integer(0)),
        COMPACT,
    ],
    paginated: Pagination::Field("symbols"),
    handler: |server, arguments| {
        let max_symbols = arguments.count("max_symbols").unwrap_or(0);
        let analyzer = |limit| crate::analyzers::repomap::Analyzer::new().with_max_symbols(limit);
        let output = server.analyze(arguments, analyzer(max_symbols))?;
        let ToolOutput::Data(mut data) = output else {
            return Ok(output);
        };
        let returned = data.result["symbols"].as_array().map_or(0, Vec::len);
        // A full list tells that the limit can have cut symbols. A second run
        // without the limit gives the total.
        if max_symbols > 0 && returned == max_symbols {
            if let ToolOutput::Data(all) = server.analyze(arguments, analyzer(0))? {
                let total = all.result["symbols"].as_array().map_or(0, Vec::len);
                if total > returned {
                    data.truncated.push(crate::mcp::envelope::Truncation {
                        path: "/symbols".to_string(),
                        total,
                        returned,
                    });
                }
            }
        }
        Ok(ToolOutput::Data(data))
    },
};

pub(super) static SMELLS: ToolSpec = ToolSpec {
    name: "smells",
    description: "Finds architectural smells such as dependency cycles. The tool uses Tarjan SCC.",
    params: &[PATH],
    paginated: Pagination::Field("smells"),
    handler: |server, arguments| {
        server.analyze(arguments, crate::analyzers::smells::Analyzer::default())
    },
};

pub(super) static FLAGS: ToolSpec = ToolSpec {
    name: "flags",
    description: "Finds feature flags and identifies the stale flags.",
    params: &[
        PATH,
        Param::string(
            "provider",
            "The flag provider to find, such as `launchdarkly` or `unleash`.",
        )
        .with_default(ParamDefault::Configuration("feature_flags.providers")),
        Param::integer(
            "stale_days",
            0,
            MAX_DAYS,
            "The age in days after which a flag is stale. \
             The value 0 selects the built-in age of 14 days.",
        )
        .with_default(ParamDefault::Configuration("feature_flags.stale_days")),
    ],
    paginated: Pagination::Field("flags"),
    handler: flags,
};

fn flags(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    let providers: Vec<String> = arguments
        .string("provider")
        .map(|provider| vec![provider.to_string()])
        .unwrap_or_default();
    let mut config = server.config.clone();
    if !providers.is_empty() {
        config.feature_flags.providers = providers;
    }
    // The same rule as the `flags` command: the argument, then the
    // configured value. The analyzer uses 14 days when the value is 0.
    if let Some(stale_days) = arguments.count_u32("stale_days") {
        config.feature_flags.stale_days = stale_days;
    }
    server.analyze_with_config(
        arguments,
        &config,
        crate::analyzers::flags::Analyzer::default(),
    )
}

pub(super) static SCORE: ToolSpec = ToolSpec {
    name: "score",
    description: "Gives the health score of the repository and the score of each component.",
    params: &[PATH],
    paginated: Pagination::Entries("components"),
    handler: |server, arguments| {
        server.analyze(
            arguments,
            crate::score::Analyzer::from_config(&server.config),
        )
    },
};
