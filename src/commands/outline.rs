//! The `outline` command.

use std::io::Write;
use std::path::PathBuf;

use super::scope::{build_context, scoped_file_set, Scope};
use crate::config::Config;
use crate::core::Result;
use crate::output::{format_with_limits, Format};

pub struct OutlineRequest {
    /// One file instead of the repository. A relative path starts at the
    /// repository path.
    pub file: Option<PathBuf>,
    pub scope: Scope,
}

pub fn run(
    path: &PathBuf,
    config: &Config,
    request: &OutlineRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    use crate::analyzers::outline::{outline_file_in, Analyzer as OutlineAnalyzer, OutlineResult};
    use crate::core::Analyzer;

    let mut result = if let Some(file) = &request.file {
        // A relative path starts at the repository path and not at the
        // directory of the shell.
        let resolved = if file.is_relative() {
            path.join(file)
        } else {
            file.clone()
        };
        OutlineResult {
            files: vec![outline_file_in(path, &resolved)?],
        }
    } else {
        let file_set = scoped_file_set(path, config, &request.scope)?;
        let ctx = build_context(path, &file_set, config);
        OutlineAnalyzer.analyze(&ctx)?
    };

    // `--top` and `--offset` page the `files` list in each format. The shared
    // rule for nested lists pages `files[].functions`, so it does not apply.
    // SARIF keeps each finding.
    let mut omitted = 0;
    if !matches!(format, Format::Sarif) {
        let offset = request.scope.offset.unwrap_or(0);
        let limit = match request.scope.top {
            Some(0) | None => usize::MAX,
            Some(top) => top,
        };
        let after_offset = result.files.len().saturating_sub(offset);
        result.files = result.files.into_iter().skip(offset).take(limit).collect();
        omitted = after_offset - result.files.len();
    }

    match format {
        Format::Markdown | Format::Text => write!(out, "{}", result.to_markdown())?,
        _ => {
            let mut value = serde_json::to_value(&result)?;
            if omitted > 0 {
                value["files_omitted"] = omitted.into();
            }
            let mut out = out;
            format_with_limits(value, format, None, None, &mut out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::repository;

    fn render(request: &OutlineRequest, format: Format) -> String {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        run(&path, &Config::default(), request, format, &mut out).expect("outline");
        String::from_utf8(out).expect("utf-8")
    }

    fn request(file: Option<&str>, top: Option<usize>, offset: Option<usize>) -> OutlineRequest {
        OutlineRequest {
            file: file.map(PathBuf::from),
            scope: Scope {
                top,
                offset,
                ..Scope::default()
            },
        }
    }

    #[test]
    fn test_repository_mode_lists_each_file() {
        let value: serde_json::Value =
            serde_json::from_str(&render(&request(None, None, None), Format::Json)).expect("json");
        let files: Vec<&str> = value["files"]
            .as_array()
            .expect("files")
            .iter()
            .map(|file| file["file"].as_str().expect("file"))
            .collect();
        assert_eq!(files, ["shop.py", "util.py"]);
    }

    #[test]
    fn test_single_file_mode_resolves_against_the_repository_path() {
        let value: serde_json::Value =
            serde_json::from_str(&render(&request(Some("util.py"), None, None), Format::Json))
                .expect("json");
        assert_eq!(value["files"].as_array().expect("files").len(), 1);
        assert_eq!(value["files"][0]["file"], "util.py");
    }

    #[test]
    fn test_top_and_offset_page_the_files_and_count_the_omitted() {
        let value: serde_json::Value =
            serde_json::from_str(&render(&request(None, Some(1), Some(0)), Format::Json))
                .expect("json");
        assert_eq!(value["files"].as_array().expect("files").len(), 1);
        assert_eq!(value["files_omitted"], 1);
    }
}
