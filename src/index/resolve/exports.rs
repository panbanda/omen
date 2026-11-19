//! The names that each file exports, computed once for each file.
//!
//! A query of one name of a barrel module read each re-export of the module,
//! thus a barrel with many wildcard re-exports cost its size for each name.
//! The table of a file has each name that it exports, with the definitions
//! of `Resolver::exported_name`. It is built from the tables of the files
//! that the file re-exports. A file whose re-exports reach a cycle has no
//! table, and its queries use `exported_name`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::{Family, Id, ImportBinding, ImportKind, Resolver, SymbolKind};

/// The definitions of each name that a file exports.
pub(super) type ExportTable<'a> = HashMap<&'a str, Vec<Id>>;

/// The table of a file, or `None` when its re-exports reach a cycle.
pub(super) type ExportTables<'a> = HashMap<usize, Option<Arc<ExportTable<'a>>>>;

/// How a binding gives names of its target again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReExport {
    Named,
    Wildcard,
}

impl<'a> Resolver<'a> {
    fn re_export(&self, file: usize, binding: &ImportBinding) -> Option<ReExport> {
        let python = self.family(file) == Family::Python;
        if binding.kind == ImportKind::ReExportNamed
            || (python && binding.kind == ImportKind::Named)
        {
            Some(ReExport::Named)
        } else if binding.kind == ImportKind::ReExportWildcard
            || (python && binding.kind == ImportKind::Wildcard)
        {
            Some(ReExport::Wildcard)
        } else {
            None
        }
    }

    /// The files whose tables the table of `file` reads.
    fn re_export_targets(&self, file: usize) -> Vec<usize> {
        let mut targets: Vec<usize> = self.files[file]
            .imports
            .iter()
            .filter(|binding| self.re_export(file, binding).is_some())
            .flat_map(|binding| self.target_files(binding))
            .collect();
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    /// The table of `file`. The search visits each file one time and does
    /// not use recursion, because a chain of barrels can be long.
    pub(super) fn export_table(&self, file: usize) -> Option<Arc<ExportTable<'a>>> {
        if let Some(found) = self.export_tables.read().get(&file) {
            return found.clone();
        }
        struct Frame {
            file: usize,
            targets: Vec<usize>,
            next: usize,
            reaches_cycle: bool,
        }
        let mut built: ExportTables<'a> = HashMap::new();
        let known =
            |file: usize, built: &ExportTables<'a>| -> Option<Option<Arc<ExportTable<'a>>>> {
                built
                    .get(&file)
                    .cloned()
                    .or_else(|| self.export_tables.read().get(&file).cloned())
            };
        let mut searching: HashSet<usize> = HashSet::from([file]);
        let mut stack = vec![Frame {
            file,
            targets: self.re_export_targets(file),
            next: 0,
            reaches_cycle: false,
        }];
        while let Some(frame) = stack.last_mut() {
            if frame.next < frame.targets.len() {
                let target = frame.targets[frame.next];
                frame.next += 1;
                if known(target, &built).is_some() {
                    continue;
                }
                if searching.contains(&target) {
                    // Each file on the stack reaches the cycle.
                    for frame in &mut stack {
                        frame.reaches_cycle = true;
                    }
                    continue;
                }
                searching.insert(target);
                stack.push(Frame {
                    file: target,
                    targets: self.re_export_targets(target),
                    next: 0,
                    reaches_cycle: false,
                });
                continue;
            }
            let frame = stack.pop().expect("a frame");
            searching.remove(&frame.file);
            let tables: Option<HashMap<usize, Arc<ExportTable<'a>>>> = if frame.reaches_cycle {
                None
            } else {
                frame
                    .targets
                    .iter()
                    .map(|target| Some((*target, known(*target, &built).flatten()?)))
                    .collect()
            };
            let table = tables.map(|tables| Arc::new(self.build_export_table(frame.file, &tables)));
            built.insert(frame.file, table);
        }
        let found = built.get(&file).cloned().flatten();
        self.export_tables.write().extend(built);
        found
    }

    /// The table of `file` from the tables of the files that it re-exports.
    fn build_export_table(
        &self,
        file: usize,
        tables: &HashMap<usize, Arc<ExportTable<'a>>>,
    ) -> ExportTable<'a> {
        let facts = &self.files[file];
        let mut defaults: Vec<Id> = Vec::new();
        let mut top_level: HashMap<&'a str, Vec<Id>> = HashMap::new();
        let mut aliased: HashMap<&'a str, Vec<Id>> = HashMap::new();
        let mut in_module: HashMap<&'a str, Vec<Id>> = HashMap::new();
        for (position, symbol) in facts.symbols.iter().enumerate() {
            let id = (file, position);
            let link = &facts.link_facts[position];
            if link.default_export {
                defaults.push(id);
            }
            if self.is_top_level(id) {
                top_level.entry(symbol.name.as_str()).or_default().push(id);
            }
            for alias in &link.export_aliases {
                aliased.entry(alias.as_str()).or_default().push(id);
            }
            let in_a_module = link
                .parent
                .is_some_and(|parent| facts.symbols[parent].kind == SymbolKind::Module);
            if in_a_module {
                in_module.entry(symbol.name.as_str()).or_default().push(id);
            }
        }
        let mut re_exported: HashMap<&'a str, Vec<Id>> = HashMap::new();
        for binding in &facts.imports {
            let Some(kind) = self.re_export(file, binding) else {
                continue;
            };
            for target in self.target_files(binding) {
                let table = &tables[&target];
                match kind {
                    ReExport::Named => {
                        let Some(local) = binding.local_name.as_deref() else {
                            continue;
                        };
                        let source = binding.imported_name.as_deref().unwrap_or(local);
                        if let Some(found) = table.get(source) {
                            re_exported.entry(local).or_default().extend(found);
                        }
                    }
                    ReExport::Wildcard => {
                        for (name, found) in table.iter() {
                            re_exported.entry(name).or_default().extend(found);
                        }
                    }
                }
            }
        }
        // The order of the rules is the order of `exported_name`: the first
        // rule that finds a definition gives the name.
        let own = |name: &str| -> Option<Vec<Id>> {
            if name == "default" && !defaults.is_empty() {
                return Some(defaults.clone());
            }
            if let Some(found) = top_level.get(name) {
                return Some(found.clone());
            }
            if let Some(found) = aliased.get(name) {
                return Some(found.clone());
            }
            match in_module.get(name).map(Vec::as_slice) {
                Some([only]) => Some(vec![*only]),
                _ => None,
            }
        };
        let mut names: HashSet<&'a str> = HashSet::new();
        names.extend(top_level.keys());
        names.extend(aliased.keys());
        names.extend(in_module.keys());
        names.extend(re_exported.keys());
        if !defaults.is_empty() {
            names.insert("default");
        }
        let mut table = HashMap::with_capacity(names.len());
        for name in names {
            let mut found = match own(name) {
                Some(found) => found,
                None => match re_exported.remove(name) {
                    Some(found) => found,
                    None => continue,
                },
            };
            found.sort_unstable();
            found.dedup();
            table.insert(name, found);
        }
        table
    }
}

