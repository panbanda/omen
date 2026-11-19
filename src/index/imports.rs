//! Import resolution. A module map for each language gives the file to which
//! an import binding refers.
//!
//! No rule selects the first of several candidates. Two or more modules that
//! match one import path give [`ImportTarget::Ambiguous`]. The files of one
//! package give [`ImportTarget::Package`], which is a certain target.

pub mod cargo;
pub mod typescript;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use parking_lot::RwLock;
use rayon::prelude::*;

use super::extract::FileSymbols;
use super::model::{ImportBinding, ImportKind, ImportTarget, SymbolKind};
use crate::core::Language;

/// The end of the name of a Go test file.
const GO_TEST_SUFFIX: &str = "_test.go";

/// The facts of the repository that the module maps use.
struct Repository<'a> {
    files: &'a [FileSymbols],
    paths: BTreeSet<&'a str>,
    manifests: &'a Manifests,
    /// The position of each file, by its path.
    by_path: HashMap<&'a str, usize>,
    /// The files whose path is the key or ends with `/` and the key.
    by_suffix: HashMap<&'a str, Vec<usize>>,
    /// The files whose directory is the key or ends with `/` and the key.
    by_directory_suffix: HashMap<&'a str, Vec<usize>>,
    /// The files that declare the package or the namespace of the key.
    by_module: HashMap<&'a str, Vec<usize>>,
    /// The files that the manifests make the root of a Rust crate.
    rust_roots: BTreeSet<String>,
    /// The files of `rust_roots` that declare a child module with `mod name;`,
    /// by their directory and the name of the module. A directory can have
    /// many roots, thus a search through them for each module costs too much.
    rust_roots_by_child: HashMap<(String, String), Vec<String>>,
    /// The root file of the library of each Rust package, by the directory of the package.
    rust_libraries: HashMap<&'a str, String>,
    /// The directory of a dependency whose library has a name that is not
    /// the key of the dependency, as `[lib] name` gives. The key is the
    /// directory of the manifest and the name of the library.
    rust_library_names: HashMap<(&'a str, &'a str), String>,
    /// The modules that a Rust file defines itself, outside each other
    /// module: the file and the name of the module.
    rust_inline_modules: HashSet<(&'a str, &'a str)>,
    /// The Rust files and the names of the child modules that they declare
    /// with `mod name;`.
    rust_module_declarations: HashSet<(&'a str, &'a str)>,
    /// The results of `rust_parent`. Each import with `super` or `crate`
    /// asks for the parent of its file.
    rust_parents: RwLock<HashMap<String, Option<String>>>,
    /// The options of each TypeScript project file after `extends`.
    project_options: RwLock<HashMap<String, std::sync::Arc<typescript::ProjectOptions>>>,
    inherited_options: typescript::InheritedMemo,
}

/// The module tree of the Rust files of an index: which file is the root of a crate, the
/// parent of a module, and the child module of a name. The index gives `use` and `mod`
/// bindings the same tree.
pub struct RustModules<'a> {
    repository: Repository<'a>,
}

impl<'a> RustModules<'a> {
    pub fn new(files: &'a [FileSymbols], manifests: &'a Manifests) -> Self {
        Self {
            repository: Repository::new(files, manifests),
        }
    }

    /// The file of the root module of the crate that holds `file`.
    pub fn root(&self, file: &str) -> Option<String> {
        self.repository.rust_root(file)
    }

    /// The file of the module that declares the module in `file`.
    pub fn parent(&self, file: &str) -> Option<String> {
        self.repository.rust_parent(file)
    }

    /// The file of the child module `name` that `mod name;` in `file` declares.
    pub fn child(&self, file: &str, name: &str) -> Option<String> {
        self.repository
            .rust_child(&self.repository.rust_children(file), name)
    }
}

/// The text and each part of the text that starts after a `/`.
fn suffixes(path: &str) -> impl Iterator<Item = &str> {
    std::iter::once(path).chain(
        path.match_indices('/')
            .map(move |(position, _)| &path[position + 1..]),
    )
}

impl<'a> Repository<'a> {
    /// A repository can have many thousands of files and imports. The maps
    /// let each import find its files without a search through all files.
    fn new(files: &'a [FileSymbols], manifests: &'a Manifests) -> Self {
        let mut by_path = HashMap::new();
        let mut by_suffix: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut by_directory_suffix: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut by_module: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut rust_inline_modules = HashSet::new();
        let mut rust_module_declarations = HashSet::new();
        for (position, file) in files.iter().enumerate() {
            by_path.insert(file.file.as_str(), position);
            for suffix in suffixes(&file.file) {
                by_suffix.entry(suffix).or_default().push(position);
            }
            for suffix in suffixes(directory(&file.file)) {
                by_directory_suffix
                    .entry(suffix)
                    .or_default()
                    .push(position);
            }
            for module in &file.modules {
                by_module.entry(module.as_str()).or_default().push(position);
            }
            if file.language == Language::Rust {
                rust_inline_modules.extend(
                    file.symbols
                        .iter()
                        .filter(|symbol| {
                            symbol.kind == SymbolKind::Module && symbol.container.is_none()
                        })
                        .map(|symbol| (file.file.as_str(), symbol.name.as_str())),
                );
                rust_module_declarations.extend(
                    file.imports
                        .iter()
                        .filter(|binding| binding.kind == ImportKind::ModuleDeclaration)
                        .filter_map(|binding| binding.local_name.as_deref())
                        .map(|child| (file.file.as_str(), child)),
                );
            }
        }
        let mut repository = Self {
            files,
            paths: files.iter().map(|file| file.file.as_str()).collect(),
            manifests,
            by_path,
            by_suffix,
            by_directory_suffix,
            by_module,
            rust_roots: BTreeSet::new(),
            rust_roots_by_child: HashMap::new(),
            rust_libraries: HashMap::new(),
            rust_library_names: HashMap::new(),
            rust_inline_modules,
            rust_module_declarations,
            rust_parents: RwLock::new(HashMap::new()),
            project_options: RwLock::new(HashMap::new()),
            inherited_options: RwLock::new(HashMap::new()),
        };
        repository.read_rust_packages();
        repository
    }

    /// Finds the root files of the crates of each package: the library, the
    /// targets that have a `path` key, the build script, `src/main.rs`, and
    /// each file of a directory in which `cargo` makes a target of each file.
    fn read_rust_packages(&mut self) {
        let manifests = self.manifests;
        let mut roots = BTreeSet::new();
        for (here, package) in &manifests.rust_packages {
            if package.library_name.is_none() {
                continue;
            }
            let library = package
                .library_path
                .clone()
                .or_else(|| join(here, "src/lib.rs"));
            if let Some(library) = self.existing(library) {
                self.rust_libraries.insert(here.as_str(), library.clone());
                roots.insert(library);
            }
            for path in &package.target_paths {
                roots.extend(self.existing(Some(path.clone())));
            }
            roots.extend(self.existing(join(here, "src/main.rs")));
            for targets in &package.target_directories {
                roots.extend(
                    self.in_directories(targets)
                        .filter(|file| {
                            file.language == Language::Rust && directory(&file.file) == targets
                        })
                        .map(|file| file.file.clone()),
                );
            }
        }
        for (here, package) in &manifests.rust_packages {
            for (key, dependency) in &package.dependencies {
                // A dependency with a `package` key has the name of its key.
                if dependency.package != *key {
                    continue;
                }
                let cargo::CrateSource::Repository(target) = manifests.rust_crate_in(here, key)
                else {
                    continue;
                };
                let library = manifests
                    .rust_packages
                    .get(&target)
                    .and_then(|package| package.library_name.as_deref());
                if let Some(library) = library.filter(|library| *library != key.as_str()) {
                    self.rust_library_names
                        .insert((here.as_str(), library), target);
                }
            }
        }
        for root in &roots {
            let Some(position) = self.by_path.get(root.as_str()) else {
                continue;
            };
            let declared = self.files[*position]
                .imports
                .iter()
                .filter(|binding| binding.kind == ImportKind::ModuleDeclaration)
                .filter_map(|binding| binding.local_name.clone());
            for child in declared {
                self.rust_roots_by_child
                    .entry((directory(root).to_string(), child))
                    .or_default()
                    .push(root.clone());
            }
        }
        self.rust_roots = roots;
    }

    /// The files in the directories whose path is `suffix` or ends with
    /// `/suffix`, in path order.
    fn in_directories(&self, suffix: &str) -> impl Iterator<Item = &'a FileSymbols> + '_ {
        let files = self.files;
        self.by_directory_suffix
            .get(suffix)
            .into_iter()
            .flatten()
            .map(move |position| &files[*position])
    }
}

/// The facts from the files of a repository that are not source files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifests {
    /// The module path of each `go.mod` file, by the directory of the file.
    pub go_modules: BTreeMap<String, String>,
    /// The directories in which Ruby `require` looks for a file.
    pub ruby_load_roots: BTreeSet<String>,
    /// The facts of each `Cargo.toml` file, by the directory of the file.
    pub rust_packages: BTreeMap<String, cargo::Package>,
    /// The directories of the Rust packages, by the name of their library.
    pub rust_library_directories: BTreeMap<String, BTreeSet<String>>,
    /// The packages that the `package.json` files of the repository name,
    /// sorted by the name.
    pub node_packages: Vec<NodePackage>,
    /// The project files of TypeScript and JavaScript.
    pub projects: typescript::ProjectConfigs,
    /// The test runners that each `package.json` file depends on, by the
    /// directory of the file.
    pub test_runners: BTreeMap<String, TestRunners>,
}

/// A set of the test runners of ECMAScript that the index knows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TestRunners(u8);

impl TestRunners {
    pub const JEST: Self = Self(1);
    pub const VITEST: Self = Self(2);
    pub const MOCHA: Self = Self(4);

    /// The runners that the dependencies of a `package.json` text name.
    pub fn of_package(package_json: &str) -> Self {
        let Ok(package) = serde_json::from_str::<serde_json::Value>(package_json) else {
            return Self::default();
        };
        let mut runners = Self::default();
        for field in ["dependencies", "devDependencies"] {
            let Some(dependencies) = package.get(field).and_then(|value| value.as_object()) else {
                continue;
            };
            for (name, runner) in [
                ("jest", Self::JEST),
                ("vitest", Self::VITEST),
                ("mocha", Self::MOCHA),
            ] {
                if dependencies.contains_key(name) {
                    runners = runners.union(runner);
                }
            }
        }
        runners
    }

    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn contains(self, runner: Self) -> bool {
        self.0 & runner.0 == runner.0
    }

