//! The tools that search the semantic index.

use std::path::{Path, PathBuf};

use super::{
    Arguments, HandlerResult, Pagination, Param, ParamDefault, ToolOutput, ToolSpec, MAX_COUNT,
};
use crate::mcp::McpServer;
use crate::semantic::{SearchConfig, SearchFilters, SearchOutput, SemanticSearch};

const TOP_K: Param = Param::integer("top_k", 1, MAX_COUNT, "The maximum number of results.")
    .with_default(ParamDefault::Integer(10));

const MIN_SCORE: Param = Param::number(
    "min_score",
    0.0,
    1.0,
    "The minimum similarity score of a result.",
)
.with_default(ParamDefault::Number(0.3));

const FILES: Param = Param::string_array(
    "files",
    "The files in which to search. The default is all files.",
);

const MAX_COMPLEXITY: Param = Param::integer(
    "max_complexity",
    0,
    MAX_COUNT,
    "The maximum cyclomatic complexity of a result. The tool removes symbols above this value.",
);

const INCLUDE_PROJECTS: Param = Param::string_array(
    "include_projects",
    "The roots of more projects in which to search.",
);

pub(super) static SEMANTIC_SEARCH: ToolSpec = ToolSpec {
    name: "semantic_search",
    description: "Finds the symbols that agree best with a query in natural language. \
        The tool uses TF-IDF.",
    params: &[
        Param::string("query", "The query in natural language.").required(),
        TOP_K,
        MIN_SCORE,
        FILES,
        MAX_COMPLEXITY,
        INCLUDE_PROJECTS,
    ],
    paginated: Pagination::Field("results"),
    handler: |server, arguments| {
        let query = arguments.string("query").unwrap_or_default();
        search(server, arguments, query, query)
    },
};

pub(super) static SEMANTIC_SEARCH_HYDE: ToolSpec = ToolSpec {
    name: "semantic_search_hyde",
    description: "Finds the symbols that are most like a code snippet that you write. \
        Write a snippet that looks like the code that you want to find.",
    params: &[
        Param::string(
            "hypothetical_document",
            "A code snippet that looks like the code that you want to find.",
        )
        .required(),
        Param::string(
            "query",
            "The text that the result shows as its query. The default is the snippet.",
        ),
        TOP_K,
        MIN_SCORE,
        FILES,
        MAX_COMPLEXITY,
        INCLUDE_PROJECTS,
    ],
    paginated: Pagination::Field("results"),
    handler: |server, arguments| {
        let document = arguments
            .string("hypothetical_document")
            .unwrap_or_default();
        let shown_query = arguments.string("query").unwrap_or(document);
        search(server, arguments, document, shown_query)
    },
};

/// Searches for `text` and labels the output with `shown_query`.
fn search(
    server: &McpServer,
    arguments: &Arguments<'_>,
    text: &str,
    shown_query: &str,
) -> HandlerResult {
    let top_k = arguments.count("top_k").unwrap_or(10);
    let min_score = arguments.number("min_score").unwrap_or(0.3) as f32;
    let max_complexity = arguments.count_u32("max_complexity");
    let files = arguments.strings("files");
    let include_projects: Option<Vec<PathBuf>> = arguments
        .strings("include_projects")
        .map(|projects| {
            projects
                .into_iter()
                .map(|project| {
                    server.resolve_confined_path(
                        "include_projects",
                        Path::new(project),
                        Some(&server.root_path),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;

    let search_config = SearchConfig {
        min_score,
        ..SearchConfig::default()
    };
    let search = SemanticSearch::new(&search_config, &server.root_path)?;
    search.index(&server.config)?;

    let mut output = if let Some(projects) = &include_projects {
        let mut all_projects: Vec<&Path> = vec![&server.root_path];
        all_projects.extend(projects.iter().map(PathBuf::as_path));
        let found =
            crate::semantic::multi_repo::multi_repo_search(&all_projects, text, top_k, min_score)?;
        SearchOutput::new(text.to_string(), found.total_symbols, found.results)
    } else if let Some(files) = &files {
        search.search_in_files(text, files, Some(top_k))?
    } else if max_complexity.is_some() {
        let filters = SearchFilters {
            min_score,
            max_complexity,
        };
        search.search_filtered(text, Some(top_k), &filters)?
    } else {
        search.search(text, Some(top_k))?
    };

    // Not each search path applies the filter, thus apply it here also.
    if let Some(maximum) = max_complexity {
        output.results.retain(|result| {
            result
                .cyclomatic_complexity
                .is_none_or(|complexity| complexity <= maximum)
        });
    }
    output.query = shown_query.to_string();

    Ok(ToolOutput::data(serde_json::to_value(&output)?))
}
