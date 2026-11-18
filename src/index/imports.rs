//! Import resolution. A module map for each language gives the file to which
//! an import binding refers.
//!
//! No rule selects the first of several candidates. Two or more modules that
//! match one import path give [`ImportTarget::Ambiguous`]. The files of one
//! package give [`ImportTarget::Package`], which is a certain target.

pub mod cargo;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use parking_lot::RwLock;

use super::extract::FileSymbols;
use super::model::{ImportBinding, ImportKind, ImportTarget, SymbolKind};
use crate::core::Language;

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
    /// The packages that the `package.json` files of the repository name.
    pub node_packages: Vec<NodePackage>,
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
    Some(NodePackage {
        name,
        directory: directory.to_string(),
        exports,
    })
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
        }
    }
}

impl Manifests {
    /// Reads a `go.mod`, `.gemspec`, `Cargo.toml`, or `package.json` file.
    /// `path` is relative to the root of the repository. A different file has
    /// no effect.
    pub fn add(&mut self, path: &str, text: &str) {
        let here = directory(path);
        if file_name(path) == "package.json" {
            self.node_packages.extend(node_package(here, text));
            self.node_packages.sort_by(|left, right| {
                (&left.directory, &left.name).cmp(&(&right.directory, &right.name))
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
        // The files of the package directory, for a module whose path starts the import path.
        let package = |root: &String, name: &String| -> Option<BTreeSet<String>> {
            let directory_of_package = if path == name {
                root.clone()
            } else {
                join(root, path.strip_prefix(&format!("{name}/"))?)?
            };
            Some(
                self.in_directories(&directory_of_package)
                    .filter(|file| {
                        file.language == Language::Go
                            && directory(&file.file) == directory_of_package
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
        const EXTENSIONS: &[&str] = &["ts", "tsx", "d.ts", "js", "jsx", "mjs", "cjs"];
        let path = binding.module_path.as_str();
        if !path.starts_with('.') {
            // A package of the repository is not external. The `exports` of
            // its `package.json` name the file. An entry can name a source
            // file and its build outputs. The files that are in the
            // repository are the candidates.
            let mut own_package = false;
            let mut found = BTreeSet::new();
            for package in &self.manifests.node_packages {
                let subpath = match path.strip_prefix(package.name.as_str()) {
                    Some("") => ".".to_string(),
                    Some(rest) if rest.starts_with('/') => format!(".{rest}"),
                    _ => continue,
                };
                own_package = true;
                for file in package.exports.get(&subpath).into_iter().flatten() {
                    found.extend(self.existing(join(&package.directory, file)));
                }
            }
            return if own_package {
                target_of(found, ImportTarget::Unresolved)
            } else {
                ImportTarget::External
            };
        }
        let Some(base) = join(directory(&binding.file), path) else {
            return ImportTarget::Unresolved;
        };
        if let Some(exact) = self.existing(Some(base.clone())) {
            return ImportTarget::Resolved(exact);
        }
        let mut found = BTreeSet::new();
        for extension in EXTENSIONS {
            found.extend(self.existing(Some(format!("{base}.{extension}"))));
            found.extend(self.existing(Some(format!("{base}/index.{extension}"))));
        }
        // TypeScript source can name the output file `./a.js` of the source file `./a.ts`.
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
        target_of(found, ImportTarget::Unresolved)
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
        .filter(|file| file.language == Language::Go && !file.file.ends_with("_test.go"))
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
            .iter()
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
            .iter()
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
            {
                manifests.add(file, source);
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
