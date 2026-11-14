//! The `context` command.

use std::io::Write;
use std::path::PathBuf;

use crate::config::Config;
use crate::core::{FileSet, Result};
use crate::output::Format;

pub struct ContextRequest {
    pub max_tokens: usize,
}

pub fn run(
    path: &PathBuf,
    config: &Config,
    request: &ContextRequest,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    let file_set = FileSet::from_path(path, config)?;
    let mut context = crate::context::build_context(
        path,
        &file_set,
        config,
        Some(crate::context::max_symbols_for_token_budget(
            request.max_tokens,
        )),
        Some(25),
    )?;
    crate::context::apply_token_budget(&mut context, request.max_tokens);

    match format {
        Format::Markdown | Format::Text => write!(out, "{}", context.render_markdown())?,
        Format::Json | Format::JsonCompact | Format::Sarif => {
            let mut out = out;
            format.format(&context, &mut out)?
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::repository;

    fn render(format: Format) -> String {
        let (_directory, path) = repository();
        let mut out = Vec::new();
        run(
            &path,
            &Config::default(),
            &ContextRequest { max_tokens: 8000 },
            format,
            &mut out,
        )
        .expect("context");
        String::from_utf8(out).expect("utf-8")
    }

    #[test]
    fn test_json_has_the_context_keys() {
        let value: serde_json::Value = serde_json::from_str(&render(Format::Json)).expect("json");
        assert_eq!(value["file_count"], 2);
        assert!(value["top_symbols"].is_array());
    }

    #[test]
    fn test_markdown_is_rendered_text_and_not_json() {
        let text = render(Format::Markdown);
        assert!(text.contains("# "), "{text}");
        assert!(serde_json::from_str::<serde_json::Value>(&text).is_err());
    }
}
