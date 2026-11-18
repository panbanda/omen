//! The tools that run one analyzer on a path.

use super::{
    Arguments, HandlerResult, Pagination, Param, ParamDefault, ToolOutput, ToolSpec, COMPACT,
    EXCLUDE_TESTS, MAX_COUNT, PATH,
};
use crate::mcp::error::{ErrorCode, ToolError};
use crate::mcp::McpServer;

/// The largest history window: 100 years.
const MAX_DAYS: u64 = 36_500;

const SINCE_DESCRIPTION: &str = "The time window of git history to read, such as 30d, 3m, or 1y. \
    The value all reads the whole history.";

const SINCE_EXPECTED: &str = "a time window such as 30d, 3m, 1y, or all";

/// The days of the `since` argument, or `default_days` when the call has no `since`.
fn window_days(arguments: &Arguments, default_days: u32) -> Result<u32, ToolError> {
    let Some(since) = arguments.string("since") else {
        return Ok(default_days);
    };
    crate::git::since_to_days(since).map_err(|_| {
        ToolError::new(
            ErrorCode::InvalidArgument,
            "The value of `since` is not a time window.",
        )
        .with_param("since")
        .with_expected(SINCE_EXPECTED)
        .with_received(&serde_json::Value::String(since.to_string()))
        .with_next("Send the call again with a window such as `30d`, `3m`, `1y`, or `all`.")
    })
}

pub(super) static COMPLEXITY: ToolSpec = ToolSpec {
    name: "complexity",
    description: "Gives the cyclomatic complexity and the cognitive complexity of each function. \
        The limit applies to the functions. The function with the highest cyclomatic \
        complexity is first, and a file without a function on the page is not in the result.",
    params: &[PATH, EXCLUDE_TESTS],
    paginated: Pagination::Ranked {
        parent: "files",
        child: "functions",
        rank: &crate::output::COMPLEXITY_RANK,
    },
    handler: |server, arguments| {
        server.analyze(arguments, crate::analyzers::complexity::Analyzer::default())
    },
};

pub(super) static SATD: ToolSpec = ToolSpec {
    name: "satd",
    description: "Finds debt comments such as TODO, FIXME, and HACK. \
        The tool also finds the markers in `satd.markers` of the configuration.",
    params: &[PATH, EXCLUDE_TESTS],
    paginated: Pagination::Field("items"),
    handler: |server, arguments| {
        server.analyze(
            arguments,
            crate::commands::analyzers::satd_analyzer(&server.config),
        )
    },
};

pub(super) static DEADCODE: ToolSpec = ToolSpec {
    name: "deadcode",
    description: "Finds symbols that no code uses.",
    params: &[PATH, EXCLUDE_TESTS],
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
    params: &[PATH, EXCLUDE_TESTS],
    paginated: Pagination::Field("stubs"),
    handler: |server, arguments| server.analyze(arguments, crate::analyzers::stubs::Analyzer),
};

pub(super) static CHURN: ToolSpec = ToolSpec {
    name: "churn",
    description: "Finds the files that change most frequently in the git history.",
    params: &[
        PATH,
        EXCLUDE_TESTS,
        Param::string("since", SINCE_DESCRIPTION)
            .with_default(ParamDefault::Configuration("churn.since")),
    ],
    paginated: Pagination::Field("files"),
    handler: |server, arguments| {
        let days = window_days(
            arguments,
            crate::commands::analyzers::configured_churn_days(&server.config),
        )?;
        server.analyze(arguments, crate::commands::analyzers::churn_analyzer(days))
    },
};

pub(super) static CLONES: ToolSpec = ToolSpec {
    name: "clones",
    description: "Finds code clones with MinHash and LSH.",
    params: &[
        PATH,
        EXCLUDE_TESTS,
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
             If false, `exclude_tests` of the configuration applies.",
        )
        .with_default(ParamDefault::Boolean(false)),
    ],
    paginated: Pagination::Field("clones"),
    handler: |server, arguments| {
        let include_tests = arguments.boolean("include_tests").unwrap_or(false);
        server.analyze(
            arguments,
            crate::commands::analyzers::clones_analyzer(
                &server.config,
                arguments.count("min_tokens"),
                arguments.number("similarity"),
                include_tests,
            ),
        )
    },
};

pub(super) static DEFECT: ToolSpec = ToolSpec {
    name: "defect",
    description: "Gives the defect probability of each file from PMAT metrics.",
    params: &[
        PATH,
        EXCLUDE_TESTS,
        Param::string("since", SINCE_DESCRIPTION)
            .with_default(ParamDefault::Configuration("defect.since")),
    ],
    paginated: Pagination::Field("files"),
    handler: |server, arguments| {
        let days = window_days(arguments, server.config.defect.since.days())?;
        server.analyze(arguments, crate::commands::analyzers::defect_analyzer(days))
    },
};

