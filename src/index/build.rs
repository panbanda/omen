//! Builds the index of a set of files: the symbols, the import bindings, and
//! the call sites, with their resolutions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use super::extract::{extract_file, link_files, FileSymbols};
use super::imports::{resolve_imports, Manifests};
use super::resolve::resolve_references;
use crate::core::{Language, Result};

/// The facts of each file, in path order.
#[derive(Debug, Clone, Default)]
pub struct CodeIndex {
    pub files: Vec<FileSymbols>,
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The reader of the content of a file.
pub type ReadFile<'a> = &'a (dyn Fn(&Path) -> Result<Vec<u8>> + Sync);

/// The reader of the files of one directory, without the files of the
/// directories in it.
pub type ListDirectory<'a> = &'a (dyn Fn(&Path) -> Vec<PathBuf> + Sync);

fn is_manifest(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    name.is_some_and(|name| {
        ["go.mod", "Cargo.toml", "package.json"].contains(&name) || name.ends_with(".gemspec")
    })
}

/// Reads the manifest files in the directories of the source files and in
/// the directories above them, up to the root. `list` and `read` get the
/// paths in the form that `files` has: an absolute path stays absolute.
fn read_manifests(
    root: &Path,
    files: &[PathBuf],
    read: ReadFile<'_>,
    list: ListDirectory<'_>,
) -> Manifests {
    let mut directories = BTreeSet::new();
    for file in files {
        let mut directory = file.parent();
        while let Some(current) = directory {
            let inside = current.is_relative() || current.starts_with(root);
            if !inside || !directories.insert(current.to_path_buf()) {
                break;
            }
            directory = current.parent();
        }
    }
    let mut manifests = Manifests::default();
    for directory in directories {
        let mut candidates: Vec<PathBuf> = list(&directory)
            .into_iter()
            .filter(|path| is_manifest(path))
            .collect();
        candidates.sort();
        for path in candidates {
            if let Ok(content) = read(&path) {
                manifests.add(&relative(root, &path), &String::from_utf8_lossy(&content));
            }
        }
    }
    manifests
}

impl CodeIndex {
    /// Builds the index of `files` with `read` as the source of the content
    /// and `list` as the source of the names of the manifest files.
    ///
    /// The paths in the index are relative to `root`. The function ignores a
    /// file that has no supported language or that `read` cannot give. The
    /// result does not depend on the order of `files`.
    pub fn build(
        root: &Path,
        files: &[PathBuf],
        read: ReadFile<'_>,
        list: ListDirectory<'_>,
    ) -> Self {
        let mut facts: Vec<FileSymbols> = files
            .par_iter()
            .filter_map(|path| {
                let language = Language::detect(path)?;
                let content = read(path).ok()?;
                let parsed = super::headers::parse(path, language, &content)?;
                Some(extract_file(&parsed, &relative(root, path)))
            })
            .collect();
        super::headers::reparse_headers_of_cpp_files(root, files, read, &mut facts);
        facts.sort_by(|left, right| left.file.cmp(&right.file));
        facts.dedup_by(|later, earlier| later.file == earlier.file);
        link_files(&mut facts);
        resolve_imports(&mut facts, &read_manifests(root, files, read, list));
        resolve_references(&mut facts);
        Self { files: facts }
    }

    /// Builds the index of `files` from the file system.
    pub fn from_files(root: &Path, files: &[PathBuf]) -> Self {
        let list = |directory: &Path| -> Vec<PathBuf> {
            let Ok(entries) = std::fs::read_dir(directory) else {
                return Vec::new();
            };
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
                .map(|entry| entry.path())
                .collect()
        };
        Self::build(root, files, &|path| Ok(std::fs::read(path)?), &list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::model::ImportTarget;

    #[test]
    fn test_manifests_come_from_the_content_reader() {
        let sources = [
            (
                "gem/shop.gemspec",
                "Gem::Specification.new do |spec|\n  spec.require_paths = [\"src\"]\nend\n",
            ),
            ("gem/src/shop/util.rb", "def helper\nend\n"),
            ("gem/bin/run.rb", "require 'shop/util'\n"),
            ("web/package.json", "{ \"name\": \"shop\" }\n"),
            ("web/tools/run.ts", "import { open } from \"shop\";\n"),
        ];
        let read = |path: &Path| {
            sources
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| text.as_bytes().to_vec())
                .ok_or_else(|| crate::core::Error::git("no such file".to_string()))
        };
        let list = |directory: &Path| -> Vec<PathBuf> {
            sources
                .iter()
                .map(|(name, _)| PathBuf::from(name))
                .filter(|path| path.parent() == Some(directory))
                .collect()
        };
        let files: Vec<PathBuf> = ["gem/bin/run.rb", "gem/src/shop/util.rb", "web/tools/run.ts"]
            .iter()
            .map(PathBuf::from)
            .collect();

        let index = CodeIndex::build(Path::new("."), &files, &read, &list);

        let targets: Vec<&ImportTarget> = index
            .files
            .iter()
            .flat_map(|file| &file.imports)
            .map(|binding| &binding.target_file)
            .collect();
        assert_eq!(
            targets,
            [
                &ImportTarget::Resolved("gem/src/shop/util.rb".to_string()),
                &ImportTarget::Unresolved,
            ]
        );
    }

    #[test]
    fn test_index_of_files_on_disk_reads_the_go_module() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("util")).unwrap();
        std::fs::write(root.join("go.mod"), "module example.com/shop\n").unwrap();
        std::fs::write(
            root.join("main.go"),
            "package main\n\nimport \"example.com/shop/util\"\n\nfunc run() int {\n\treturn util.Helper()\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("util/util.go"),
            "package util\n\nfunc Helper() int { return 1 }\n",
        )
        .unwrap();
        let files = vec![root.join("util/util.go"), root.join("main.go")];

        let index = CodeIndex::from_files(root, &files);

        let paths: Vec<&str> = index.files.iter().map(|file| file.file.as_str()).collect();
        assert_eq!(paths, ["main.go", "util/util.go"]);
        let call = &index.files[0].references[0];
        assert_eq!(call.resolution.candidates, ["util/util.go#Helper"]);
    }
}
