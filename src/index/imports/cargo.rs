//! The facts of a `Cargo.toml` file: the library, the other targets, and the
//! dependencies of a package. The index reads a manifest as TOML data. It
//! does not run `cargo`, and it does not read a lockfile.

use std::collections::{BTreeMap, BTreeSet};

use super::join;

/// The directories in which `cargo` finds a target for each file, with the
/// key of `[package]` that stops the search.
const AUTOMATIC_TARGETS: [(&str, &str); 4] = [
    ("src/bin", "autobins"),
    ("examples", "autoexamples"),
    ("tests", "autotests"),
    ("benches", "autobenches"),
];

/// The tables of a manifest that list targets with a `path` key.
const TARGET_TABLES: [&str; 4] = ["bin", "example", "test", "bench"];

/// The tables of a manifest that list dependencies.
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// One dependency of a manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dependency {
    /// The dependency has a `path` key.
    pub is_local: bool,
    /// The directory that the `path` key gives, when the directory is in the repository.
    pub path: Option<String>,
    /// The name of the package, with `_` for `-`. The `package` key gives it
    /// when the dependency has a different name in the source.
    pub package: String,
    /// The dependency has `workspace = true`. The table
    /// `[workspace.dependencies]` of the workspace gives its source.
    pub inherited: bool,
}

/// The facts of one manifest. Each path is relative to the root of the repository.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Package {
    /// The name with which Rust source refers to the library. A manifest
    /// without `[package]` has none.
    pub library_name: Option<String>,
    /// The file that `[lib] path` gives.
    pub library_path: Option<String>,
    /// The files that the `path` keys of the other targets and the `build`
    /// key give. Each file is the root of a crate.
    pub target_paths: BTreeSet<String>,
    /// The directories in which each file is the root of a crate.
    pub target_directories: Vec<String>,
    /// The dependencies, by the name that Rust source uses.
    pub dependencies: BTreeMap<String, Dependency>,
    /// The table `[workspace.dependencies]`. A manifest without `[workspace]` has none.
    pub workspace_dependencies: Option<BTreeMap<String, Dependency>>,
    /// The directory of the repository that a `[patch]` table gives to a
    /// package, by the name of the package with `_` for `-`.
    pub patches: BTreeMap<String, String>,
}

/// Where the crate that a name refers to is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrateSource {
    /// The library of the package in this directory of the repository.
    Repository(String),
    /// A crate that the manifest puts in a directory, and that directory is
    /// not in the repository. The crate is not a dependency from a registry.
    OutsideRepository,
    /// A dependency that the manifest declares, and that is not in the repository.
    External,
    /// The manifest does not declare the name.
    NotDeclared,
}

fn table_of(value: Option<&toml::Value>) -> Option<&toml::Table> {
    value.and_then(toml::Value::as_table)
}

fn text_of<'a>(table: &'a toml::Table, key: &str) -> Option<&'a str> {
    table.get(key).and_then(toml::Value::as_str)
}

fn source_name(name: &str) -> String {
    name.replace('-', "_")
}