pub(super) static CHANGES: ToolSpec = ToolSpec {
    name: "changes",
    description: "Gives the risk of each recent commit. The tool uses just-in-time risk analysis. \
        The commit with the highest risk is first.",
    params: &[
        PATH,
        Param::string(
            "since",
            "The time window of git history to read, such as 30d, 3m, or 1y. \
             The value all is not a window of this tool.",
        )
        .with_default(ParamDefault::Configuration("changes.since")),
    ],
    paginated: Pagination::Field("commits"),
    handler: |server, arguments| {
        let days = window_days(arguments, server.config.changes.since.days())?;
        if days == u32::MAX {
            return Err(ToolError::new(
                ErrorCode::InvalidArgument,
                crate::commands::analyzers::CHANGES_ALL_MESSAGE,
            )
            .with_param("since")
            .with_expected("a time window such as 30d, 3m, or 1y")
            .with_received(&serde_json::Value::String("all".to_string()))
            .with_next("Send the call again with a window such as `30d`, `3m`, or `1y`."));
        }
        server.analyze(
            arguments,
            crate::commands::analyzers::changes_analyzer(days),
        )
    },
};

pub(super) static DIFF: ToolSpec = ToolSpec {
    name: "diff",
    description: "Gives the risk and the change set of the current branch in relation to a target ref. \
        `symbols` has each symbol that the branch added, removed, or changed, with its signature, \
        its complexity, and its callers. `summary` has the complete counts. \
        `security` has the sinks, the entry points, and the paths that the branch adds or removes. \
        The symbols that are not test code come first. Then a changed or removed symbol comes \
        before an added symbol. Then the symbol with more transitive callers comes first. \
        The limit applies to the symbols. If you give no ref, the tool finds the default branch.",
    params: &[
        PATH,
        Param::string("base", "The target ref, such as a branch or a commit."),
        Param::string(
            "head",
            "The target ref when `base` is absent. \
             The tool always compares the current branch, thus it ignores `head` when you give `base`.",
        ),
        Param::integer(
            "max_callers",
            0,
            MAX_COUNT,
            "The maximum number of direct callers that each symbol lists. The value 0 removes the limit. \
             `direct_caller_count` is always the complete count.",
        )
        .with_default(ParamDefault::Integer(
            crate::commands::diff::DEFAULT_MAX_CALLERS as u64,
        )),
    ],
    paginated: Pagination::Field(crate::commands::diff::PAGED_LIST),
    handler: diff,
};

fn diff(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    let scope = server.scope(arguments)?;
    let target = arguments.string("base").or(arguments.string("head"));
    let max_callers = arguments
        .count("max_callers")
        .unwrap_or(crate::commands::diff::DEFAULT_MAX_CALLERS);
    let report = crate::commands::diff::analyze(&scope.path, target, &server.config, max_callers)?;

    let limit = arguments.count("limit").unwrap_or(0);
    let page = report
        .symbols
        .iter()
        .skip(arguments.count("offset").unwrap_or(0))
        .take(if limit == 0 { usize::MAX } else { limit });
    let (total, omitted) = page.fold((0, 0), |(total, omitted), symbol| {
        (
            total + symbol.direct_caller_count,
            omitted + symbol.direct_callers_omitted,
        )
    });
    let mut data = super::ToolData::new(serde_json::to_value(&report)?);
    if omitted > 0 {
        data.truncated.push(crate::mcp::envelope::Truncation {
            path: format!("/{}/*/direct_callers", crate::commands::diff::PAGED_LIST),
            total,
            returned: total - omitted,
        });
    }
    Ok(ToolOutput::Data(data))
}

pub(super) static TDG: ToolSpec = ToolSpec {
    name: "tdg",
    description: "Gives the Technical Debt Gradient of each file and finds critical defects.",
    params: &[PATH, EXCLUDE_TESTS],
    paginated: Pagination::Field("files"),
    handler: |server, arguments| {
        server.analyze(arguments, crate::analyzers::tdg::Analyzer::default())
    },
};

pub(super) static GRAPH: ToolSpec = ToolSpec {
    name: "graph",
    description: "Gives the dependency graph of the modules, with a Mermaid diagram. \
        The limit applies to the members of the cycles.",
    params: &[PATH, EXCLUDE_TESTS],
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
        EXCLUDE_TESTS,
        Param::string("since", SINCE_DESCRIPTION)
            .with_default(ParamDefault::Configuration("hotspot.since")),
    ],
    paginated: Pagination::Field("hotspots"),
    handler: |server, arguments| {
        let days = window_days(arguments, server.config.hotspot.since.days())?;
        server.analyze(
            arguments,
            crate::commands::analyzers::hotspot_analyzer(days),
        )
    },
};

pub(super) static TEMPORAL: ToolSpec = ToolSpec {
    name: "temporal",
    description: "Finds the files that change together in the git history.",
    params: &[
        PATH,
        EXCLUDE_TESTS,
        Param::string("since", SINCE_DESCRIPTION).with_default(ParamDefault::String("30d")),
    ],
    paginated: Pagination::Field("couplings"),
    handler: |server, arguments| {
        let days = window_days(arguments, crate::analyzers::temporal::DEFAULT_DAYS)?;
        server.analyze(
            arguments,
            crate::analyzers::temporal::Analyzer::new().with_days(days),
        )
    },
};

