//! The tools that search the semantic index.

use std::path::{Path, PathBuf};

use super::{
    Arguments, HandlerResult, Pagination, Param, ParamDefault, ToolData, ToolOutput, ToolSpec,
    MAX_COUNT,
};
use crate::mcp::envelope::Truncation;
use crate::mcp::McpServer;
use crate::semantic::{SearchConfig, SearchOutput, SemanticSearch};

const TOP_K: Param = Param::integer(
    "top_k",
    1,
    MAX_COUNT,
    "The maximum number of results. It applies after the other filters.",
)
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
    "The files in which to search, as paths from the root of their project. \
     The default is all files. With `include_projects`, the list applies to each project.",
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
    let include_projects: Vec<PathBuf> = arguments
        .strings("include_projects")
        .unwrap_or_default()
        .into_iter()
        .map(|project| {
            server.resolve_confined_path(
                "include_projects",
                Path::new(project),
                Some(&server.root_path),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let search_config = SearchConfig {
        min_score,
        ..SearchConfig::default()
    };
    let search = SemanticSearch::new(&search_config, &server.root_path)?;
    search.index(&server.config)?;

    // Get all the results in rank sequence. The filters must apply before
    // `top_k`, or a result that a filter removes takes the place of a result
    // that the filters accept.
    let all = Some(usize::MAX);
    let mut output = if !include_projects.is_empty() {
        let mut all_projects: Vec<&Path> = vec![&server.root_path];
        all_projects.extend(include_projects.iter().map(PathBuf::as_path));
        let found = crate::semantic::multi_repo::multi_repo_search(
            &all_projects,
            text,
            usize::MAX,
            min_score,
        )?;
        SearchOutput::new(text.to_string(), found.total_symbols, found.results)
    } else if let Some(files) = &files {
        search.search_in_files(text, files, all)?
    } else {
        search.search(text, all)?
    };

    let labelled = !include_projects.is_empty();
    output.results.retain(|result| {
        let file = if labelled {
            file_in_project(&result.file)
        } else {
            &result.file
        };
        result.score >= min_score
            && files.as_ref().is_none_or(|files| files.contains(&file))
            && max_complexity.is_none_or(|maximum| {
                result
                    .cyclomatic_complexity
                    .is_none_or(|complexity| complexity <= maximum)
            })
    });
    let accepted = output.results.len();
    output.results.truncate(top_k);
    output.query = shown_query.to_string();

    let truncated = if output.results.len() < accepted {
        vec![Truncation {
            path: "/results".to_string(),
            total: accepted,
            returned: output.results.len(),
        }]
    } else {
        Vec::new()
    };
    Ok(ToolOutput::Data(ToolData {
        truncated,
        ..ToolData::new(serde_json::to_value(&output)?)
    }))
}

/// The path of a result without the `[project] ` label that a search in
/// more than one project puts before it.
fn file_in_project(file: &str) -> &str {
    file.strip_prefix('[')
        .and_then(|labelled| labelled.split_once("] "))
        .map_or(file, |(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::file_in_project;

    #[test]
    fn file_in_project_removes_only_the_project_label() {
        assert_eq!(file_in_project("[omen] src/main.rs"), "src/main.rs");
        assert_eq!(file_in_project("src/main.rs"), "src/main.rs");
        assert_eq!(file_in_project("[id]/page.tsx"), "[id]/page.tsx");
    }
}
