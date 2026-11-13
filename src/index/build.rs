//! Builds the index of a set of files: the symbols, the import bindings, and
//! the call sites, with their resolutions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use super::extract::{extract_file, link_files, FileSymbols};
use super::imports::{resolve_imports, Manifests};
use super::resolve::resolve_references;
use crate::core::{Language, Result};
use crate::parser::Parser;

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

/// Reads the `go.mod` files and the `.gemspec` files in the directories of
/// the source files and in the directories above them, up to the root.
fn read_manifests(
    root: &Path,
    files: &[PathBuf],
    read: &(dyn Fn(&Path) -> Result<Vec<u8>> + Sync),
) -> Manifests {
    let mut directories = BTreeSet::new();
    for file in files {
        let mut directory = file.parent();
        while let Some(current) = directory {
            if !current.starts_with(root) || !directories.insert(current.to_path_buf()) {
                break;
            }
            directory = current.parent();
        }
    }
    let mut manifests = Manifests::default();
    for directory in directories {
        let mut candidates = vec![directory.join("go.mod"), directory.join("Cargo.toml")];
        if let Ok(entries) = std::fs::read_dir(&directory) {
            let mut specifications: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "gemspec")
                })
                .collect();
            specifications.sort();
            candidates.extend(specifications);
        }
        for path in candidates {
            if let Ok(content) = read(&path) {
                manifests.add(&relative(root, &path), &String::from_utf8_lossy(&content));
            }
        }
    }
    manifests
}

impl CodeIndex {
    /// Builds the index of `files` with `read` as the source of the content.
    ///
    /// The paths in the index are relative to `root`. The function ignores a
    /// file that has no supported language or that `read` cannot give. The
    /// result does not depend on the order of `files`.
    pub fn build(
        root: &Path,
        files: &[PathBuf],
        read: &(dyn Fn(&Path) -> Result<Vec<u8>> + Sync),
    ) -> Self {
        let mut facts: Vec<FileSymbols> = files
            .par_iter()
            .filter_map(|path| {
                let language = Language::detect(path)?;
                let content = read(path).ok()?;
                let parsed = Parser::new().parse(&content, language, path).ok()?;
                Some(extract_file(&parsed, &relative(root, path)))
            })
            .collect();
        facts.sort_by(|left, right| left.file.cmp(&right.file));
        facts.dedup_by(|later, earlier| later.file == earlier.file);
        link_files(&mut facts);
        resolve_imports(&mut facts, &read_manifests(root, files, read));
        resolve_references(&mut facts);
        Self { files: facts }
    }

    /// Builds the index of `files` from the file system.
    pub fn from_files(root: &Path, files: &[PathBuf]) -> Self {
        Self::build(root, files, &|path| Ok(std::fs::read(path)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