pub(super) static OWNERSHIP: ToolSpec = ToolSpec {
    name: "ownership",
    description: "Gives the owners of each file and the bus factor. The tool uses git blame.",
    params: &[PATH, EXCLUDE_TESTS],
    paginated: Pagination::Field("files"),
    handler: |server, arguments| {
        server.analyze(arguments, crate::analyzers::ownership::Analyzer::default())
    },
};

pub(super) static COHESION: ToolSpec = ToolSpec {
    name: "cohesion",
    description: "Gives the CK metrics of each class: WMC, CBO, RFC, LCOM4, DIT, and NOC.",
    params: &[PATH, EXCLUDE_TESTS],
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
        EXCLUDE_TESTS,
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
    params: &[PATH, EXCLUDE_TESTS],
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
            1,
            MAX_DAYS,
            "The age in days after which a flag is stale.",
        )
        .with_default(ParamDefault::Configuration("feature_flags.stale_days")),
    ],
    paginated: Pagination::Field("flags"),
    handler: flags,
};

fn flags(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    // The same function as the `flags` command: the argument, then the
    // configured value.
    let config = crate::commands::analyzers::flags_config(
        &server.config,
        arguments.string("provider"),
        arguments.count_u32("stale_days"),
    );
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
            crate::commands::analyzers::score_analyzer(&server.config),
        )
    },
};

const SECURITY_CATEGORIES: &[&str] = &[
    "command_execution",
    "code_evaluation",
    "unsafe_deserialization",
    "sql_string_construction",
    "weak_hash",
    "weak_cipher",
    "tls_verification_disabled",
    "hardcoded_credential",
    "memory_unsafe",
];

pub(super) static SECURITY: ToolSpec = ToolSpec {
    name: "security",
    description: "Gives the sinks, the entry points, and the paths from the entry points to the sinks. \
        A sink is a call, a member, or a literal that the table of sinks names, for example a command \
        or a weak hash. The result gives facts. It does not say that the code has a vulnerability, \
        and it does not follow data. A sink with the state `no_known_entry` has no known path. \
        That state does not say that the sink is safe. The `coverage` field says what omen recognises. \
        The limit applies to the sinks.",
    params: &[
        PATH,
        EXCLUDE_TESTS,
        Param::choice(
            "section",
            &["all", "sinks", "surface", "paths"],
            "The section of the report: the sinks, the entry points (`surface`), or the paths.",
        )
        .with_default(ParamDefault::String("all")),
        Param::string(
            "entry",
            "Keep the paths of this entry point. Give the key of an entry point. Use it with the section `paths`.",
        ),
        Param::string(
            "sink",
            "Keep the paths to this sink. Give the key of a sink. Use it with the section `paths`.",
        ),
        Param::choice("category", SECURITY_CATEGORIES, "Keep the sinks of one category."),
        Param::choice(
            "min_basis",
            super::navigation::BASIS_NAMES,
            "The weakest basis of an edge of a path. The default is the value of `security.min_basis` in the configuration.",
        ),
        Param::boolean(
            "experimental",
            "Look for the sinks of the experimental categories also. The default is the value of `security.experimental` in the configuration.",
        ),
    ],
    paginated: Pagination::Field("sinks"),
    handler: security,
};

fn security(server: &McpServer, arguments: &Arguments<'_>) -> HandlerResult {
    use crate::analyzers::security::{Analyzer, Report, Section};

    let section = arguments
        .string("section")
        .and_then(Section::from_name)
        .unwrap_or(Section::All);
    if section != Section::Paths
        && (arguments.string("entry").is_some() || arguments.string("sink").is_some())
    {
        return Err(ToolError::new(
            ErrorCode::InvalidArgument,
            "`entry` and `sink` apply to the section `paths` only.".to_string(),
        )
        .with_param("section")
        .with_expected("paths")
        .with_next("Call the tool again with `section` set to `paths`."));
    }
    let mut analyzer = Analyzer::from_config(&server.config)?;
    if let Some(basis) = arguments
        .string("min_basis")
        .and_then(crate::index::model::ResolutionBasis::from_name)
    {
        analyzer = analyzer.with_min_basis(basis);
    }
    if arguments.boolean("experimental") == Some(true) {
        analyzer = analyzer.with_experimental(true);
    }
    let ToolOutput::Data(mut data) = server.analyze(arguments, analyzer)? else {
        return Err(ToolError::new(
            ErrorCode::Internal,
            "The analyzer gave no data.".to_string(),
        ));
    };
    let report: Report = serde_json::from_value(data.result)?;
    let report = report.restricted(
        arguments.string("category"),
        arguments.string("entry"),
        arguments.string("sink"),
    );
    let report = if arguments.excludes_tests() {
        report.without_tests()
    } else {
        report
    };
    data.result = report.section(section);
    Ok(ToolOutput::Data(data))
}
