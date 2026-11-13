//! Import resolution. A module map for each language gives the file to which
//! an import binding refers.
//!
//! No rule selects the first of several candidates. Two or more modules that
//! match one import path give [`ImportTarget::Ambiguous`]. The files of one
//! package give [`ImportTarget::Package`], which is a certain target.

use std::collections::{BTreeMap, BTreeSet};

use super::extract::FileSymbols;
use super::model::{ImportBinding, ImportKind, ImportTarget};
use crate::core::Language;

/// The facts of the repository that the module maps use.
struct Repository<'a> {
    files: &'a [FileSymbols],
    paths: BTreeSet<&'a str>,
    /// The module path of each `go.mod` file, by the directory of the file.
    go_modules: &'a BTreeMap<String, String>,
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
        let nested = format!("/{suffix}");
        self.files
            .iter()
            .filter(|file| languages.contains(&file.language))
            .filter(|file| file.file == suffix || file.file.ends_with(&nested))
            .map(|file| file.file.clone())
            .collect()
    }

    fn go(&self, binding: &ImportBinding) -> ImportTarget {
        let here = directory(&binding.file);
        let own = self
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
                self.files
                    .iter()
                    .filter(|file| {
                        file.language == Language::Go
                            && directory(&file.file) == directory_of_package
                    })
                    .map(|file| file.file.clone())
                    .collect(),
            )
        };
        let matches: Vec<(&String, BTreeSet<String>)> = self
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
        let nested = format!("/{module}");
        let mut directories: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        for file in self
            .files
            .iter()
            .filter(|file| file.language == Language::Python)
        {
            let here = directory(&file.file);
            let named = match base {
                Some(base) => join(base, &module).as_deref() == Some(here),
                None => here == module || here.ends_with(&nested),
            };
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
        let mut found = BTreeSet::new();
        for form in forms {
            match base {
                Some(base) => found.extend(self.existing(join(base, &form))),
                None => found.extend(self.with_suffix(&form, &[Language::Python])),
            }
        }
        found
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
            return ImportTarget::External;
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

    /// The directory that has the child modules of the module in `file`.
    fn rust_children(file: &str) -> String {
        let here = directory(file);
        let stem = file_name(file).trim_end_matches(".rs");
        if ["main", "lib", "mod"].contains(&stem) {
            here.to_string()
        } else {
            join(here, stem).unwrap_or_default()
        }
    }

    /// The file of the module that declares the module in `file`.
    fn rust_parent(&self, file: &str) -> Option<String> {
        let here = directory(file);
        let stem = file_name(file).trim_end_matches(".rs");
        // The module whose child modules are in `children`.
        let owner = |children: &str| -> Option<String> {
            let mut found: Vec<String> = ["main.rs", "lib.rs", "mod.rs"]
                .iter()
                .filter_map(|root| self.existing(join(children, root)))
                .collect();
            if let Some((above, name)) = children.rsplit_once('/') {
                found.extend(self.existing(join(above, &format!("{name}.rs"))));
            } else if !children.is_empty() {
                found.extend(self.existing(Some(format!("{children}.rs"))));
            }
            (found.len() == 1).then(|| found.remove(0))
        };
        match stem {
            "main" | "lib" => None,
            "mod" if here.is_empty() => None,
            "mod" => owner(directory(here)),
            _ => owner(here),
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
        ["main.rs", "lib.rs"]
            .contains(&file_name(&current))
            .then_some(current)
    }

    fn rust(&self, binding: &ImportBinding) -> ImportTarget {
        let mut segments = binding.module_path.split("::").peekable();
        let mut module = binding.file.clone();
        let mut moved = false;
        match segments.peek().copied() {
            Some("crate") => match self.rust_root(&binding.file) {
                Some(root) => {
                    module = root;
                    moved = true;
                    segments.next();
                }
                None => return ImportTarget::Unresolved,
            },
            Some("self") => {
                moved = true;
                segments.next();
            }
            _ => {}
        }
        while segments.peek() == Some(&"super") {
            match self.rust_parent(&module) {
                Some(parent) => module = parent,
                None => return ImportTarget::Unresolved,
            }
            moved = true;
            segments.next();
        }
        for segment in segments {
            match self.rust_child(&Self::rust_children(&module), segment) {
                Some(child) => {
                    module = child;
                    moved = true;
                }
                None => break,
            }
        }
        if moved {
            ImportTarget::Resolved(module)
        } else if binding.kind == ImportKind::ModuleDeclaration {
            ImportTarget::Unresolved
        } else {
            ImportTarget::External
        }
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
                .files
                .iter()
                .filter(|file| file.language == language && file.modules.contains(&namespace))
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
        // The extraction marks `require` as external. Only `require_relative` has a known directory.
        if binding.target_file == ImportTarget::External {
            return ImportTarget::External;
        }
        let path = binding.module_path.as_str();
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
        let facts = self.files.iter().find(|facts| facts.file == file)?;
        let mut defined = facts
            .symbols
            .iter()
            .filter(|symbol| symbol.container.is_none() && symbol.name == name);
        if let Some(first) = defined.next() {
            return defined.next().is_none().then(|| first.key.clone());
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

/// Gives each import binding its target file and its target symbol.
///
/// `go_modules` has the module path of each `go.mod` file, by the directory
/// of that file. The result does not depend on the order of `files`.
pub fn resolve_imports(files: &mut [FileSymbols], go_modules: &BTreeMap<String, String>) {
    let resolve = |files: &[FileSymbols],
                   symbols: bool|
     -> Vec<Vec<(ImportTarget, Option<String>)>> {
        let repository = Repository {
            files,
            paths: files.iter().map(|file| file.file.as_str()).collect(),
            go_modules,
        };
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
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::Path;

    use super::*;
    use crate::index::extract::{extract_file, link_files};
    use crate::parser::Parser;

    /// Extracts and resolves the files. Each tuple is a path and a source.
    pub(crate) fn index(files: &[(&str, &str)]) -> Vec<FileSymbols> {
        let mut go_modules = BTreeMap::new();
        let mut facts = Vec::new();
        for (file, source) in files {
            if file_name(file) == "go.mod" {
                go_modules.extend(
                    go_module_path(source).map(|module| (directory(file).to_string(), module)),
                );
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
        resolve_imports(&mut facts, &go_modules);
        facts
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
}
