//! The file dependency graph that `graph` and `smells` share.
//!
//! The two analyzers call the same functions. They cannot disagree on the
//! edges or on the cycles for the same input.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};

use super::build::CodeIndex;
use super::model::{ImportKind, ImportTarget};

/// The files that each file imports, in path order of the importing files.
///
/// A statement that declares a module in a different file, as Rust `mod name;`
/// does, is containment and not a dependency. An import that has more than one
/// possible file gives no target, because the index cannot tell which one the
/// import names. An import of a module that is not in the repository gives its
/// path as the target only when `include_external` is set.
pub fn file_dependencies(index: &CodeIndex, include_external: bool) -> Vec<(String, Vec<String>)> {
    index
        .files
        .iter()
        .map(|file| {
            let mut targets = Vec::new();
            for binding in &file.imports {
                if binding.kind == ImportKind::ModuleDeclaration {
                    continue;
                }
                match &binding.target_file {
                    ImportTarget::Resolved(target) => targets.push(target.clone()),
                    ImportTarget::Package(members) => targets.extend(members.iter().cloned()),
                    ImportTarget::Ambiguous(_) => {}
                    ImportTarget::External | ImportTarget::Unresolved => {
                        if include_external {
                            targets.push(binding.module_path.clone());
                        }
                    }
                }
            }
            (file.file.clone(), targets)
        })
        .collect()
}

/// The path of each import statement of each file, as the source has it, with
/// no resolution. A path that more than one statement names occurs one time.
pub fn file_import_specifiers(index: &CodeIndex) -> Vec<(String, Vec<String>)> {
    index
        .files
        .iter()
        .map(|file| {
            let mut paths: Vec<String> = Vec::new();
            for binding in &file.imports {
                if binding.kind != ImportKind::ModuleDeclaration
                    && !paths.contains(&binding.module_path)
                {
                    paths.push(binding.module_path.clone());
                }
            }
            (file.file.clone(), paths)
        })
        .collect()
}

/// Builds a dependency graph from the pairs of a file and its imports.
///
/// A file has no edge to itself, and a repeated import gives one edge.
/// `include_external` gives a target that is not a file of the input its own node.
pub fn build_dependency_graph(
    file_imports: &[(String, Vec<String>)],
    include_external: bool,
) -> (DiGraph<String, ()>, HashMap<String, NodeIndex>) {
    let mut graph: DiGraph<String, ()> =
        DiGraph::with_capacity(file_imports.len(), file_imports.len() * 4);
    let mut node_indices: HashMap<String, NodeIndex> = HashMap::with_capacity(file_imports.len());

    for (path, _) in file_imports {
        node_indices
            .entry(path.clone())
            .or_insert_with(|| graph.add_node(path.clone()));
    }

    for (from_path, imports) in file_imports {
        let from_idx = node_indices[from_path];

        for import in imports {
            let to_idx = if let Some(&idx) = node_indices.get(import) {
                idx
            } else if include_external {
                *node_indices
                    .entry(import.clone())
                    .or_insert_with(|| graph.add_node(import.clone()))
            } else {
                continue;
            };

            if from_idx != to_idx && !graph.contains_edge(from_idx, to_idx) {
                graph.add_edge(from_idx, to_idx, ());
            }
        }
    }

    (graph, node_indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_of(sources: &[(&str, &str)]) -> CodeIndex {
        let directory = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        for (name, text) in sources {
            let path = directory.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            files.push(path);
        }
        CodeIndex::from_files(directory.path(), &files)
    }

    fn dependencies_of<'a>(dependencies: &'a [(String, Vec<String>)], file: &str) -> &'a [String] {
        &dependencies
            .iter()
            .find(|(name, _)| name == file)
            .unwrap_or_else(|| panic!("no entry for {file}"))
            .1
    }

    #[test]
    fn test_relative_typescript_import_gives_the_target_file() {
        let index = index_of(&[
            ("a.ts", "import { b } from './b';\nexport const a = b;\n"),
            ("b.ts", "export const b = 1;\n"),
        ]);

        let dependencies = file_dependencies(&index, false);

        assert_eq!(dependencies_of(&dependencies, "a.ts"), ["b.ts"]);
        assert!(dependencies_of(&dependencies, "b.ts").is_empty());
    }

    #[test]
    fn test_specifiers_are_the_paths_of_the_source_without_resolution() {
        let index = index_of(&[
            (
                "a.ts",
                "import { b } from './b';\nimport React from 'react';\n",
            ),
            ("b.ts", "export const b = 1;\n"),
        ]);

        let specifiers = file_import_specifiers(&index);

        assert_eq!(dependencies_of(&specifiers, "a.ts"), ["./b", "react"]);
    }

    #[test]
    fn test_external_import_has_a_target_only_when_requested() {
        let index = index_of(&[("a.ts", "import React from 'react';\nexport const a = 1;\n")]);

        assert!(dependencies_of(&file_dependencies(&index, false), "a.ts").is_empty());
        assert_eq!(
            dependencies_of(&file_dependencies(&index, true), "a.ts"),
            ["react"]
        );
    }

    #[test]
    fn test_rust_module_declaration_is_not_a_dependency() {
        let index = index_of(&[
            ("src/lib.rs", "mod util;\npub fn run() { util::help(); }\n"),
            ("src/util.rs", "pub fn help() {}\n"),
        ]);

        let dependencies = file_dependencies(&index, false);

        assert!(dependencies_of(&dependencies, "src/lib.rs").is_empty());
    }

    #[test]
    fn test_go_package_import_gives_each_file_of_the_package() {
        let index = index_of(&[
            ("go.mod", "module example.com/shop\n"),
            (
                "main.go",
                "package main\n\nimport \"example.com/shop/util\"\n\nfunc run() { util.A() }\n",
            ),
            ("util/a.go", "package util\n\nfunc A() {}\n"),
            ("util/b.go", "package util\n\nfunc B() {}\n"),
        ]);

        let dependencies = file_dependencies(&index, false);

        assert_eq!(
            dependencies_of(&dependencies, "main.go"),
            ["util/a.go", "util/b.go"]
        );
    }

    #[test]
    fn test_graph_has_no_self_loop_and_no_parallel_edge() {
        let pairs = vec![
            (
                "a".to_string(),
                vec!["a".to_string(), "b".to_string(), "b".to_string()],
            ),
            ("b".to_string(), Vec::new()),
        ];

        let (graph, _) = build_dependency_graph(&pairs, false);

        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn test_graph_gives_an_external_target_its_own_node_on_request() {
        let pairs = vec![("a".to_string(), vec!["react".to_string()])];

        let (without, _) = build_dependency_graph(&pairs, false);
        let (with, nodes) = build_dependency_graph(&pairs, true);

        assert_eq!(without.node_count(), 1);
        assert_eq!(with.node_count(), 2);
        assert!(nodes.contains_key("react"));
    }
}