    /// The runners for `file`: the runners of each `package.json` in the
    /// directory of the file and in each directory above it.
    pub fn of_file(manifests: &Manifests, file: &str) -> Self {
        let mut runners = Self::default();
        let mut here = directory(file);
        loop {
            if let Some(found) = manifests.test_runners.get(here) {
                runners = runners.union(*found);
            }
            if here.is_empty() {
                return runners;
            }
            here = directory(here);
        }
    }
}

/// The facts of one `package.json` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePackage {
    pub name: String,
    /// The directory of the `package.json` file.
    pub directory: String,
    /// The files that each entry of `exports` names, by the subpath of the
    /// entry. `.` is the package itself. An entry can name one file for each
    /// condition. Without `exports`, `.` has the files of `main`, `module`,
    /// and `types`.
    pub exports: BTreeMap<String, BTreeSet<String>>,
    /// The `package.json` file has `exports`. Then a subpath that no entry
    /// names is not a file of the package.
    pub has_exports: bool,
    /// The keys of `exports` with `*`, with the files of each key.
    pub export_patterns: std::sync::Arc<typescript::PatternTable<BTreeSet<String>>>,
    /// The config that `extends` with the name of the package names: the
    /// `tsconfig` field, else `tsconfig.json`. The path is relative to the
    /// directory of the package.
    pub tsconfig: String,
}

/// Adds each string in an `exports` value to `out`. A value is a path or an
/// object that has one value for each condition.
fn export_paths(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::String(path) => {
            out.insert(path.clone());
        }
        serde_json::Value::Object(conditions) => {
            for condition in conditions.values() {
                export_paths(condition, out);
            }
        }
        serde_json::Value::Array(choices) => {
            for choice in choices {
                export_paths(choice, out);
            }
        }
        _ => {}
    }
}

/// Reads the name and the exports of a `package.json` file in `directory`.
pub fn node_package(directory: &str, package_json: &str) -> Option<NodePackage> {
    let package: serde_json::Value = serde_json::from_str(package_json).ok()?;
    let name = package.get("name")?.as_str()?.to_string();
    let mut exports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    match package.get("exports") {
        Some(serde_json::Value::Object(entries))
            if entries.keys().all(|key| key.starts_with('.')) =>
        {
            for (subpath, value) in entries {
                export_paths(value, exports.entry(subpath.clone()).or_default());
            }
        }
        Some(value) => export_paths(value, exports.entry(".".to_string()).or_default()),
        None => {
            for field in ["main", "module", "types"] {
                if let Some(value) = package.get(field) {
                    export_paths(value, exports.entry(".".to_string()).or_default());
                }
            }
        }
    }
    let export_patterns = std::sync::Arc::new(typescript::PatternTable::new(
        exports
            .iter()
            .filter(|(key, _)| key.contains('*'))
            .map(|(key, files)| (key.clone(), files.clone())),
    ));
    Some(NodePackage {
        name,
        directory: directory.to_string(),
        exports,
        has_exports: package.get("exports").is_some(),
        export_patterns,
        tsconfig: package
            .get("tsconfig")
            .and_then(|value| value.as_str())
            .unwrap_or("tsconfig.json")
            .to_string(),
    })
}

/// The entries of the `exports` key with `*` that matches `subpath`, with the
/// matched text for each `*`. Of the keys that match, the key with the
/// longest text before `*` applies, as in Node.js.
fn export_pattern(package: &NodePackage, subpath: &str) -> Vec<String> {
    let Some((entries, matched, _)) = package.export_patterns.find_wildcard(subpath) else {
        return Vec::new();
    };
    entries
        .iter()
        .map(|entry| entry.replace('*', matched))
        .collect()
}

impl Default for Manifests {
    /// Ruby projects put library files in `lib`, and tools add the root.
    fn default() -> Self {
        Self {
            go_modules: BTreeMap::new(),
            ruby_load_roots: [String::new(), "lib".to_string()].into(),
            rust_packages: BTreeMap::new(),
            rust_library_directories: BTreeMap::new(),
            node_packages: Vec::new(),
            projects: typescript::ProjectConfigs::default(),
            test_runners: BTreeMap::new(),
        }
    }
}

impl Manifests {
    /// Reads a `go.mod`, `.gemspec`, `Cargo.toml`, `package.json`, or TypeScript project file.
    /// `path` is relative to the root of the repository. A different file has
    /// no effect.
    /// Adds a TypeScript config whatever its name, as for a config that
    /// `extends` names.
    pub fn add_project(&mut self, path: &str, text: &str) {
        if let Some(config) = typescript::project_config(text) {
            self.projects.insert(path.to_string(), config);
        }
    }

    pub fn add(&mut self, path: &str, text: &str) {
        let here = directory(path);
        if typescript::is_project_file(file_name(path)) {
            if let Some(config) = typescript::project_config(text) {
                self.projects.insert(path.to_string(), config);
            }
        } else if file_name(path) == "package.json" {
            let runners = TestRunners::of_package(text);
            if runners != TestRunners::default() {
                self.test_runners.insert(here.to_string(), runners);
            }
            self.node_packages.extend(node_package(here, text));
            self.node_packages.sort_by(|left, right| {
                (&left.name, &left.directory).cmp(&(&right.name, &right.directory))
            });
        } else if file_name(path) == "go.mod" {
            self.go_modules
                .extend(go_module_path(text).map(|module| (here.to_string(), module)));
        } else if path.ends_with(".gemspec") {
            for root in ruby_require_paths(text) {
                self.ruby_load_roots.extend(join(here, &root));
            }
        } else if file_name(path) == "Cargo.toml" {
            let Some(package) = cargo::package(here, text) else {
                return;
            };
            if let Some(library) = &package.library_name {
                self.rust_library_directories
                    .entry(library.clone())
                    .or_default()
                    .insert(here.to_string());
            }
            self.rust_packages.insert(here.to_string(), package);
        }
    }

    /// The source of the crate that Rust source in `file` calls `name`.
    pub fn rust_crate(&self, file: &str, name: &str) -> cargo::CrateSource {
        self.rust_crate_in(directory(file), name)
    }

    /// The source of the crate that Rust source in `directory` calls `name`.
    fn rust_crate_in(&self, directory: &str, name: &str) -> cargo::CrateSource {
        cargo::crate_source(
            &self.rust_packages,
            &self.rust_library_directories,
            directory,
            name,
        )
    }

    /// The directory of the manifest nearest to `file`.
    fn rust_manifest_directory(&self, file: &str) -> Option<&str> {
        let mut here = directory(file);
        loop {
            if let Some((found, _)) = self.rust_packages.get_key_value(here) {
                return Some(found.as_str());
            }
            if here.is_empty() {
                return None;
            }
            here = directory(here);
        }
    }

    /// Tells if the `Cargo.toml` file nearest to `file` declares a dependency
    /// that Rust source calls `name`, and that is not a crate of the repository.
    pub fn declares_rust_dependency(&self, file: &str, name: &str) -> bool {
        self.rust_crate(file, name) == cargo::CrateSource::External
    }
}

/// The `require_paths` of a gem specification. Without the assignment, the
/// value is `lib`. If the value is not a list of string literals, the result is empty.
pub fn ruby_require_paths(gemspec: &str) -> Vec<String> {
    let Some((_, after)) = gemspec.split_once("require_paths") else {
        return vec!["lib".to_string()];
    };
    let literal = after
        .trim_start()
        .strip_prefix('=')
        .map(str::trim_start)
        .and_then(|value| value.strip_prefix('['))
        .and_then(|list| list.split_once(']'));
    let Some((list, _)) = literal else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for item in list.split(',') {
        let item = item.trim();
        let quoted = ['"', '\'']
            .iter()
            .find_map(|quote| item.strip_prefix(*quote)?.strip_suffix(*quote));
        match quoted {
            Some(path) if !path.contains(['#', '"', '\'']) => paths.push(path.to_string()),
            _ => return Vec::new(),
        }
    }
    paths
}

/// The module path that a `go.mod` file declares.
pub fn go_module_path(go_mod: &str) -> Option<String> {
    go_mod.lines().find_map(|line| {
        let name = line.trim().strip_prefix("module")?;
        if !name.starts_with(char::is_whitespace) {
            return None;
        }
        let name = name.trim().trim_matches('"');
        (!name.is_empty()).then(|| name.to_string())
    })
}

fn directory(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(directory, _)| directory)
}

fn file_name(file: &str) -> &str {
    file.rsplit_once('/').map_or(file, |(_, name)| name)
}