#[cfg(test)]
mod timing_tests {
    use crate::index::resolve::testing::rows;
    use crate::index::timing::{assert_scales_linearly, Stopwatch};

    /// `size` modules with one function each, a barrel that re-exports each
    /// module with a wildcard, and a file that imports each function from
    /// the barrel and calls it.
    fn wildcard_barrel_at(size: usize) -> std::time::Duration {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut barrel = String::new();
        let mut names = Vec::new();
        let mut calls = String::new();
        for position in 0..size {
            files.push((
                format!("m{position}.ts"),
                format!("export function f{position}() {{}}\n"),
            ));
            barrel.push_str(&format!("export * from './m{position}';\n"));
            names.push(format!("f{position}"));
            calls.push_str(&format!("\tf{position}();\n"));
        }
        files.push(("index.ts".to_string(), barrel));
        files.push((
            "view.ts".to_string(),
            format!(
                "import {{ {} }} from './index';\n\nexport function view() {{\n{calls}}}\n",
                names.join(", ")
            ),
        ));
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let started = Stopwatch::start();
        let found = rows(&files, "view.ts");
        let elapsed = started.elapsed();
        assert_eq!(found.len(), size);
        let last = size - 1;
        assert_eq!(
            found[last],
            format!(
                "{} view>f{last} UniqueCandidate ImportedName [m{last}.ts#f{last}]",
                4 + last
            )
        );
        elapsed
    }

    #[test]
    fn test_timing_case_resolves_each_call() {
        wildcard_barrel_at(3);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_calls_through_a_wildcard_barrel_resolve_in_linear_time() {
        assert_scales_linearly("wildcard barrel", 4_000, wildcard_barrel_at);
    }
}
