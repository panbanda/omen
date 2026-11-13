//! The tools that search the semantic index.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{
    Arguments, HandlerResult, Pagination, Param, ParamDefault, ToolData, ToolOutput, ToolSpec,
    MAX_COUNT,
};
use crate::mcp::envelope::Truncation;
use crate::mcp::error::{ErrorCode, ToolError};
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
     The default is all files. With `include_projects`, a path applies to each project, \
     and a path with a `[label] ` before it, as in the `file` of a result, applies to that project.",
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
    if let (true, Some(files)) = (labelled, &files) {
        let labels = project_labels(&server.root_path, &include_projects);
        for file in files {
            if let Some(label) = file_label(file) {
                if !labels.iter().any(|known| known == label) {
                    return Err(unknown_project_label(file, label, labels));
                }
            }
        }
    }
    output.results.retain(|result| {
        // A plain path in `files` applies to each project. A path with a
        // `[project] ` label is equal to the `file` of a result.
        let in_files = |files: &Vec<&str>| {
            files.contains(&result.file.as_str())
                || (labelled && files.contains(&file_in_project(&result.file)))
        };
        result.score >= min_score
            && files.as_ref().is_none_or(in_files)
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

/// The project label of a file in the `[project] path` form.
fn file_label(file: &str) -> Option<&str> {
    file.strip_prefix('[')
        .and_then(|labelled| labelled.split_once("] "))
        .map(|(label, _)| label)
}

/// The label of each project of a search, in the sequence of the projects.
///
/// `multi_repo_search` makes the same labels: the name of the directory, with
/// a number after the second and each subsequent use of one name. A test
/// compares these labels with the labels in the results.
fn project_labels(root: &Path, include_projects: &[PathBuf]) -> Vec<String> {
    let mut uses: BTreeMap<String, usize> = BTreeMap::new();
    std::iter::once(root)
        .chain(include_projects.iter().map(PathBuf::as_path))
        .map(|project| {
            let name = project
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown")
                .to_string();
            let count = uses.entry(name.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                format!("{name}-{count}")
            } else {
                name
            }
        })
        .collect()
}

fn unknown_project_label(file: &str, label: &str, labels: Vec<String>) -> ToolError {
    ToolError::new(
        ErrorCode::InvalidArgument,
        format!("The label `{label}` in `files` is not the label of a project of this search."),
    )
    .with_param("files")
    .with_expected("a path, or a path with the `[label] ` of a project of this search")
    .with_received(&serde_json::json!(file))
    .with_suggestions(labels)
    .with_next("Use a label from `suggestions`, or remove the label to search in each project.")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{file_in_project, file_label, project_labels};

    #[test]
    fn file_label_gives_the_label_of_a_labelled_path_only() {
        assert_eq!(file_label("[omen] src/main.rs"), Some("omen"));
        assert_eq!(file_label("src/main.rs"), None);
        assert_eq!(file_label("[id]/page.tsx"), None);
    }

    #[test]
    fn project_labels_number_a_name_that_more_than_one_project_has() {
        let projects = [
            PathBuf::from("/work/app"),
            PathBuf::from("/other/omen"),
            PathBuf::from("/third/app"),
        ];
        assert_eq!(
            project_labels(Path::new("/work/omen"), &projects),
            ["omen", "app", "omen-2", "app-2"]
        );
        assert_eq!(project_labels(Path::new("/"), &[]), ["unknown"]);
    }

    #[test]
    fn file_in_project_removes_only_the_project_label() {
        assert_eq!(file_in_project("[omen] src/main.rs"), "src/main.rs");
        assert_eq!(file_in_project("src/main.rs"), "src/main.rs");
        assert_eq!(file_in_project("[id]/page.tsx"), "[id]/page.tsx");
    }
}