/// Joins a relative path to a directory and removes `.` and `..`. The result
/// is `None` if the path goes out of the repository.
fn join(directory: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<&str> = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

fn target_of(candidates: BTreeSet<String>, not_found: ImportTarget) -> ImportTarget {
    let mut candidates: Vec<String> = candidates.into_iter().collect();
    match candidates.len() {
        0 => not_found,
        1 => ImportTarget::Resolved(candidates.remove(0)),
        _ => ImportTarget::Ambiguous(candidates),
    }
}

/// The target for the files of one package. The set must not be empty.
fn package_of(files: BTreeSet<String>) -> ImportTarget {
    let mut files: Vec<String> = files.into_iter().collect();
    if files.len() == 1 {
        ImportTarget::Resolved(files.remove(0))
    } else {
        ImportTarget::Package(files)
    }
}

/// The target for a path that can refer to each of these packages.
fn one_package(mut packages: Vec<BTreeSet<String>>, not_found: ImportTarget) -> ImportTarget {
    packages.retain(|files| !files.is_empty());
    match packages.len() {
        0 => not_found,
        1 => package_of(packages.remove(0)),
        _ => ImportTarget::Ambiguous(packages.into_iter().flatten().collect()),
    }
}

impl Repository<'_> {
    fn existing(&self, path: Option<String>) -> Option<String> {
        path.filter(|path| self.paths.contains(path.as_str()))
    }

    /// The files whose path is `suffix` or ends with `/suffix`.
    fn with_suffix(&self, suffix: &str, languages: &[Language]) -> BTreeSet<String> {
        self.by_suffix
            .get(suffix)
            .into_iter()
            .flatten()
            .map(|position| &self.files[*position])
            .filter(|file| languages.contains(&file.language))
            .map(|file| file.file.clone())
            .collect()
    }

    fn go(&self, binding: &ImportBinding) -> ImportTarget {
        let here = directory(&binding.file);
        let own = self
            .manifests
            .go_modules
            .iter()
            .filter(|(root, _)| {
                root.is_empty() || here == root.as_str() || here.starts_with(&format!("{root}/"))
            })
            .max_by_key(|(root, _)| root.len());
        let Some((own_root, _)) = own else {
            return ImportTarget::Unresolved;
        };
        let path = &binding.module_path;
        let importer_is_test = binding.file.ends_with(GO_TEST_SUFFIX);
        // The files of the package directory, for a module whose path starts the import path.
        let package = |root: &String, name: &String| -> Option<BTreeSet<String>> {
            let directory_of_package = if path == name {
                root.clone()
            } else {
                join(root, path.strip_prefix(&format!("{name}/"))?)?
            };
            // A `_test.go` file is a part of the package only for the tests
            // of the same directory, and a `_test` package is never a part of it.
            let sees_tests = importer_is_test && here == directory_of_package;
            let in_package = |file: &FileSymbols| {
                !file.file.ends_with(GO_TEST_SUFFIX)
                    || (sees_tests
                        && !file
                            .modules
                            .first()
                            .is_some_and(|package| package.ends_with("_test")))
            };
            Some(
                self.in_directories(&directory_of_package)
                    .filter(|file| {
                        file.language == Language::Go
                            && directory(&file.file) == directory_of_package
                            && in_package(file)
                    })
                    .map(|file| file.file.clone())
                    .collect(),
            )
        };
        let matches: Vec<(&String, BTreeSet<String>)> = self
            .manifests
            .go_modules
            .iter()
            .filter_map(|(root, name)| Some((root, package(root, name)?)))
            .collect();
        if matches.is_empty() {
            return ImportTarget::External;
        }
        // The module of the importing file wins. If not, two modules of the
        // repository that declare the same module path are ambiguous.
        let own_package = matches
            .iter()
            .find(|(root, files)| *root == own_root && !files.is_empty());
        match own_package {
            Some((_, files)) => package_of(files.clone()),
            None => one_package(
                matches.into_iter().map(|(_, files)| files).collect(),
                ImportTarget::Unresolved,
            ),
        }
    }

    /// The Python files of each directory that the dotted name `segments`
    /// names. A directory without `__init__.py` is a namespace package.
    fn python_directories(&self, base: Option<&str>, segments: &[&str]) -> Vec<BTreeSet<String>> {
        let module = segments.join("/");
        let exact = base.map(|base| join(base, &module));
        let suffix = match &exact {
            Some(Some(path)) => path.as_str(),
            Some(None) => return Vec::new(),
            None => module.as_str(),
        };
        let mut directories: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        for file in self
            .in_directories(suffix)
            .filter(|file| file.language == Language::Python)
        {
            let here = directory(&file.file);
            let named = exact.is_none() || here == suffix;
            if named && !module.is_empty() {
                directories
                    .entry(here)
                    .or_default()
                    .insert(file.file.clone());
            }
        }
        directories.into_values().collect()
    }

    /// The files of the Python module `segments` below `base`. If `base` is
    /// `None`, the module can be below each directory.
    fn python_module(&self, base: Option<&str>, segments: &[&str]) -> BTreeSet<String> {
        let module = segments.join("/");
        let forms = if module.is_empty() {
            vec!["__init__.py".to_string()]
        } else {
            vec![format!("{module}.py"), format!("{module}/__init__.py")]
        };
        let files = |stub: &str| {
            let mut found = BTreeSet::new();
            for form in &forms {
                let form = format!("{form}{stub}");
                match base {
                    Some(base) => found.extend(self.existing(join(base, &form))),
                    None => found.extend(self.with_suffix(&form, &[Language::Python])),
                }
            }
            found
        };
        // A stub file `name.pyi` is the module when no source file exists,
        // as it is for an extension module.
        let found = files("");
        if found.is_empty() {
            files("i")
        } else {
            found
        }
    }

    fn python(&self, binding: &ImportBinding) -> ImportTarget {
        let path = binding.module_path.as_str();
        let dots = path.len() - path.trim_start_matches('.').len();
        let mut segments: Vec<&str> = path[dots..].split('.').filter(|s| !s.is_empty()).collect();
        let base = if dots == 0 {
            None
        } else {
            let up = "../".repeat(dots - 1);
            match join(directory(&binding.file), &up) {
                Some(base) => Some(base),
                None => return ImportTarget::Unresolved,
            }
        };
        let not_found = if dots == 0 {
            ImportTarget::External
        } else {
            ImportTarget::Unresolved
        };
        // `from package import name` refers to the module `package.name` if that module exists.
        let from_import = matches!(binding.kind, ImportKind::Named);
        if let (true, Some(name)) = (from_import, binding.imported_name.as_deref()) {
            segments.push(name);
            let submodule = self.python_module(base.as_deref(), &segments);
            if !submodule.is_empty() {
                return target_of(submodule, not_found);
            }
            segments.pop();
        }
        let module = self.python_module(base.as_deref(), &segments);
        if !module.is_empty() {
            return target_of(module, not_found);
        }
        one_package(
            self.python_directories(base.as_deref(), &segments),
            not_found,
        )
    }

    fn ecma(&self, binding: &ImportBinding) -> ImportTarget {
        let path = binding.module_path.as_str();
        if path.starts_with('.') {
            return match join(directory(&binding.file), path) {
                Some(base) => target_of(self.ecma_files(&base), ImportTarget::Unresolved),
                None => ImportTarget::Unresolved,
            };
        }
        // TypeScript reads `paths` and then `baseUrl` of the project file
        // of the importing file before it looks for a package.
        let mut mapped_in_repository = false;
        if let Some(options) = self.project_options_of(&binding.file) {
            let (paths, catch_all) = options.mapped(path);
            mapped_in_repository = !paths.is_empty() && !catch_all;
            for mapped in paths {
                let found = self.ecma_files(&mapped);
                if !found.is_empty() {
                    return target_of(found, ImportTarget::Unresolved);
                }
            }
            if let Some(base) = options
                .base_url
                .as_deref()
                .and_then(|base| join(base, path))
            {
                let found = self.ecma_files(&base);
                if !found.is_empty() {
                    return target_of(found, ImportTarget::Unresolved);
                }
            }
        }
        // A package of the repository is not external. The `exports` of
        // its `package.json` name the file. An entry can name a source
        // file and its build outputs. The files that are in the
        // repository are the candidates.
        let mut own_package = false;
        let mut found = BTreeSet::new();
        for (package, rest) in typescript::packages_of(&self.manifests.node_packages, path) {
            let subpath = format!(".{rest}");
            own_package = true;
            found.extend(self.package_files(package, &subpath));
        }
        // A module that `paths` maps into the repository is not external,
        // also when the index does not have the file.
        if own_package {
            target_of(found, ImportTarget::Unresolved)
        } else if mapped_in_repository {
            ImportTarget::Unresolved
        } else {
            ImportTarget::External
        }
    }

    /// The files that a module path names, as a relative import does: the
    /// file itself, the file with an extension, or the index file of the
    /// directory. TypeScript source can name the output file `./a.js` of
    /// the source file `./a.ts`.
    fn ecma_files(&self, base: &str) -> BTreeSet<String> {
        if let Some(exact) = self.existing(Some(base.to_string())) {
            return BTreeSet::from([exact]);
        }
        let mut found = BTreeSet::new();
        for extension in typescript::SOURCE_EXTENSIONS {
            found.extend(self.existing(Some(format!("{base}.{extension}"))));
            found.extend(self.existing(Some(format!("{base}/index.{extension}"))));
        }
        for (output, sources) in [
            ("js", ["ts", "tsx"]),
            ("mjs", ["mts", "mts"]),
            ("cjs", ["cts", "cts"]),
        ] {
            if let Some(stem) = base.strip_suffix(&format!(".{output}")) {
                for source in sources {
                    found.extend(self.existing(Some(format!("{stem}.{source}"))));
                }
            }
        }
        found
    }

    /// The options of the project file that applies to `file`.
    fn project_options_of(&self, file: &str) -> Option<std::sync::Arc<typescript::ProjectOptions>> {
        let projects = &self.manifests.projects;
        let config = projects.nearest(file)?;
        Some(self.project_options(config))
    }

    fn project_options(&self, config: &str) -> std::sync::Arc<typescript::ProjectOptions> {
        if let Some(found) = self.project_options.read().get(config) {
            return std::sync::Arc::clone(found);
        }
        let options = std::sync::Arc::new(self.manifests.projects.options(
            config,
            &self.manifests.node_packages,
            &self.inherited_options,
        ));
        self.project_options
            .write()
            .insert(config.to_string(), std::sync::Arc::clone(&options));
        options
    }

    /// The files of `package` that the subpath names. An entry of `exports`
    /// with `*` names each subpath that matches it. A package without
    /// `exports` gives each file of its directory, as a relative import does.
    ///
    /// A clone of a repository does not have the build output, thus an entry
    /// can name a file that is not in the repository. Then the source file
    /// of the output is the target. These rules give the source file, and
    /// the first rule that gives a file applies:
    ///
    /// 1. A project file in the directory of the package has `outDir` and
    ///    `rootDir`. The output `outDir/a.js` has the source `rootDir/a.ts`.
    /// 2. The output `dist/a.js` of the package has the source `src/a.ts`.
    fn package_files(&self, package: &NodePackage, subpath: &str) -> BTreeSet<String> {
        let mut entries: Vec<String> = package
            .exports
            .get(subpath)
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        // An exact key applies before a pattern, also with `null`.
        if !package.exports.contains_key(subpath) {
            entries = export_pattern(package, subpath);
        }
        let legacy = entries.is_empty() && !package.has_exports && subpath != ".";
        let outputs: Vec<String> = if legacy {
            join(&package.directory, subpath).into_iter().collect()
        } else {
            entries
                .iter()
                .filter_map(|entry| join(&package.directory, entry))
                .collect()
        };
        let found: BTreeSet<String> = if legacy {
            outputs
                .iter()
                .flat_map(|base| self.ecma_files(base))
                .collect()
        } else {
            outputs
                .iter()
                .filter_map(|output| self.existing(Some(output.clone())))
                .collect()
        };
        if !found.is_empty() {
            return found;
        }
        let configured: BTreeSet<String> = outputs
            .iter()
            .flat_map(|output| self.source_by_project(package, output))
            .collect();
        if !configured.is_empty() {
            return configured;
        }
        outputs
            .iter()
            .flat_map(|output| self.source_by_location(package, output))
            .collect()
    }

    /// The source files of a build output by rule 1 of `package_files`.
    fn source_by_project(&self, package: &NodePackage, output: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        for config in self.manifests.projects.in_directory(&package.directory) {
            let options = self.project_options(config);
            let (Some(out_dir), Some(root_dir)) = (&options.out_dir, &options.root_dir) else {
                continue;
            };
            // An empty directory is the root of the repository.
            let rest = match out_dir.as_str() {
                "" => Some(output),
                out_dir => output
                    .strip_prefix(out_dir)
                    .and_then(|rest| rest.strip_prefix('/')),
            };
            let Some(source) = rest.and_then(|rest| join(root_dir, rest)) else {
                continue;
            };
            found.extend(self.sources_of(&source));
        }
        found
    }

    /// The source files of a build output by rule 2 of `package_files`.
    fn source_by_location(&self, package: &NodePackage, output: &str) -> BTreeSet<String> {
        let Some(rest) = output
            .strip_prefix(package.directory.as_str())
            .map(|rest| rest.trim_start_matches('/'))
            .and_then(|rest| rest.strip_prefix("dist/"))
        else {
            return BTreeSet::new();
        };
        join(&package.directory, &format!("src/{rest}"))
            .map(|source| self.sources_of(&source))
            .unwrap_or_default()
    }

    /// The source files that can give the build output at `output`.
    fn sources_of(&self, output: &str) -> BTreeSet<String> {
        let (stem, had_extension) = typescript::output_stem(output);
        typescript::source_candidates(stem, had_extension)
            .into_iter()
            .filter_map(|candidate| self.existing(Some(candidate)))
            .collect()
    }

    /// The file of the Rust module `name` whose parent has its children in `children`.
    fn rust_child(&self, children: &str, name: &str) -> Option<String> {
        let flat = self.existing(join(children, &format!("{name}.rs")));
        let nested = self.existing(join(children, &format!("{name}/mod.rs")));
        match (flat, nested) {
            (Some(file), None) | (None, Some(file)) => Some(file),
            _ => None,
        }
    }

    /// The directory that has the child modules of the module in `file`. The
    /// root file of a crate has its child modules in its own directory, for
    /// each name of the root file.
    fn rust_children(&self, file: &str) -> String {
        let here = directory(file);
        let stem = file_name(file).trim_end_matches(".rs");
        if ["main", "lib", "mod"].contains(&stem) || self.rust_roots.contains(file) {
            here.to_string()
        } else {
            join(here, stem).unwrap_or_default()
        }
    }

    /// The file of the module that declares the module in `file`.
    fn rust_parent(&self, file: &str) -> Option<String> {
        if let Some(found) = self.rust_parents.read().get(file) {
            return found.clone();
        }
        let found = self.find_rust_parent(file);
        self.rust_parents
            .write()
            .insert(file.to_string(), found.clone());
        found
    }

    fn find_rust_parent(&self, file: &str) -> Option<String> {
        if self.rust_roots.contains(file) {
            return None;
        }
        let here = directory(file);
        let stem = file_name(file).trim_end_matches(".rs");
        // The module that declares the child module `child` in `children`.
        let owner = |children: &str, child: &str| -> Option<String> {
            let mut found: BTreeSet<String> = ["main.rs", "lib.rs", "mod.rs"]
                .iter()
                .filter_map(|root| self.existing(join(children, root)))
                .collect();
            let named_roots = self
                .rust_roots_by_child
                .get(&(children.to_string(), child.to_string()));
            found.extend(named_roots.into_iter().flatten().cloned());
            if let Some((above, name)) = children.rsplit_once('/') {
                found.extend(self.existing(join(above, &format!("{name}.rs"))));
            } else if !children.is_empty() {
                found.extend(self.existing(Some(format!("{children}.rs"))));
            }
            // A directory can have two or more roots. The root that has
            // `mod child;` is the parent.
            if found.len() > 1 {
                found.retain(|candidate| {
                    self.rust_module_declarations
                        .contains(&(candidate.as_str(), child))
                });
            }
            let mut found = found.into_iter();
            match (found.next(), found.next()) {
                (Some(only), None) => Some(only),
                _ => None,
            }
        };
        match stem {
            "main" | "lib" => None,
            "mod" if here.is_empty() => None,
            "mod" => owner(directory(here), file_name(here)),
            _ => owner(here, stem),
        }
    }

    fn rust_root(&self, file: &str) -> Option<String> {
        let mut current = file.to_string();
        let mut steps = 0;
        while let Some(parent) = self.rust_parent(&current) {
            current = parent;
            steps += 1;
            if steps > 64 {
                return None;
            }
        }
        let is_root = ["main.rs", "lib.rs"].contains(&file_name(&current))
            || self.rust_roots.contains(current.as_str());
        is_root.then_some(current)
    }

    fn rust(&self, binding: &ImportBinding) -> ImportTarget {
        // `mod name;` declares a child of the module of the file. A crate with the same
        // name is a different module.
        if binding.kind == ImportKind::ModuleDeclaration {
            return self
                .rust_child(&self.rust_children(&binding.file), &binding.module_path)
                .map_or(ImportTarget::Unresolved, ImportTarget::Resolved);
        }
        match self.rust_walk(&binding.file, &binding.module_path) {
            None => ImportTarget::Unresolved,
            Some((module, true, _)) => ImportTarget::Resolved(module),
            Some(_) => ImportTarget::External,
        }
    }

    /// The file of the module that the Rust path `path` names in `file`.
    /// The path must name a module with each segment.
    fn rust_module(&self, file: &str, path: &str) -> Option<String> {
        match self.rust_walk(file, path) {
            Some((module, true, true)) => Some(module),
            _ => None,
        }
    }

    /// Goes from the module of `file` along the segments of `path`. The
    /// result is the last module file, a flag that tells if a segment named a
    /// module, and a flag that tells if each segment named a module. `None`
    /// means that the path names a crate root or a parent that has no file.
    fn rust_walk(&self, file: &str, path: &str) -> Option<(String, bool, bool)> {
        let mut segments = path.split("::").peekable();
        let mut module = file.to_string();
        let mut moved = false;
        match segments.peek().copied() {
            Some("crate") => {
                module = self.rust_root(file)?;
                moved = true;
                segments.next();
            }
            Some("self") => {
                moved = true;
                segments.next();
            }
            // A module in the file is not a module file. The path stays
            // in the file, and no segment after it names a file.
            Some(first) if self.rust_inline_modules.contains(&(file, first)) => {
                return Some((module, true, false));
            }
            // A name that is not a child module of the file can be the name
            // of a crate. The manifest of the file tells which crate it is.
            Some(first) if self.rust_child(&self.rust_children(file), first).is_none() => {
                match self.manifests.rust_crate(file, first) {
                    cargo::CrateSource::Repository(package) => {
                        // Without the library file, the path has no target.
                        module = self.rust_libraries.get(package.as_str())?.clone();
                        moved = true;
                        segments.next();
                    }
                    cargo::CrateSource::OutsideRepository => return None,
                    cargo::CrateSource::NotDeclared => {
                        let renamed = self
                            .manifests
                            .rust_manifest_directory(file)
                            .and_then(|here| self.rust_library_names.get(&(here, first)));
                        if let Some(package) = renamed {
                            module = self.rust_libraries.get(package.as_str())?.clone();
                            moved = true;
                            segments.next();
                        }
                    }
                    cargo::CrateSource::External => {}
                }
            }
            _ => {}
        }
        while segments.peek() == Some(&"super") {
            module = self.rust_parent(&module)?;
            moved = true;
            segments.next();
        }
        let mut complete = true;
        for segment in segments {
            match self.rust_child(&self.rust_children(&module), segment) {
                Some(child) => {
                    module = child;
                    moved = true;
                }
                None => {
                    complete = false;
                    break;
                }
            }
        }
        Some((module, moved, complete))
    }

    /// Resolves a qualified name of a language that declares a package or a
    /// namespace in each file. The longest prefix that is a namespace wins. The
    /// segment after it must be a top-level symbol of a file of that namespace.
    fn qualified(
        &self,
        binding: &ImportBinding,
        language: Language,
        separator: &str,
    ) -> ImportTarget {
        let mut segments: Vec<&str> = binding
            .module_path
            .split(separator)
            .filter(|segment| !segment.is_empty())
            .collect();
        if let Some(name) = binding.imported_name.as_deref() {
            if segments.last() != Some(&name) && name != "*" {
                segments.push(name);
            }
        }
        let mut in_repository = false;
        for length in (1..=segments.len()).rev() {
            let namespace = segments[..length].join(separator);
            let members: Vec<&FileSymbols> = self
                .by_module
                .get(namespace.as_str())
                .into_iter()
                .flatten()
                .map(|position| &self.files[*position])
                .filter(|file| file.language == language)
                .collect();
            if members.is_empty() {
                continue;
            }
            in_repository = true;
            let Some(name) = segments.get(length) else {
                // The path is the namespace. All its files are one package.
                return package_of(members.iter().map(|file| file.file.clone()).collect());
            };
            let files: BTreeSet<String> = members
                .iter()
                .filter(|file| {
                    file.symbols
                        .iter()
                        .any(|symbol| symbol.container.is_none() && symbol.name == *name)
                })
                .map(|file| file.file.clone())
                .collect();
            if !files.is_empty() {
                return target_of(files, ImportTarget::Unresolved);
            }
        }
        if in_repository {
            ImportTarget::Unresolved
        } else {
            ImportTarget::External
        }
    }

    /// Resolves a path to the file next to the includer, and then to the one
    /// file of the repository whose path ends with the path.
    fn include(&self, binding: &ImportBinding, languages: &[Language]) -> ImportTarget {
        let path = binding.module_path.trim_start_matches('/');
        let quoted = binding.target_file == ImportTarget::Unresolved;
        if quoted {
            if let Some(file) = self.existing(join(directory(&binding.file), path)) {
                return ImportTarget::Resolved(file);
            }
        }
        match join("", path) {
            Some(path) => target_of(
                self.with_suffix(&path, languages),
                binding.target_file.clone(),
            ),
            None => binding.target_file.clone(),
        }
    }

    fn ruby(&self, binding: &ImportBinding) -> ImportTarget {
        let path = binding.module_path.as_str();
        // The extraction marks `require` as external. Its directory is not in
        // the syntax, thus only a file in a load root is a certain target.
        if binding.target_file == ImportTarget::External {
            let file = if path.ends_with(".rb") {
                path.to_string()
            } else {
                format!("{path}.rb")
            };
            let in_roots: BTreeSet<String> = self
                .manifests
                .ruby_load_roots
                .iter()
                .filter_map(|root| self.existing(join(root, &file)))
                .collect();
            let anywhere = join("", &file)
                .map(|file| self.with_suffix(&file, &[Language::Ruby]))
                .unwrap_or_default();
            return match (in_roots.is_empty(), anywhere.is_empty()) {
                (false, _) => target_of(in_roots, ImportTarget::Unresolved),
                (true, false) => ImportTarget::Unresolved,
                (true, true) => ImportTarget::External,
            };
        }
        let exact = self.existing(join(directory(&binding.file), path));
        let source = self.existing(join(directory(&binding.file), &format!("{path}.rb")));
        target_of(
            exact.into_iter().chain(source).collect(),
            ImportTarget::Unresolved,
        )
    }

    fn bash(&self, binding: &ImportBinding) -> ImportTarget {
        const SCRIPT_DIRECTORIES: &[&str] = &[
            "$(dirname \"$0\")/",
            "$(dirname \"${BASH_SOURCE[0]}\")/",
            "$(dirname \"$BASH_SOURCE\")/",
            "${BASH_SOURCE%/*}/",
        ];
        let mut path = binding.module_path.as_str();
        for prefix in SCRIPT_DIRECTORIES {
            if let Some(rest) = path.strip_prefix(prefix) {
                path = rest;
            }
        }
        if path.starts_with('/') || path.starts_with('~') {
            return ImportTarget::External;
        }
        if path.contains('$') || path.contains('`') {
            return ImportTarget::Unresolved;
        }
        match self.existing(join(directory(&binding.file), path)) {
            Some(file) => ImportTarget::Resolved(file),
            None => ImportTarget::Unresolved,
        }
    }

    fn resolve(&self, binding: &ImportBinding, language: Language) -> ImportTarget {
        match language {
            Language::Go => self.go(binding),
            Language::Python => self.python(binding),
            Language::JavaScript | Language::Jsx | Language::TypeScript | Language::Tsx => {
                self.ecma(binding)
            }
            Language::Rust => self.rust(binding),
            Language::Java | Language::CSharp => self.qualified(binding, language, "."),
            Language::Php if binding.kind == ImportKind::Load => {
                self.include(binding, &[Language::Php])
            }
            Language::Php => self.qualified(binding, language, "\\"),
            Language::C | Language::Cpp => self.include(binding, &[Language::C, Language::Cpp]),
            Language::Ruby => self.ruby(binding),
            Language::Bash => self.bash(binding),
        }
    }

    /// Finds the key of the top-level symbol `name` of `file`. If the file
    /// does not define the name, the search follows the re-exports of the file.
    fn symbol(
        &self,
        file: &str,
        name: &str,
        visited: &mut BTreeSet<(String, String)>,
    ) -> Option<String> {
        if !visited.insert((file.to_string(), name.to_string())) {
            return None;
        }
        let facts = &self.files[*self.by_path.get(file)?];
        let mut defined = facts
            .symbols
            .iter()
            .filter(|symbol| symbol.container.is_none() && symbol.name == name);
        if let Some(first) = defined.next() {
            return defined.next().is_none().then(|| first.key.to_string());
        }
        // `export { original as name }` gives the name to a symbol of the file.
        let mut aliased = facts
            .symbols
            .iter()
            .zip(&facts.link_facts)
            .filter(|(_, link)| link.export_aliases.iter().any(|alias| alias == name));
        if let Some((first, _)) = aliased.next() {
            return aliased.next().is_none().then(|| first.key.to_string());
        }
        if let Some(key) = inline_module_symbol(facts, name) {
            return Some(key);
        }
        // Each import of a Python module is a name of that module.
        let python = facts.language == Language::Python;
        let mut found = BTreeSet::new();
        for binding in &facts.imports {
            let ImportTarget::Resolved(target) = &binding.target_file else {
                continue;
            };
            let named = binding.kind == ImportKind::ReExportNamed
                || (python && binding.kind == ImportKind::Named);
            let wildcard = binding.kind == ImportKind::ReExportWildcard
                || (python && binding.kind == ImportKind::Wildcard);
            if named && binding.local_name.as_deref() == Some(name) {
                let source = binding.imported_name.as_deref().unwrap_or(name);
                found.extend(self.symbol(target, source, visited));
            } else if wildcard && binding.local_name.is_none() {
                found.extend(self.symbol(target, name, visited));
            }
        }
        (found.len() == 1)
            .then(|| found.into_iter().next())
            .flatten()
    }
}

