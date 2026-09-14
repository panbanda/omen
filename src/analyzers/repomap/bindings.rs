//! Syntax-backed import identities; deliberately excludes wildcard/re-export inference.
use std::path::{Component, Path, PathBuf};
use tree_sitter::Node;

#[derive(Debug, Clone)]
pub struct ImportBinding {
    pub local: String,
    pub original: String,
    pub path: String,
}

fn field(node: Node<'_>, name: &str, source: &[u8]) -> Option<String> {
    Some(
        node.child_by_field_name(name)?
            .utf8_text(source)
            .ok()?
            .to_string(),
    )
}

pub fn extract(root: &Node<'_>, source: &[u8]) -> Vec<ImportBinding> {
    let mut result = Vec::new();
    fn visit(node: Node<'_>, source: &[u8], out: &mut Vec<ImportBinding>) {
        if node.kind() == "import_specifier" {
            if let Some(original) = field(node, "name", source) {
                let local = field(node, "alias", source).unwrap_or_else(|| original.clone());
                let mut parent = node.parent();
                while let Some(p) = parent {
                    if p.kind() == "import_statement" {
                        if let Some(path) = field(p, "source", source) {
                            out.push(ImportBinding {
                                local,
                                original,
                                path: path.trim_matches(['\'', '"']).to_string(),
                            });
                        }
                        break;
                    }
                    parent = p.parent();
                }
            }
        } else if node.kind() == "use_as_clause" {
            if let (Some(mut path), Some(local)) =
                (field(node, "path", source), field(node, "alias", source))
            {
                let mut parent = node.parent();
                while let Some(p) = parent {
                    if p.kind() == "scoped_use_list" {
                        if let Some(prefix) = field(p, "path", source) {
                            path = format!("{prefix}::{path}");
                        }
                    }
                    if p.kind() == "use_declaration" {
                        // Block-scoped aliases need per-use lexical binding, not
                        // a file-wide alias. Do not leak them to other functions.
                        if p.parent().is_none_or(|n| n.kind() != "source_file") {
                            return;
                        }
                        break;
                    }
                    parent = p.parent();
                }
                let original = path.rsplit("::").next().unwrap_or(&path).to_string();
                out.push(ImportBinding {
                    local,
                    original,
                    path,
                });
            }
        }
        for child in node.named_children(&mut node.walk()) {
            visit(child, source, out);
        }
    }
    visit(*root, source, &mut result);
    result
}

/// Exact source-relative TS/JS imports. Unknown non-relative paths defer to the
/// existing heuristic; a relative path that misses never falls back to basename.
pub fn matches(caller: &str, import: &str, target: &str) -> Option<bool> {
    if !import.starts_with("./") && !import.starts_with("../") {
        return None;
    }
    let joined = Path::new(caller).parent()?.join(import);
    let mut normalized = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::Normal(s) => normalized.push(s),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Some(false);
                }
            }
            Component::CurDir => {}
            _ => return Some(false),
        }
    }
    let target = Path::new(target);
    if normalized == target {
        return Some(true);
    }
    if normalized.extension().is_none() {
        return Some(["ts", "tsx", "js", "jsx", "mts", "cts"].iter().any(|ext| {
            normalized.with_extension(ext) == target
                || normalized.join(format!("index.{ext}")) == target
        }));
    }
    // TypeScript commonly spells runtime .js in imports of .ts source.
    Some(
        normalized.extension().is_some_and(|ext| ext == "js")
            && normalized.with_extension("ts") == target,
    )
}