fn dependency(directory: &str, key: &str, value: &toml::Value) -> Dependency {
    let table = value.as_table();
    let text = |name: &str| table.and_then(|table| text_of(table, name));
    Dependency {
        is_local: text("path").is_some(),
        path: text("path").and_then(|path| join(directory, path)),
        package: source_name(text("package").unwrap_or(key)),
        inherited: table
            .and_then(|table| table.get("workspace"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
    }
}

fn dependencies(directory: &str, table: &toml::Table, out: &mut BTreeMap<String, Dependency>) {
    for (key, value) in table {
        let found = dependency(directory, key, value);
        // One name can be in two tables. A table that gives a directory wins.
        let keep = out
            .get(&source_name(key))
            .is_some_and(|known| known.is_local && !found.is_local);
        if !keep {
            out.insert(source_name(key), found);
        }
    }
}

/// Reads the manifest in `directory`. A manifest that is not TOML gives `None`.
pub fn package(directory: &str, cargo_toml: &str) -> Option<Package> {
    let manifest = toml::from_str::<toml::Table>(cargo_toml).ok()?;
    let mut found = Package::default();
    let package = table_of(manifest.get("package"));
    let library = table_of(manifest.get("lib"));
    if let Some(package) = package {
        let package_name = text_of(package, "name").map(|name| name.replace('-', "_"));
        found.library_name = library
            .and_then(|library| text_of(library, "name"))
            .map(str::to_string)
            .or(package_name);
    }
    found.library_path = library
        .and_then(|library| text_of(library, "path"))
        .and_then(|path| join(directory, path));
    for kind in TARGET_TABLES {
        let targets = manifest.get(kind).and_then(toml::Value::as_array);
        for target in targets.into_iter().flatten() {
            let path = target.as_table().and_then(|target| text_of(target, "path"));
            found
                .target_paths
                .extend(path.and_then(|path| join(directory, path)));
        }
    }
    if let Some(package) = package {
        let build = text_of(package, "build").unwrap_or("build.rs");
        found.target_paths.extend(join(directory, build));
        for (automatic, switch) in AUTOMATIC_TARGETS {
            if package.get(switch).and_then(toml::Value::as_bool) != Some(false) {
                found.target_directories.extend(join(directory, automatic));
            }
        }
    }
    for kind in DEPENDENCY_TABLES {
        if let Some(table) = table_of(manifest.get(kind)) {
            dependencies(directory, table, &mut found.dependencies);
        }
    }
    for target in table_of(manifest.get("target"))
        .into_iter()
        .flat_map(|targets| targets.values())
        .filter_map(toml::Value::as_table)
    {
        for kind in DEPENDENCY_TABLES {
            if let Some(table) = table_of(target.get(kind)) {
                dependencies(directory, table, &mut found.dependencies);
            }
        }
    }
    if let Some(workspace) = table_of(manifest.get("workspace")) {
        let mut shared = BTreeMap::new();
        if let Some(table) = table_of(workspace.get("dependencies")) {
            dependencies(directory, table, &mut shared);
        }
        found.workspace_dependencies = Some(shared);
    }
    for registry in table_of(manifest.get("patch"))
        .into_iter()
        .flat_map(|patches| patches.values())
        .filter_map(toml::Value::as_table)
    {
        for (key, value) in registry {
            let patch = dependency(directory, key, value);
            if let Some(path) = patch.path {
                found.patches.insert(patch.package, path);
            }
        }
    }
    Some(found)
}

fn parent_directory(directory: &str) -> Option<&str> {
    if directory.is_empty() {
        return None;
    }
    Some(directory.rsplit_once('/').map_or("", |(parent, _)| parent))
}

/// The manifest in `directory` or in the nearest directory above it for which
/// `accept` is true.
fn nearest<'a>(
    packages: &'a BTreeMap<String, Package>,
    directory: &str,
    accept: impl Fn(&Package) -> bool,
) -> Option<(&'a str, &'a Package)> {
    let mut here = Some(directory);
    while let Some(current) = here {
        if let Some((found, package)) = packages.get_key_value(current) {
            if accept(package) {
                return Some((found.as_str(), package));
            }
        }
        here = parent_directory(current);
    }
    None
}

/// The source of the crate that Rust source in `directory` calls `name`.
///
/// The nearest manifest gives the answer: its own library, a dependency with
/// a `path` key, a dependency that the workspace gives a `path` key or a
/// `[patch]`, or a dependency from a registry. A crate of the repository that
/// the manifest does not make a dependency is not the answer. `libraries`
/// gives the directories of the packages for each library name.
pub fn crate_source(
    packages: &BTreeMap<String, Package>,
    libraries: &BTreeMap<String, BTreeSet<String>>,
    directory: &str,
    name: &str,
) -> CrateSource {
    let Some((here, manifest)) = nearest(packages, directory, |_| true) else {
        return CrateSource::NotDeclared;
    };
    if manifest.library_name.as_deref() == Some(name) {
        return CrateSource::Repository(here.to_string());
    }
    let workspace = nearest(packages, here, |package| {
        package.workspace_dependencies.is_some()
    })
    .map(|(_, workspace)| workspace);
    let shared = |name: &str| workspace?.workspace_dependencies.as_ref()?.get(name);
    let Some(declared) = manifest.dependencies.get(name).or_else(|| shared(name)) else {
        return CrateSource::NotDeclared;
    };
    let dependency = match (declared.inherited, shared(name)) {
        (true, Some(shared)) => shared,
        // The index does not have the table of the workspace. The one
        // package of the repository with the name is then the dependency.
        (true, None) => {
            let named = libraries.get(name).map(|directories| {
                let mut directories = directories.iter();
                (directories.next(), directories.next())
            });
            return match named {
                None => CrateSource::External,
                Some((Some(only), None)) => CrateSource::Repository(only.clone()),
                Some(_) => CrateSource::NotDeclared,
            };
        }
        (false, _) => declared,
    };
    let patched = [Some(manifest), workspace]
        .into_iter()
        .flatten()
        .find_map(|root| root.patches.get(&dependency.package));
    match dependency.path.as_ref().or(patched) {
        Some(path) => CrateSource::Repository(path.clone()),
        None if dependency.is_local => CrateSource::OutsideRepository,
        None => CrateSource::External,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifests(files: &[(&str, &str)]) -> crate::index::imports::Manifests {
        let mut manifests = crate::index::imports::Manifests::default();
        for (directory, text) in files {
            let path = join(directory, "Cargo.toml").expect("the directory is in the repository");
            manifests.add(&path, text);
        }
        manifests
    }

    const WORKSPACE: &str = "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\ntools = { path = \"crates/tools\" }\nserde = \"1\"\nfixed = { package = \"fixed-up\", version = \"1\" }\n\n[patch.crates-io]\nfixed-up = { path = \"vendor/fixed\" }\n";
    const APP: &str = "[package]\nname = \"app\"\n\n[dependencies]\ntools.workspace = true\nserde = { workspace = true }\nkit = { package = \"render-kit\", path = \"../render-kit\" }\nfixed.workspace = true\nlost.workspace = true\nlog = \"0.4\"\noutside = { path = \"../../../outside\" }\n\n[target.'cfg(unix)'.dev-dependencies]\nunix-only = { path = \"../unix\" }\n";

    /// The name of the library is `[lib] name`, else the name of the package
    /// with `_` for each `-`. Comments and each form of a TOML text are valid.
    #[test]
    fn test_package_reads_the_library_name_from_the_toml_tables() {
        let library = |manifest: &str| package("kit", manifest).unwrap().library_name;
        let commented = "[package] # the package\nname = \"render-kit\" # the name\n";
        assert_eq!(library(commented).as_deref(), Some("render_kit"));
        let single_quotes = "[package]\nname = 'render-kit'\n";
        assert_eq!(library(single_quotes).as_deref(), Some("render_kit"));
        let inline = "package = { name = \"render-kit\" }\n";
        assert_eq!(library(inline).as_deref(), Some("render_kit"));
        let named =
            "[package]\nname = \"render-kit\"\n\n[lib] # the library\nname = 'kit' # short\n";
        assert_eq!(library(named).as_deref(), Some("kit"));
        let dependency_name = "[package]\nname = \"app\"\n\n[dependencies.other]\nname = \"x\"\n";
        assert_eq!(library(dependency_name).as_deref(), Some("app"));
        assert_eq!(library("[workspace]\nmembers = []\n"), None);
    }

    #[test]
    fn test_package_reads_the_library_and_the_target_paths() {
        let manifest = "[package]\nname = \"remote-server\"\nautotests = false\n\n[lib]\npath = \"src/server.rs\"\n\n[[bin]]\nname = \"remote\"\npath = \"src/main.rs\"\n\n[[example]]\nname = \"demo\"\npath = \"demo/run.rs\"\n";
        let found = package("crates/remote", manifest).unwrap();
        assert_eq!(found.library_name.as_deref(), Some("remote_server"));
        assert_eq!(
            found.library_path.as_deref(),
            Some("crates/remote/src/server.rs")
        );
        assert_eq!(
            found.target_paths.iter().collect::<Vec<_>>(),
            [
                "crates/remote/build.rs",
                "crates/remote/demo/run.rs",
                "crates/remote/src/main.rs"
            ]
        );
        assert_eq!(
            found.target_directories,
            [
                "crates/remote/src/bin",
                "crates/remote/examples",
                "crates/remote/benches"
            ]
        );
    }

    #[test]
    fn test_path_key_in_each_form_of_toml_gives_the_directory() {
        let manifest = "[dependencies]\nlocal_alias.path = \"../local\"\nlocal_alias.package = \"local-pkg\"\ntabled = \"1\"\nspaced = { version = \"1\", path = \"x\" } # path\nalgo = { package = \"pathfinding\", version = \"4\" }\n\n[dependencies.long_form]\npath = \"../long\"\n\n[dependencies.long_external]\nversion = \"2\"\n";
        let found = package("crates/app", manifest).unwrap();
        let paths: Vec<(&str, Option<&str>)> = found
            .dependencies
            .iter()
            .map(|(name, dependency)| (name.as_str(), dependency.path.as_deref()))
            .collect();
        assert_eq!(
            paths,
            [
                ("algo", None),
                ("local_alias", Some("crates/local")),
                ("long_external", None),
                ("long_form", Some("crates/long")),
                ("spaced", Some("crates/app/x")),
                ("tabled", None),
            ]
        );
        assert_eq!(found.dependencies["local_alias"].package, "local_pkg");
        assert_eq!(found.dependencies["algo"].package, "pathfinding");
    }

    #[test]
    fn test_manifest_that_is_not_toml_gives_no_package() {
        assert_eq!(package("", "[package\nname = "), None);
    }

    #[test]
    fn test_crate_source_follows_the_manifest_of_the_file() {
        let manifests = manifests(&[("", WORKSPACE), ("crates/app", APP)]);
        let source = |name: &str| manifests.rust_crate("crates/app/src/main.rs", name);
        let repository = |directory: &str| CrateSource::Repository(directory.to_string());
        assert_eq!(source("app"), repository("crates/app"));
        assert_eq!(source("tools"), repository("crates/tools"));
        assert_eq!(source("kit"), repository("crates/render-kit"));
        assert_eq!(source("unix_only"), repository("crates/unix"));
        assert_eq!(source("fixed"), repository("vendor/fixed"));
        assert_eq!(source("serde"), CrateSource::External);
        assert_eq!(source("log"), CrateSource::External);
        assert_eq!(source("outside"), CrateSource::OutsideRepository);
        assert_eq!(source("lost"), CrateSource::External);
        assert_eq!(source("render_kit"), CrateSource::NotDeclared);
        assert_eq!(source("missing"), CrateSource::NotDeclared);
    }

    #[test]
    fn test_crate_source_of_a_file_without_a_manifest_is_not_declared() {
        let manifests = manifests(&[("crates/app", APP)]);
        assert_eq!(
            manifests.rust_crate("scripts/run.rs", "app"),
            CrateSource::NotDeclared
        );
    }

    /// `tools.workspace = true` has no table when the index does not have the
    /// manifest of the workspace. The name of a package is then the evidence.
    #[test]
    fn test_inherited_dependency_without_the_workspace_table() {
        let tools = "[package]\nname = \"tools\"\n";
        let one = manifests(&[("crates/app", APP), ("crates/tools", tools)]);
        assert_eq!(
            one.rust_crate("crates/app/src/main.rs", "tools"),
            CrateSource::Repository("crates/tools".to_string())
        );
        assert_eq!(
            one.rust_crate("crates/app/src/main.rs", "serde"),
            CrateSource::External
        );
        let two = manifests(&[("crates/app", APP), ("a/tools", tools), ("b/tools", tools)]);
        assert_eq!(
            two.rust_crate("crates/app/src/main.rs", "tools"),
            CrateSource::NotDeclared
        );
    }

    #[test]
    fn test_file_of_the_workspace_root_reads_the_workspace_dependencies() {
        let manifests = manifests(&[("", WORKSPACE)]);
        assert_eq!(
            manifests.rust_crate("tooling/run.rs", "serde"),
            CrateSource::External
        );
        assert_eq!(
            manifests.rust_crate("tooling/run.rs", "tools"),
            CrateSource::Repository("crates/tools".to_string())
        );
    }
}