/// The one symbol `name` of a file that is in a module in the file, as a
/// Rust `mod tests { ... }` has it.
fn inline_module_symbol(facts: &FileSymbols, name: &str) -> Option<String> {
    let in_module = |symbol: &&crate::index::model::Symbol| {
        symbol.name == name
            && symbol.container.as_ref().is_some_and(|container| {
                facts.symbols.iter().any(|module| {
                    module.kind == crate::index::model::SymbolKind::Module
                        && module.name == *container
                })
            })
    };
    let mut found = facts.symbols.iter().filter(in_module);
    let first = found.next()?;
    found.next().is_none().then(|| first.key.to_string())
}

/// Gives a Go import without an alias the name that the package clause of
/// its target declares. The last segment of the import path is the name only
/// by convention.
fn go_package_names(files: &mut [FileSymbols]) {
    let declared: BTreeMap<&str, &str> = files
        .iter()
        .filter(|file| file.language == Language::Go && !file.file.ends_with(GO_TEST_SUFFIX))
        .filter_map(|file| Some((file.file.as_str(), file.modules.first()?.as_str())))
        // No file can import a `main` package.
        .filter(|(_, package)| *package != "main")
        .collect();
    let names: Vec<Vec<Option<String>>> = files
        .iter()
        .map(|file| {
            file.imports
                .iter()
                .map(|binding| {
                    let targets: &[String] = match &binding.target_file {
                        ImportTarget::Resolved(target) => std::slice::from_ref(target),
                        ImportTarget::Package(targets) => targets,
                        _ => &[],
                    };
                    let default_name = binding.kind == ImportKind::Module
                        && binding.local_name == binding.imported_name;
                    let packages: BTreeSet<&str> = targets
                        .iter()
                        .filter(|_| file.language == Language::Go && default_name)
                        .filter_map(|target| declared.get(target.as_str()).copied())
                        .collect();
                    match packages.into_iter().collect::<Vec<&str>>().as_slice() {
                        [only] => Some(only.to_string()),
                        _ => None,
                    }
                })
                .collect()
        })
        .collect();
    for (file, names) in files.iter_mut().zip(names) {
        for (binding, name) in file.imports.iter_mut().zip(names) {
            if name.is_some() {
                binding.local_name = name;
            }
        }
    }
}

