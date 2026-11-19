//! Builds the index of a set of files: the symbols, the import bindings, and
//! the call sites, with their resolutions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use super::extract::{extract_file, link_files, FileSymbols};
use super::imports::{resolve_imports, Manifests};
use super::resolve::resolve_references_with;
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

/// Tells if the file is a manifest that the index reads.
pub fn is_manifest(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    name.is_some_and(|name| {
        ["go.mod", "Cargo.toml", "package.json"].contains(&name)
            || name.ends_with(".gemspec")
            || super::imports::typescript::is_project_file(name)
    })
}

/// The manifest files in the directories of the source files and in the
/// directories above them, up to the root, in the order of the directories.
/// `list` gets the paths in the form that `files` has: an absolute path
/// stays absolute.
pub fn manifest_paths(root: &Path, files: &[PathBuf], list: ListDirectory<'_>) -> Vec<PathBuf> {
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
    let mut paths = Vec::new();
    for directory in directories {
        let mut candidates: Vec<PathBuf> = list(&directory)
            .into_iter()
            .filter(|path| is_manifest(path))
            .collect();
        candidates.sort();
        paths.extend(candidates);
    }
    paths
}

/// The maximum number of rounds that follow `extends` of TypeScript configs.
/// Each round reads one more level of the chain.
const EXTENDS_ROUNDS: usize = 8;

/// The parent directories that the search for an unknown package can try,
/// for each manifest that the repository has.
const PROBES_PER_MANIFEST: usize = 4;

/// The manifest files that `manifest_paths` gives, and the TypeScript
/// configs that their `extends` chains name. A config of a package can be in
/// a directory without a source file. The content of each file is `None`
/// when `read` cannot give it.
pub fn manifest_files(
    root: &Path,
    files: &[PathBuf],
    read: ReadFile<'_>,
    list: ListDirectory<'_>,
) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    discover_manifests(root, files, read, list).0
}

/// A manifest file and its content.
type ManifestFile = (PathBuf, Option<Vec<u8>>);

/// The result of `manifest_files`, and the manifests that the files give.
fn discover_manifests(
    root: &Path,
    files: &[PathBuf],
    read: ReadFile<'_>,
    list: ListDirectory<'_>,
) -> (Vec<ManifestFile>, Manifests) {
    // A path of the repository in the form that `files` has.
    let absolute = files.first().is_some_and(|file| file.is_absolute());
    let in_form = |relative_path: &str| {
        if absolute {
            root.join(relative_path)
        } else {
            PathBuf::from(relative_path)
        }
    };
    let mut found: Vec<ManifestFile> = Vec::new();
    let mut manifests = Manifests::default();
    let mut tried: BTreeSet<String> = BTreeSet::new();
    // A config that `extends` names is a config, whatever its name.
    let add = |path: PathBuf,
               extended: bool,
               manifests: &mut Manifests,
               found: &mut Vec<ManifestFile>| {
        let content = read(&path).ok();
        if let Some(content) = &content {
            let text = String::from_utf8_lossy(content);
            if extended {
                manifests.add_project(&relative(root, &path), &text);
            } else {
                manifests.add(&relative(root, &path), &text);
            }
        }
        found.push((path, content));
    };
    for path in manifest_paths(root, files, list) {
        tried.insert(relative(root, &path));
        add(path, false, &mut manifests, &mut found);
    }
    // The search for a package that `extends` names and that the index has
    // not read tries each parent directory of the known packages. The number
    // of names times the number of parents can grow faster than the
    // repository, thus the tries have a limit in proportion to the
    // manifests. A name that the tries do not reach stays unknown.
    let mut probes = PROBES_PER_MANIFEST * (found.len() + 1);
    for _ in 0..EXTENDS_ROUNDS {
        let packages = &manifests.node_packages;
        let mut wanted: Vec<(String, bool)> = manifests
            .projects
            .unread(packages)
            .into_iter()
            .map(|path| (path, true))
            .collect();
        // A workspace package is often next to the other packages. Its
        // `package.json` gives the directory of the package.
        let parents: BTreeSet<&str> = packages
            .iter()
            .map(|package| {
                package
                    .directory
                    .rsplit_once('/')
                    .map_or("", |(parent, _)| parent)
            })
            .collect();
        for name in manifests.projects.unknown_packages(packages) {
            let last = name.rsplit('/').next().unwrap_or(&name).to_string();
            for parent in &parents {
                if probes == 0 {
                    break;
                }
                probes -= 1;
                for directory in [&name, &last] {
                    let path = if parent.is_empty() {
                        format!("{directory}/package.json")
                    } else {
                        format!("{parent}/{directory}/package.json")
                    };
                    wanted.push((path, false));
                }
            }
        }
        wanted.retain(|(path, _)| tried.insert(path.clone()));
        let listed = |path: &Path| {
            path.parent()
                .is_some_and(|directory| list(directory).iter().any(|entry| entry == path))
        };
        let mut added = false;
        for (path, extended) in wanted {
            let path = in_form(&path);
            // Only a file that the listing of its directory has is read.
            if !listed(&path) {
                continue;
            }
            add(path, extended, &mut manifests, &mut found);
            added = true;
        }
        if !added {
            break;
        }
    }
    (found, manifests)
}