/// Gives each import binding its target file and its target symbol.
///
/// The result does not depend on the order of `files`.
pub fn resolve_imports(files: &mut [FileSymbols], manifests: &Manifests) {
    let resolve = |files: &[FileSymbols],
                   symbols: bool|
     -> Vec<Vec<(ImportTarget, Option<String>)>> {
        let repository = Repository::new(files, manifests);
        files
            .par_iter()
            .map(|file| {
                file.imports
                    .iter()
                    .map(|binding| {
                        if !symbols {
                            return (repository.resolve(binding, file.language), None);
                        }
                        let named =
                            matches!(binding.kind, ImportKind::Named | ImportKind::ReExportNamed);
                        let symbol = match (&binding.target_file, &binding.imported_name) {
                            (ImportTarget::Resolved(target), Some(name)) if named => {
                                repository.symbol(target, name, &mut BTreeSet::new())
                            }
                            _ => None,
                        };
                        (binding.target_file.clone(), symbol)
                    })
                    .collect()
            })
            .collect()
    };
    let path_targets: Vec<BTreeMap<String, String>> = {
        let repository = Repository::new(files, manifests);
        files
            .par_iter()
            .map(|file| {
                let mut qualifiers: BTreeSet<&str> = file
                    .references
                    .iter()
                    .filter(|_| file.language == Language::Rust)
                    .filter_map(|reference| reference.qualifier.as_deref())
                    .chain(
                        file.value_sites
                            .qualifiers()
                            .filter(|_| file.language == Language::Rust),
                    )
                    .flat_map(|qualifier| {
                        // The path before a type names the module of the type.
                        let module = qualifier.rsplit_once("::").map(|(module, _)| module);
                        [Some(qualifier), module]
                    })
                    .flatten()
                    .collect();
                // A declared type can have a path, as `shapes::Circle` has.
                if file.language == Language::Rust {
                    let declared = file
                        .link_facts
                        .iter()
                        .flat_map(|facts| {
                            facts
                                .return_type
                                .iter()
                                .chain(facts.parameter_types.iter().flatten())
                        })
                        .chain(
                            file.locals
                                .iter()
                                .filter_map(|local| local.name.type_name.as_ref()),
                        );
                    for text in declared {
                        for word in text.split(|character: char| {
                            !(character.is_alphanumeric() || character == '_' || character == ':')
                        }) {
                            if let Some((module, _)) = word.rsplit_once("::") {
                                qualifiers.insert(module);
                            }
                        }
                    }
                }
                qualifiers
                    .into_iter()
                    .filter_map(|qualifier| {
                        let module = repository.rust_module(&file.file, qualifier)?;
                        Some((qualifier.to_string(), module))
                    })
                    .collect()
            })
            .collect()
    };
    for (file, targets) in files.iter_mut().zip(path_targets) {
        file.path_targets = targets;
    }
    // The symbol search reads the targets of all files. Thus the targets come first.
    for symbols in [false, true] {
        let results = resolve(files, symbols);
        for (file, results) in files.iter_mut().zip(results) {
            for (binding, (target, symbol)) in file.imports.iter_mut().zip(results) {
                binding.target_file = target;
                binding.target_symbol = symbol;
            }
        }
    }
    go_package_names(files);
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::Path;

    use super::*;
    use crate::index::extract::{extract_file, link_files};
    use crate::parser::Parser;

    /// Extracts and resolves the files. Each tuple is a path and a source.
    pub(crate) fn index(files: &[(&str, &str)]) -> Vec<FileSymbols> {
        index_with_manifests(files).0
    }

    /// Like `index`. The manifests are the second part of the result.
    pub(crate) fn index_with_manifests(files: &[(&str, &str)]) -> (Vec<FileSymbols>, Manifests) {
        let mut manifests = Manifests::default();
        let mut facts = Vec::new();
        for (file, source) in files {
            if ["go.mod", "Cargo.toml", "package.json"].contains(&file_name(file))
                || file.ends_with(".gemspec")
                || typescript::is_project_file(file_name(file))
            {
                manifests.add(file, source);
                continue;
            }
            // A different JSON file is a config that `extends` names.
            if file.ends_with(".json") {
                manifests.add_project(file, source);
                continue;
            }
            let language = Language::detect(Path::new(file)).unwrap();
            let parsed = Parser::new()
                .parse(source.as_bytes(), language, Path::new(file))
                .unwrap();
            facts.push(extract_file(&parsed, file));
        }
        facts.sort_by(|left, right| left.file.cmp(&right.file));
        link_files(&mut facts);
        resolve_imports(&mut facts, &manifests);
        (facts, manifests)
    }

    /// One line for each import of `file`:
    /// `line kind path imported->local target [symbol]`.
    pub(crate) fn rows(files: &[(&str, &str)], file: &str) -> Vec<String> {
        let facts = index(files);
        let facts = facts.iter().find(|facts| facts.file == file).unwrap();
        facts
            .imports
            .iter()
            .map(|binding| {
                let target = match &binding.target_file {
                    ImportTarget::Resolved(path) => path.clone(),
                    ImportTarget::Package(paths) => format!("package({})", paths.join(",")),
                    ImportTarget::Ambiguous(paths) => format!("ambiguous({})", paths.join(",")),
                    ImportTarget::External => "external".to_string(),
                    ImportTarget::Unresolved => "unresolved".to_string(),
                };
                let mut row = format!(
                    "{} {:?} {} {}->{} {target}",
                    binding.range.start_line,
                    binding.kind,
                    binding.module_path,
                    binding.imported_name.as_deref().unwrap_or("-"),
                    binding.local_name.as_deref().unwrap_or("-"),
                );
                if let Some(symbol) = &binding.target_symbol {
                    row.push_str(&format!(" [{symbol}]"));
                }
                row
            })
            .collect()
    }
}

#[cfg(test)]
mod thread_tests {
    use super::*;

    /// Files of each language that resolves imports through the memos of the
    /// repository: TypeScript configs with chains and a cycle of `extends`,
    /// Rust modules with `super` and `crate`, Go packages, Python packages,
    /// and Java packages.
    fn sources(count: usize) -> Vec<(String, String)> {
        let mut sources = vec![
            ("go.mod".to_string(), "module example.com/shop\n".to_string()),
            (
                "Cargo.toml".to_string(),
                "[package]\nname = \"shop\"\n".to_string(),
            ),
            (
                "web/tsconfig.base.json".to_string(),
                "{ \"compilerOptions\": { \"baseUrl\": \"${configDir}\", \"paths\": { \"@lib/*\": [\"lib/*\"] } } }\n"
                    .to_string(),
            ),
            (
                "web/cycle/a.json".to_string(),
                "{ \"extends\": \"./b.json\" }\n".to_string(),
            ),
            (
                "web/cycle/b.json".to_string(),
                "{ \"extends\": \"./a.json\" }\n".to_string(),
            ),
            (
                "web/cycle/tsconfig.json".to_string(),
                "{ \"extends\": \"./a.json\" }\n".to_string(),
            ),
        ];
        let mut modules = String::new();
        for position in 0..count {
            let next = (position + 1) % count;
            sources.push((
                format!("web/p{position}/tsconfig.json"),
                "{ \"extends\": \"../tsconfig.base.json\" }\n".to_string(),
            ));
            sources.push((
                format!("web/p{position}/lib/f{position}.ts"),
                format!("export function f{position}() {{}}\n"),
            ));
            sources.push((
                format!("web/p{position}/main.ts"),
                format!(
                    "import {{ f{position} }} from \"@lib/f{position}\";\nimport {{ g{position} }} from \"../cycle/g{position}\";\nexport {{ f{position}, g{position} }};\n"
                ),
            ));
            sources.push((
                format!("web/cycle/g{position}.ts"),
                format!("import {{ f{position} }} from \"../p{position}/main\";\nexport const g{position} = f{position};\n"),
            ));
            modules.push_str(&format!("mod m{position};\n"));
            sources.push((
                format!("src/m{position}.rs"),
                format!(
                    "use super::m{next}::run{next};\nuse crate::m{next};\npub fn run{position}() {{ run{next}(); m{next}::run{next}(); }}\n"
                ),
            ));
            sources.push((
                format!("shop/p{position}/a.go"),
                format!(
                    "package p{position}\n\nimport \"example.com/shop/shop/p{next}\"\n\nfunc A() {{ p{next}.A() }}\n"
                ),
            ));
            sources.push((
                format!("py/p{position}/__init__.py"),
                format!("from ..p{next} import run\nfrom .helper import help\n"),
            ));
            sources.push((
                format!("py/p{position}/helper.py"),
                "def help():\n    pass\n".to_string(),
            ));
            sources.push((
                format!("java/p{position}/A.java"),
                format!("package p{position};\n\nimport p{next}.A;\n\npublic class A {{}}\n"),
            ));
        }
        sources.push(("src/lib.rs".to_string(), modules));
        sources
    }