/// Reads the manifest files that `manifest_files` gives.
fn read_manifests(
    root: &Path,
    files: &[PathBuf],
    read: ReadFile<'_>,
    list: ListDirectory<'_>,
) -> Manifests {
    discover_manifests(root, files, read, list).1
}

/// The files of one directory of the file system, without the directories.
pub fn list_directory(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
        .map(|entry| entry.path())
        .collect()
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
                if !Language::is_source_path(path) {
                    return None;
                }
                let content = read(path).ok()?;
                let language = Language::detect_with_content(path, &content)?;
                let parsed = super::headers::parse(path, language, &content)?;
                Some(extract_file(&parsed, &relative(root, path)))
            })
            .collect();
        super::headers::reparse_headers_of_cpp_files(root, files, read, &mut facts);
        facts.sort_by(|left, right| left.file.cmp(&right.file));
        facts.dedup_by(|later, earlier| later.file == earlier.file);
        link_files(&mut facts);
        let manifests = read_manifests(root, files, read, list);
        resolve_imports(&mut facts, &manifests);
        super::test_code::link_files(&mut facts);
        resolve_references_with(&mut facts, &manifests);
        Self { files: facts }
    }

    /// Builds the index of `files` from the file system.
    pub fn from_files(root: &Path, files: &[PathBuf]) -> Self {
        Self::build(
            root,
            files,
            &|path| Ok(std::fs::read(path)?),
            &list_directory,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::model::ImportTarget;

    #[test]
    fn test_c_call_through_a_header_that_cpp_includes_keeps_c_linkage() {
        // A C++ file includes `api.h`, thus the index parses it as C++. The
        // call in `main.c` is still a C call, and the C++ definition without
        // `extern "C"` is a different function.
        let sources = [
            ("api.h", "void shared(void);\n"),
            (
                "main.c",
                "#include \"api.h\"\n\nint main(void) {\n    shared();\n    return 0;\n}\n",
            ),
            (
                "use.cpp",
                "#include \"api.h\"\n\nvoid use() {\n    shared();\n}\n",
            ),
            ("impl.cpp", "void shared() {}\n"),
        ];
        let read = |path: &Path| {
            sources
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| text.as_bytes().to_vec())
                .ok_or_else(|| crate::core::Error::git("no such file".to_string()))
        };
        let list = |_: &Path| -> Vec<PathBuf> { Vec::new() };
        let files: Vec<PathBuf> = sources
            .iter()
            .map(|(name, _)| PathBuf::from(name))
            .collect();

        let index = CodeIndex::build(Path::new("."), &files, &read, &list);

        let candidates = |file: &str| -> Vec<String> {
            index
                .files
                .iter()
                .find(|facts| facts.file == file)
                .into_iter()
                .flat_map(|facts| &facts.references)
                .flat_map(|reference| {
                    reference
                        .resolution
                        .candidates
                        .iter()
                        .map(|key| key.to_string())
                })
                .collect()
        };
        assert_eq!(index.files[0].language, crate::core::Language::Cpp);
        assert_eq!(candidates("main.c"), ["api.h#shared"]);
        assert_eq!(candidates("use.cpp"), ["impl.cpp#shared"]);
    }

    #[test]
    fn test_references_share_the_text_of_the_keys_and_the_path() {
        let files = vec![PathBuf::from("a.py"), PathBuf::from("b.py")];
        let sources = [
            ("a.py", "def run():\n    helper()\n    helper()\n"),
            ("b.py", "def helper():\n    return 1\n"),
        ];
        let read = |path: &Path| {
            sources
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| text.as_bytes().to_vec())
                .ok_or_else(|| crate::core::Error::git("no such file".to_string()))
        };
        let list = |_: &Path| -> Vec<PathBuf> { Vec::new() };

        let index = CodeIndex::build(Path::new("."), &files, &read, &list);

        let helper = &index.files[1].symbols[0].key;
        let calls = &index.files[0].references;
        assert_eq!(calls.len(), 2);
        for call in calls {
            assert_eq!(call.resolution.candidates, ["b.py#helper"]);
            assert_eq!(call.resolution.candidates[0].as_ptr(), helper.as_ptr());
        }
        assert_eq!(calls[0].file.as_ptr(), calls[1].file.as_ptr());
        assert_eq!(
            calls[0].enclosing.as_ref().map(|key| key.as_ptr()),
            Some(index.files[0].symbols[0].key.as_ptr())
        );
    }

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
    fn test_index_reads_the_typescript_configs_that_extends_names() {
        // The config package has no source file, thus no source file is in its directory.
        let sources = [
            (
                "packages/app/tsconfig.json",
                "{ \"extends\": \"@acme/config/tsconfig.shared\" }",
            ),
            (
                "packages/config/package.json",
                "{ \"name\": \"@acme/config\" }",
            ),
            (
                "packages/config/tsconfig.shared.json",
                "{ \"extends\": \"./base\" }",
            ),
            // A config that `extends` names can have any name.
            (
                "packages/config/base.json",
                "{ \"compilerOptions\": { \"paths\": { \"@shared/*\": [\"../shared/src/*\"] } } }",
            ),
            (
                "packages/shared/package.json",
                "{ \"name\": \"@acme/shared\" }",
            ),
            ("packages/shared/src/text.ts", "export function trim() {}\n"),
            (
                "packages/app/src/main.ts",
                "import { trim } from \"@shared/text\";\n",
            ),
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
        let files: Vec<PathBuf> = ["packages/app/src/main.ts", "packages/shared/src/text.ts"]
            .iter()
            .map(PathBuf::from)
            .collect();

        let index = CodeIndex::build(Path::new("."), &files, &read, &list);

        assert_eq!(
            index.files[0].imports[0].target_file,
            ImportTarget::Resolved("packages/shared/src/text.ts".to_string())
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

#[cfg(test)]
mod manifest_timing_tests {
    use super::*;
    use crate::index::timing::assert_scales_linearly;
    use std::collections::HashMap;

    /// `size` packages, each in its own parent directory, and a config in
    /// each package that extends a different package that the repository
    /// does not have.
    fn missing_extended_packages_at(size: usize) -> std::time::Duration {
        let mut sources: HashMap<PathBuf, String> = HashMap::new();
        let mut files = Vec::new();
        for position in 0..size {
            let directory = format!("group{position}/package{position}");
            sources.insert(
                PathBuf::from(format!("{directory}/package.json")),
                format!("{{ \"name\": \"@acme/package{position}\" }}"),
            );
            sources.insert(
                PathBuf::from(format!("{directory}/tsconfig.json")),
                format!("{{ \"extends\": \"@acme/missing{position}/tsconfig.json\" }}"),
            );
            let source = PathBuf::from(format!("{directory}/src/index.ts"));
            sources.insert(source.clone(), "export const value = 1;\n".to_string());
            files.push(source);
        }
        let mut listings: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for path in sources.keys() {
            if let Some(parent) = path.parent() {
                listings
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(path.clone());
            }
        }
        let read = |path: &Path| {
            sources
                .get(path)
                .map(|text| text.as_bytes().to_vec())
                .ok_or_else(|| crate::core::Error::git("no such file".to_string()))
        };
        let list = |directory: &Path| listings.get(directory).cloned().unwrap_or_default();
        let started = crate::index::timing::Stopwatch::start();
        let found = manifest_files(Path::new("."), &files, &read, &list);
        let elapsed = started.elapsed();
        assert_eq!(found.len(), 2 * size);
        elapsed
    }

    #[test]
    fn test_missing_extended_packages_are_not_read() {
        missing_extended_packages_at(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_missing_extended_packages_are_probed_in_linear_time() {
        assert_scales_linearly("missing packages", 2_000, missing_extended_packages_at);
    }
}

#[cfg(test)]
mod component_tests {
    use super::*;
    use crate::index::model::{ImportTarget, ResolutionStatus};

    const APP: &str = "<template>\n  <div>\n    <Card :title=\"format(label)\" @click=\"save\" />\n    <li v-for=\"item in items\" :key=\"item.id\">{{ item.name }}</li>\n  </div>\n</template>\n\n<script setup lang=\"ts\">\nimport Card from './Card.vue';\nimport { format } from './util';\nconst props = defineProps<{ a: string }>();\nconst label = 'x';\nconst items = [];\nfunction save() {}\nfunction item() {}\nfunction unused() {}\n</script>\n";
    const CARD: &str = "<template><p>{{ title }}</p></template>\n<script setup lang=\"ts\">\ndefineProps<{ title: string }>();\n</script>\n";
    const UTILITY: &str = "export function format(value: string): string {\n  return value;\n}\n";

    fn index_of(sources: &[(&str, &str)]) -> CodeIndex {
        let read = |path: &Path| {
            sources
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| text.as_bytes().to_vec())
                .ok_or_else(|| crate::core::Error::git("no such file".to_string()))
        };
        let list = |_: &Path| -> Vec<PathBuf> { Vec::new() };
        let files: Vec<PathBuf> = sources
            .iter()
            .map(|(name, _)| PathBuf::from(name))
            .collect();
        CodeIndex::build(Path::new("."), &files, &read, &list)
    }

    fn app() -> CodeIndex {
        index_of(&[
            ("src/App.vue", APP),
            ("src/Card.vue", CARD),
            ("src/util.ts", UTILITY),
        ])
    }

    #[test]
    fn test_a_component_is_in_the_index_with_the_language_of_its_script() {
        let index = app();
        let facts = index
            .files
            .iter()
            .find(|facts| facts.file == "src/App.vue")
            .unwrap();
        assert_eq!(facts.language, Language::TypeScript);
        assert_eq!(facts.component.as_ref().unwrap().abstention, None);
        assert!(facts.is_content_of_facts(APP.as_bytes()));
        let names: Vec<&str> = facts
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect();
        for name in ["save", "item", "unused"] {
            assert!(names.contains(&name), "{names:?}");
        }
    }

    #[test]
    fn test_the_imports_of_a_component_resolve() {
        let index = app();
        let facts = index
            .files
            .iter()
            .find(|facts| facts.file == "src/App.vue")
            .unwrap();
        let targets: Vec<(&str, &ImportTarget)> = facts
            .imports
            .iter()
            .map(|binding| (binding.module_path.as_str(), &binding.target_file))
            .collect();
        assert_eq!(
            targets,
            [
                (
                    "./Card.vue",
                    &ImportTarget::Resolved("src/Card.vue".to_string())
                ),
                ("./util", &ImportTarget::Resolved("src/util.ts".to_string())),
            ]
        );
    }

    #[test]
    fn test_the_template_uses_the_names_of_the_script() {
        let index = app();
        let facts = index
            .files
            .iter()
            .find(|facts| facts.file == "src/App.vue")
            .unwrap();
        let format = facts
            .references
            .iter()
            .find(|reference| reference.name == "format")
            .unwrap();
        assert_eq!(format.resolution.candidates, ["src/util.ts#format"]);
        assert_eq!(format.name_range.start_line, 3);
        assert_eq!(format.enclosing, None);
        let values: Vec<&str> = facts
            .function_references
            .iter()
            .flat_map(|reference| {
                reference
                    .resolution
                    .candidates
                    .iter()
                    .map(|key| key.as_str())
            })
            .collect();
        assert!(values.contains(&"src/App.vue#save"), "{values:?}");
        let all = facts.references.iter().chain(&facts.function_references);
        for reference in all {
            assert!(
                !reference
                    .resolution
                    .candidates
                    .iter()
                    .any(|key| key == "src/App.vue#item"),
                "a `v-for` alias names the function `item`: {reference:?}"
            );
        }
    }

    #[test]
    fn test_a_compiler_macro_is_external() {
        let index = app();
        for file in ["src/App.vue", "src/Card.vue"] {
            let facts = index.files.iter().find(|facts| facts.file == file).unwrap();
            let macro_call = facts
                .references
                .iter()
                .find(|reference| reference.name == "defineProps")
                .unwrap();
            assert_eq!(
                macro_call.resolution.status,
                ResolutionStatus::External,
                "{file}"
            );
        }
    }

    #[test]
    fn test_a_compiler_macro_is_not_external_outside_a_component() {
        let index = index_of(&[("src/a.ts", "defineProps();\n")]);
        let call = &index.files[0].references[0];
        assert_ne!(call.resolution.status, ResolutionStatus::External);
    }
}