    /// The result of the resolution of the imports, as text.
    fn resolved(files: &[(&str, &str)], threads: usize) -> Vec<String> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let facts = pool.install(|| testing::index(files));
        facts
            .iter()
            .flat_map(|file| {
                let imports = file.imports.iter().map(move |binding| {
                    format!(
                        "{} {:?} {:?} {:?}",
                        file.file, binding.module_path, binding.target_file, binding.target_symbol
                    )
                });
                std::iter::once(format!("{} {:?}", file.file, file.path_targets)).chain(imports)
            })
            .collect()
    }

    #[test]
    fn test_imports_resolve_the_same_for_each_number_of_threads() {
        let sources = sources(150);
        let files: Vec<(&str, &str)> = sources
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let expected = resolved(&files, 1);
        assert!(expected
            .iter()
            .any(|row| row.contains("Resolved(\"web/p0/lib/f0.ts\")")));
        assert!(expected
            .iter()
            .any(|row| row.contains("Resolved(\"src/m1.rs\")")));
        for threads in [2, 3, 4, 8] {
            for _ in 0..3 {
                assert_eq!(resolved(&files, threads), expected, "{threads} threads");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_go_module_path() {
        assert_eq!(
            go_module_path("// c\nmodule example.com/shop\n\ngo 1.21\n").as_deref(),
            Some("example.com/shop")
        );
        assert_eq!(go_module_path("modulex a\n"), None);
        assert_eq!(go_module_path("go 1.21\n"), None);
    }

    #[test]
    fn test_join_removes_dots_and_stops_at_the_root() {
        assert_eq!(join("a/b", "../c/./d").as_deref(), Some("a/c/d"));
        assert_eq!(join("", "x").as_deref(), Some("x"));
        assert_eq!(join("a", "../../x"), None);
    }

    /// `roots` integration tests declare `mod common;`, and the shared module
    /// has `imports` imports with `crate::`. The parent of the shared module
    /// is one of many roots, thus it has no one parent.
    fn imports_of_a_module_of_many_roots_resolve_at(
        roots: usize,
        imports: usize,
    ) -> std::time::Duration {
        let manifest = "[package]\nname = \"kit\"\n";
        let mut owned = vec![
            ("Cargo.toml".to_string(), manifest.to_string()),
            ("src/lib.rs".to_string(), "pub fn run() {}\n".to_string()),
        ];
        for root in 0..roots {
            owned.push((format!("tests/t{root}.rs"), "mod common;\n".to_string()));
        }
        let shared: String = (0..imports)
            .map(|import| format!("use crate::helper{import};\n"))
            .collect();
        owned.push(("tests/common/mod.rs".to_string(), shared));
        let files: Vec<(&str, &str)> = owned
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let started = crate::index::timing::Stopwatch::start();
        let facts = super::testing::index(&files);
        let elapsed = started.elapsed();
        let shared = facts
            .iter()
            .find(|file| file.file == "tests/common/mod.rs")
            .expect("the shared module");
        assert_eq!(shared.imports.len(), imports);
        assert!(shared
            .imports
            .iter()
            .all(|binding| binding.target_file == ImportTarget::Unresolved));
        elapsed
    }

    #[test]
    fn test_imports_of_a_module_of_many_roots_are_unresolved() {
        imports_of_a_module_of_many_roots_resolve_at(20, 20);
    }

    /// The search for the parent of the shared module must not occur again
    /// for each import.
    #[test]
    #[ignore = "timing: run in release"]
    fn timing_imports_of_a_module_of_many_roots_resolve_in_linear_time() {
        crate::index::timing::assert_scales_linearly("roots", 2_000, |size| {
            imports_of_a_module_of_many_roots_resolve_at(size, size)
        });
    }
}

#[cfg(test)]
mod typescript_tests {
    use super::testing::rows;

    /// The target of the import of `module` in `file`.
    fn target(files: &[(&str, &str)], file: &str, module: &str) -> String {
        let found: Vec<String> = rows(files, file)
            .into_iter()
            .filter(|row| row.split(' ').nth(2) == Some(module))
            .map(|row| row.split(' ').nth(4).unwrap_or_default().to_string())
            .collect();
        assert!(!found.is_empty(), "no import of {module} in {file}");
        found[0].clone()
    }

    #[test]
    fn test_tsconfig_paths_of_the_nearest_config_with_comments_and_trailing_commas() {
        let app_config = "{\n  // The alias of the source directory.\n  \"compilerOptions\": {\n    /* A base. */ \"baseUrl\": \".\",\n    \"paths\": { \"@/*\": [\"./src/*\"], },\n  },\n}\n";
        let other_config = "{ \"compilerOptions\": { \"paths\": { \"@/*\": [\"./lib/*\"] } } }";
        for (extension, source) in [("ts", "ts"), ("js", "js"), ("tsx", "tsx"), ("jsx", "jsx")] {
            let main = format!("packages/app/src/main.{extension}");
            let svc = format!("packages/app/src/svc/svc.{source}");
            let other_main = format!("packages/other/src/main.{extension}");
            let other_svc = format!("packages/other/lib/svc/svc.{source}");
            let files = [
                ("packages/app/tsconfig.json", app_config),
                ("packages/other/tsconfig.json", other_config),
                (
                    main.as_str(),
                    "import { Svc } from '@/svc/svc';\nimport { gone } from '@/gone';\n",
                ),
                (svc.as_str(), "export class Svc {}\n"),
                (other_main.as_str(), "import { Svc } from '@/svc/svc';\n"),
                (other_svc.as_str(), "export class Svc {}\n"),
            ];
            assert_eq!(target(&files, &main, "@/svc/svc"), svc, "{extension}");
            assert_eq!(
                target(&files, &other_main, "@/svc/svc"),
                other_svc,
                "{extension}"
            );
            // A pattern that names no file of the repository does not give a file.
            assert_eq!(target(&files, &main, "@/gone"), "unresolved", "{extension}");
        }
    }

    #[test]
    fn test_tsconfig_paths_through_an_extends_chain() {
        let files = [
            (
                "packages/config/package.json",
                "{\"name\": \"@acme/config\", \"version\": \"1.0.0\"}",
            ),
            // `paths` of a base config is relative to the base config.
            (
                "packages/config/tsconfig.shared.json",
                "{\"compilerOptions\": {\"paths\": {\"@shared/*\": [\"../shared/src/*\"]}}}",
            ),
            // `${configDir}` is the directory of the config that the tool reads first.
            (
                "packages/config/tsconfig.local.json",
                "{\"compilerOptions\": {\"paths\": {\"~/*\": [\"${configDir}/src/*\"]}}}",
            ),
            (
                "packages/web/tsconfig.json",
                "{\"extends\": [\"@acme/config/tsconfig.shared.json\"]}",
            ),
            (
                "packages/api/tsconfig.json",
                "{\"extends\": [\"@acme/config/tsconfig.shared.json\", \"./tsconfig.base\"]}",
            ),
            (
                "packages/api/tsconfig.base.json",
                "{\"extends\": \"@acme/config/tsconfig.local.json\"}",
            ),
            ("packages/shared/src/text.ts", "export function trim() {}\n"),
            (
                "packages/web/src/main.ts",
                "import { trim } from '@shared/text';\n",
            ),
            (
                "packages/api/src/main.ts",
                "import { trim } from '@shared/text';\nimport { run } from '~/run';\n",
            ),
            ("packages/api/src/run.ts", "export function run() {}\n"),
            // A second config that extends the same base gets its own `${configDir}`.
            (
                "packages/admin/tsconfig.json",
                "{\"extends\": \"@acme/config/tsconfig.local.json\"}",
            ),
            (
                "packages/admin/src/main.ts",
                "import { run } from '~/run';\n",
            ),
            ("packages/admin/src/run.ts", "export function run() {}\n"),
        ];
        assert_eq!(
            target(&files, "packages/web/src/main.ts", "@shared/text"),
            "packages/shared/src/text.ts"
        );
        // A later config of `extends` replaces `paths` of an earlier config.
        assert_eq!(
            target(&files, "packages/api/src/main.ts", "~/run"),
            "packages/api/src/run.ts"
        );
        assert_eq!(
            target(&files, "packages/api/src/main.ts", "@shared/text"),
            "external"
        );
        assert_eq!(
            target(&files, "packages/admin/src/main.ts", "~/run"),
            "packages/admin/src/run.ts"
        );
    }

    #[test]
    fn test_tsconfig_base_url_and_the_longest_pattern() {
        let config = "{\"compilerOptions\": {\"baseUrl\": \"src\", \"paths\": {\"*\": [\"vendor/*\", \"*\"], \"app/*\": [\"modules/*\"], \"app/exact\": [\"modules/one.ts\"]}}}";
        let files = [
            ("tsconfig.json", config),
            ("src/modules/one.ts", "export const one = 1;\n"),
            ("src/modules/two.ts", "export const two = 2;\n"),
            ("src/utils/math.ts", "export const sum = 3;\n"),
            (
                "src/main.ts",
                "import { one } from 'app/exact';\nimport { two } from 'app/two';\nimport { sum } from 'utils/math';\nimport { x } from 'lodash';\n",
            ),
        ];
        assert_eq!(
            target(&files, "src/main.ts", "app/exact"),
            "src/modules/one.ts"
        );
        assert_eq!(
            target(&files, "src/main.ts", "app/two"),
            "src/modules/two.ts"
        );
        assert_eq!(
            target(&files, "src/main.ts", "utils/math"),
            "src/utils/math.ts"
        );
        assert_eq!(target(&files, "src/main.ts", "lodash"), "external");
    }

    /// An exact entry of `exports` with `null` hides the subpath from a
    /// pattern. A config at the root of the repository with `rootDir: "."`
    /// maps the output to a source at the root.
    /// `extends` with the name of a package reads the config that the
    /// `tsconfig` field of the package names, else its `tsconfig.json`.
    #[test]
    fn test_extends_reads_the_tsconfig_field_of_a_package() {
        let files = [
            (
                "packages/config/package.json",
                "{\"name\": \"@acme/config\", \"tsconfig\": \"./configs/base.json\"}",
            ),
            (
                "packages/config/tsconfig.json",
                "{\"compilerOptions\": {\"paths\": {\"~/*\": [\"../wrong/*\"]}}}",
            ),
            (
                "packages/config/configs/base.json",
                "{\"compilerOptions\": {\"paths\": {\"~/*\": [\"../../right/*\"]}}}",
            ),
            (
                "packages/app/tsconfig.json",
                "{\"extends\": \"@acme/config\"}",
            ),
            ("packages/right/a.ts", "export function a() {}\n"),
            ("packages/wrong/a.ts", "export function a() {}\n"),
            ("packages/app/src/main.ts", "import { a } from '~/a';\n"),
        ];
        assert_eq!(
            target(&files, "packages/app/src/main.ts", "~/a"),
            "packages/right/a.ts"
        );
    }

    /// A substitution with `${configDir}` is a path from the directory of
    /// the config, not from `baseUrl`.
    #[test]
    fn test_config_directory_substitution_does_not_use_the_base_url() {
        let files = [
            (
                "config/tsconfig.base.json",
                "{\"compilerOptions\": {\"paths\": {\"@/*\": [\"${configDir}/lib/*\", \"./other/*\"]}}}",
            ),
            (
                "app/tsconfig.json",
                "{\"extends\": \"../config/tsconfig.base.json\", \"compilerOptions\": {\"baseUrl\": \"src\"}}",
            ),
            ("app/lib/a.ts", "export function a() {}\n"),
            ("app/src/lib/a.ts", "export function a() {}\n"),
            ("app/src/other/b.ts", "export function b() {}\n"),
            (
                "app/src/main.ts",
                "import { a } from '@/a';\nimport { b } from '@/b';\n",
            ),
        ];
        let main = "app/src/main.ts";
        assert_eq!(target(&files, main, "@/a"), "app/lib/a.ts");
        assert_eq!(target(&files, main, "@/b"), "app/src/other/b.ts");
    }

    #[test]
    fn test_null_export_and_a_root_directory_of_the_repository() {
        let files = [
            (
                "package.json",
                "{\"name\": \"root-lib\", \"exports\": {\".\": \"./out/index.js\", \"./private\": null, \"./*\": \"./out/*.js\"}}",
            ),
            (
                "tsconfig.json",
                "{\"compilerOptions\": {\"rootDir\": \".\", \"outDir\": \"out\"}}",
            ),
            ("index.ts", "export function root() {}\n"),
            ("private.ts", "export function hidden() {}\n"),
            ("open.ts", "export function open() {}\n"),
            (
                "app/main.ts",
                "import { root } from 'root-lib';\nimport { hidden } from 'root-lib/private';\nimport { open } from 'root-lib/open';\n",
            ),
        ];
        let main = "app/main.ts";
        assert_eq!(target(&files, main, "root-lib"), "index.ts");
        assert_eq!(target(&files, main, "root-lib/open"), "open.ts");
        assert_eq!(target(&files, main, "root-lib/private"), "unresolved");
    }

    #[test]
    fn test_workspace_package_that_points_at_dist_maps_to_the_source() {
        let files = [
            // The `outDir` and `rootDir` of a config in the package give the rule.
            (
                "packages/lib/package.json",
                "{\"name\": \"my-lib\", \"main\": \"dist/index\", \"types\": \"dist/index.d.ts\"}",
            ),
            (
                "packages/lib/tsconfig.build.json",
                "{\"compilerOptions\": {\"rootDir\": \"src\", \"outDir\": \"dist\"}}",
            ),
            ("packages/lib/src/index.ts", "export function libFn() {}\n"),
            ("packages/lib/src/util/a.ts", "export function a() {}\n"),
            // Two configs give two output directories of one source directory.
            (
                "packages/flow/package.json",
                "{\"name\": \"@acme/flow\", \"exports\": {\".\": {\"import\": \"./dist/esm/index.js\", \"require\": \"./dist/cjs/index.js\"}, \"./*\": \"./dist/esm/*.js\"}}",
            ),
            ("packages/flow/tsconfig.json", "{\"compilerOptions\": {\"strict\": true}}"),
            (
                "packages/flow/tsconfig.build.esm.json",
                "{\"extends\": [\"./tsconfig.json\"], \"compilerOptions\": {\"rootDir\": \"src\", \"outDir\": \"dist/esm\"}}",
            ),
            (
                "packages/flow/tsconfig.build.cjs.json",
                "{\"extends\": [\"./tsconfig.json\"], \"compilerOptions\": {\"rootDir\": \"src\", \"outDir\": \"dist/cjs\"}}",
            ),
            ("packages/flow/src/index.ts", "export function flow() {}\n"),
            ("packages/flow/src/util/text.ts", "export function text() {}\n"),
            // Without `rootDir`, `dist` has the same relative paths as `src`.
            (
                "packages/utils/package.json",
                "{\"name\": \"@acme/utils\", \"exports\": {\"./*\": {\"types\": \"./dist/*.d.mts\", \"import\": \"./dist/*.mjs\"}}}",
            ),
            ("packages/utils/tsconfig.json", "{\"compilerOptions\": {\"outDir\": \"dist\"}}"),
            ("packages/utils/src/is-record.ts", "export function isRecord() {}\n"),
            ("packages/utils/src/pair.ts", "export function pair() {}\n"),
            ("packages/utils/src/pair.tsx", "export function pair() {}\n"),
            (
                "packages/app/src/main.ts",
                "import { libFn } from 'my-lib';\nimport { flow } from '@acme/flow';\nimport { text } from '@acme/flow/util/text';\nimport { isRecord } from '@acme/utils/is-record';\nimport { missing } from '@acme/utils/missing';\nimport { pair } from '@acme/utils/pair';\nimport { a } from 'my-lib/dist/util/a';\n",
            ),
        ];
        let main = "packages/app/src/main.ts";
        assert_eq!(target(&files, main, "my-lib"), "packages/lib/src/index.ts");
        // A package without `exports` gives each file of its directory.
        assert_eq!(
            target(&files, main, "my-lib/dist/util/a"),
            "packages/lib/src/util/a.ts"
        );
        assert_eq!(
            target(&files, main, "@acme/flow"),
            "packages/flow/src/index.ts"
        );
        assert_eq!(
            target(&files, main, "@acme/flow/util/text"),
            "packages/flow/src/util/text.ts"
        );
        assert_eq!(
            target(&files, main, "@acme/utils/is-record"),
            "packages/utils/src/is-record.ts"
        );
        assert_eq!(target(&files, main, "@acme/utils/missing"), "unresolved");
        assert_eq!(
            target(&files, main, "@acme/utils/pair"),
            "ambiguous(packages/utils/src/pair.ts,packages/utils/src/pair.tsx)"
        );
    }
}

#[cfg(test)]
mod typescript_timing_tests {
    use super::testing::{index, rows};
    use super::ImportTarget;
    use crate::index::timing::assert_scales_linearly;

    /// A config with `size` patterns, `size` packages whose configs extend
    /// it, and one file with `size` imports through the patterns.
    fn path_patterns_at(size: usize) -> std::time::Duration {
        let patterns: Vec<String> = (0..size)
            .map(|position| format!("\"@lib{position}/*\": [\"libs/lib{position}/src/*\"]"))
            .collect();
        let mut owned = vec![(
            "tsconfig.base.json".to_string(),
            format!(
                "{{ \"compilerOptions\": {{ \"paths\": {{ {} }} }} }}",
                patterns.join(", ")
            ),
        )];
        let mut main = String::new();
        for position in 0..size {
            owned.push((
                format!("libs/lib{position}/tsconfig.json"),
                "{ \"extends\": \"../../tsconfig.base.json\" }".to_string(),
            ));
            owned.push((
                format!("libs/lib{position}/src/index.ts"),
                "export const value = 1;\n".to_string(),
            ));
            main.push_str(&format!(
                "import {{ value as v{position} }} from '@lib{position}/index';\n"
            ));
        }
        owned.push((
            "app/tsconfig.json".to_string(),
            "{ \"extends\": \"../tsconfig.base.json\" }".to_string(),
        ));
        owned.push(("app/main.ts".to_string(), main));
        let files: Vec<(&str, &str)> = owned
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let started = crate::index::timing::Stopwatch::start();
        let found = rows(&files, "app/main.ts");
        let elapsed = started.elapsed();
        assert_eq!(found.len(), size);
        let last = format!("libs/lib{}/src/index.ts", size - 1);
        assert!(
            found.iter().any(|row| row.contains(&last)),
            "{:?}",
            found.last()
        );
        elapsed
    }

    /// `size` patterns of `paths` with one prefix and different suffixes in
    /// a base config, and `size` configs that extend it. A file beside each
    /// config imports a name that no suffix matches, and a name that the
    /// pattern of the config matches.
    fn same_prefix_patterns_at(size: usize) -> std::time::Duration {
        let patterns: Vec<String> = (0..size)
            .map(|position| format!("\"@x/*.p{position}\": [\"${{configDir}}/src/*\"]"))
            .collect();
        let mut owned = vec![(
            "tsconfig.base.json".to_string(),
            format!(
                "{{ \"compilerOptions\": {{ \"paths\": {{ {} }} }} }}",
                patterns.join(", ")
            ),
        )];
        for position in 0..size {
            owned.push((
                format!("c{position}/tsconfig.json"),
                "{ \"extends\": \"../tsconfig.base.json\" }".to_string(),
            ));
            owned.push((
                format!("c{position}/src/value.ts"),
                "export const value = 1;\n".to_string(),
            ));
            owned.push((
                format!("c{position}/src/main.ts"),
                format!("import {{ value }} from '@x/value.p{position}';\nimport {{ other }} from '@x/value.q';\n"),
            ));
        }
        let files: Vec<(&str, &str)> = owned
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let started = crate::index::timing::Stopwatch::start();
        let facts = index(&files);
        let elapsed = started.elapsed();
        let last = format!("c{}/src/main.ts", size - 1);
        let main = facts.iter().find(|facts| facts.file == last).unwrap();
        let targets: Vec<&ImportTarget> = main
            .imports
            .iter()
            .map(|binding| &binding.target_file)
            .collect();
        assert_eq!(
            targets,
            [
                &ImportTarget::Resolved(format!("c{}/src/value.ts", size - 1)),
                &ImportTarget::External
            ]
        );
        elapsed
    }

    /// A package whose `exports` has `size` keys with `*`, and `size` files
    /// that each import one subpath that a key matches and one that no key
    /// matches.
    fn export_patterns_at(size: usize) -> std::time::Duration {
        let keys: Vec<String> = (0..size)
            .map(|position| format!("\"./k{position}/*\": \"./src/k{position}/*.ts\""))
            .collect();
        let mut owned = vec![(
            "packages/lib/package.json".to_string(),
            format!(
                "{{ \"name\": \"lib\", \"exports\": {{ {} }} }}",
                keys.join(", ")
            ),
        )];
        for position in 0..size {
            owned.push((
                format!("packages/lib/src/k{position}/value.ts"),
                "export const value = 1;\n".to_string(),
            ));
            owned.push((
                format!("app/m{position}.ts"),
                format!("import {{ value }} from 'lib/k{position}/value';\nimport {{ other }} from 'lib/none{position}/value';\n"),
            ));
        }
        let files: Vec<(&str, &str)> = owned
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let started = crate::index::timing::Stopwatch::start();
        let facts = index(&files);
        let elapsed = started.elapsed();
        let last = format!("app/m{}.ts", size - 1);
        let main = facts.iter().find(|facts| facts.file == last).unwrap();
        let targets: Vec<&ImportTarget> = main
            .imports
            .iter()
            .map(|binding| &binding.target_file)
            .collect();
        assert_eq!(
            targets,
            [
                &ImportTarget::Resolved(format!("packages/lib/src/k{}/value.ts", size - 1)),
                &ImportTarget::Unresolved
            ]
        );
        elapsed
    }

    /// A chain of `size` configs in sibling directories, each of which
    /// extends the config before it, and one file beside each config that
    /// imports through the pattern of the first config.
    fn chain_of_extends_at(size: usize) -> std::time::Duration {
        let mut owned = vec![
            (
                "c0/tsconfig.json".to_string(),
                "{ \"compilerOptions\": { \"paths\": { \"@shared/*\": [\"./shared/*\"] } } }"
                    .to_string(),
            ),
            (
                "c0/shared/value.ts".to_string(),
                "export const value = 1;\n".to_string(),
            ),
        ];
        for position in 1..size {
            owned.push((
                format!("c{position}/tsconfig.json"),
                format!("{{ \"extends\": \"../c{}/tsconfig.json\" }}", position - 1),
            ));
            owned.push((
                format!("c{position}/main.ts"),
                "import { value } from '@shared/value';\n".to_string(),
            ));
        }
        let files: Vec<(&str, &str)> = owned
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let last = format!("c{}/main.ts", size - 1);
        let started = crate::index::timing::Stopwatch::start();
        let found = rows(&files, &last);
        let elapsed = started.elapsed();
        assert!(found[0].contains("c0/shared/value.ts"), "{found:?}");
        elapsed
    }

    /// `size` layers of two configs. The config of the top directory and
    /// each config of a layer extend both configs of the next layer, and the
    /// two configs of the last layer extend each other. The number of paths
    /// through the layers is `2^size`.
    fn branching_extends_with_a_cycle_at(size: usize) -> std::time::Duration {
        let mut owned = Vec::new();
        let layer = |position: usize| {
            format!("[\"../l{position}a/tsconfig.json\", \"../l{position}b/tsconfig.json\"]")
        };
        owned.push((
            "top/tsconfig.json".to_string(),
            format!(
                "{{ \"extends\": {}, \"compilerOptions\": {{ \"paths\": {{ \"@x/*\": [\"./src/*\"] }} }} }}",
                layer(0)
            ),
        ));
        for position in 0..size {
            for (side, other) in [("a", "b"), ("b", "a")] {
                let extends = if position + 1 < size {
                    layer(position + 1)
                } else {
                    format!("\"../l{position}{other}/tsconfig.json\"")
                };
                owned.push((
                    format!("l{position}{side}/tsconfig.json"),
                    format!("{{ \"extends\": {extends} }}"),
                ));
            }
        }
        owned.push((
            "top/src/value.ts".to_string(),
            "export const value = 1;\n".to_string(),
        ));
        owned.push((
            "top/main.ts".to_string(),
            "import { value } from '@x/value';\n".to_string(),
        ));
        let files: Vec<(&str, &str)> = owned
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let started = crate::index::timing::Stopwatch::start();
        let found = rows(&files, "top/main.ts");
        let elapsed = started.elapsed();
        // A cycle of `extends` is an error of `tsc`. A parent that reaches
        // the cycle gives no options, and the own pattern of the top config
        // applies.
        assert_eq!(found, ["1 Named @x/value value->value top/src/value.ts"]);
        elapsed
    }

    #[test]
    fn test_branching_extends_with_a_cycle_resolves() {
        branching_extends_with_a_cycle_at(8);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_branching_extends_with_a_cycle_resolves_in_linear_time() {
        assert_scales_linearly(
            "branching extends",
            1_000,
            branching_extends_with_a_cycle_at,
        );
    }

    #[test]
    fn test_chain_of_extends_resolves() {
        chain_of_extends_at(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_chain_of_extends_resolves_in_linear_time() {
        assert_scales_linearly("chain of extends", 500, chain_of_extends_at);
    }

    #[test]
    fn test_many_path_patterns_resolve() {
        path_patterns_at(50);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_path_patterns_resolve_in_linear_time() {
        assert_scales_linearly("path patterns", 2_000, path_patterns_at);
    }

    #[test]
    fn test_export_patterns_resolve() {
        export_patterns_at(50);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_export_patterns_resolve_in_linear_time() {
        assert_scales_linearly("export patterns", 2_000, export_patterns_at);
    }

    #[test]
    fn test_patterns_with_one_prefix_resolve() {
        same_prefix_patterns_at(50);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_patterns_with_one_prefix_resolve_in_linear_time() {
        assert_scales_linearly("patterns with one prefix", 1_000, same_prefix_patterns_at);
    }
}
