//! The symbol that each name of a file gives to an import, computed once for
//! each file.
//!
//! `Repository::symbol` follows the re-exports of a file for each name, thus
//! a barrel with many wildcard re-exports cost its size for each name. The
//! table of a file has the key of the one symbol of each name, from the
//! tables of the files that it re-exports. A file whose re-exports reach a
//! cycle has no table, and its names use `Repository::symbol`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use super::{ImportKind, ImportTarget, Repository};
use crate::core::Language;
use crate::index::model::SymbolKind;

/// The key of the one symbol of each name of a file.
pub(super) type SymbolTable<'a> = HashMap<&'a str, &'a str>;

/// The table of a file, or `None` when its re-exports reach a cycle.
pub(super) type SymbolTables<'a> = HashMap<usize, Option<Arc<SymbolTable<'a>>>>;

/// How a binding gives names of its target again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReExport {
    Named,
    Wildcard,
}

impl<'a> Repository<'a> {
    /// The key of the symbol that the name `name` of `file` gives to an import.
    pub(super) fn symbol_of(&self, file: &str, name: &str) -> Option<String> {
        let position = *self.by_path.get(file)?;
        match self.symbol_table(position) {
            Some(table) => table.get(name).map(|key| key.to_string()),
            None => self.symbol(file, name, &mut BTreeSet::new()),
        }
    }

    /// The re-exports of the file at `position`: the kind and the target.
    fn re_exports(
        &self,
        position: usize,
    ) -> impl Iterator<Item = (ReExport, usize, &'a super::ImportBinding)> + '_ {
        let facts = &self.files[position];
        let python = facts.language == Language::Python;
        facts.imports.iter().filter_map(move |binding| {
            let ImportTarget::Resolved(target) = &binding.target_file else {
                return None;
            };
            let named = binding.kind == ImportKind::ReExportNamed
                || (python && binding.kind == ImportKind::Named);
            let wildcard = (binding.kind == ImportKind::ReExportWildcard
                || (python && binding.kind == ImportKind::Wildcard))
                && binding.local_name.is_none();
            let kind = if named {
                ReExport::Named
            } else if wildcard {
                ReExport::Wildcard
            } else {
                return None;
            };
            Some((kind, *self.by_path.get(target.as_str())?, binding))
        })
    }

    /// The table of the file at `position`. The search visits each file one
    /// time and does not use recursion, because a chain of barrels can be long.
    fn symbol_table(&self, position: usize) -> Option<Arc<SymbolTable<'a>>> {
        if let Some(found) = self.symbol_tables.read().get(&position) {
            return found.clone();
        }
        struct Frame {
            file: usize,
            targets: Vec<usize>,
            next: usize,
            reaches_cycle: bool,
        }
        let targets = |file: usize| -> Vec<usize> {
            let mut found: Vec<usize> =
                self.re_exports(file).map(|(_, target, _)| target).collect();
            found.sort_unstable();
            found.dedup();
            found
        };
        let mut built: SymbolTables<'a> = HashMap::new();
        let known =
            |file: usize, built: &SymbolTables<'a>| -> Option<Option<Arc<SymbolTable<'a>>>> {
                built
                    .get(&file)
                    .cloned()
                    .or_else(|| self.symbol_tables.read().get(&file).cloned())
            };
        let mut searching: HashSet<usize> = HashSet::from([position]);
        let mut stack = vec![Frame {
            file: position,
            targets: targets(position),
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
                    targets: targets(target),
                    next: 0,
                    reaches_cycle: false,
                });
                continue;
            }
            let frame = stack.pop().expect("a frame");
            searching.remove(&frame.file);
            let tables: Option<HashMap<usize, Arc<SymbolTable<'a>>>> = if frame.reaches_cycle {
                None
            } else {
                frame
                    .targets
                    .iter()
                    .map(|target| Some((*target, known(*target, &built).flatten()?)))
                    .collect()
            };
            let table = tables.map(|tables| Arc::new(self.build_symbol_table(frame.file, &tables)));
            built.insert(frame.file, table);
        }
        let found = built.get(&position).cloned().flatten();
        self.symbol_tables.write().extend(built);
        found
    }

    /// The table of the file at `position` from the tables of the files that
    /// it re-exports. The rules are the rules of `Repository::symbol`.
    fn build_symbol_table(
        &self,
        position: usize,
        tables: &HashMap<usize, Arc<SymbolTable<'a>>>,
    ) -> SymbolTable<'a> {
        let facts = &self.files[position];
        let modules: HashSet<&str> = facts
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Module)
            .map(|symbol| symbol.name.as_str())
            .collect();
        let mut defined: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
        let mut aliased: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
        let mut in_module: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
        for (symbol, link) in facts.symbols.iter().zip(&facts.link_facts) {
            match &symbol.container {
                None => defined
                    .entry(symbol.name.as_str())
                    .or_default()
                    .push(&symbol.key),
                Some(container) if modules.contains(container.as_str()) => {
                    in_module
                        .entry(symbol.name.as_str())
                        .or_default()
                        .push(&symbol.key);
                }
                Some(_) => {}
            }
            for alias in &link.export_aliases {
                aliased.entry(alias.as_str()).or_default().push(&symbol.key);
            }
        }
        let mut re_exported: HashMap<&'a str, BTreeSet<&'a str>> = HashMap::new();
        for (kind, target, binding) in self.re_exports(position) {
            let table = &tables[&target];
            match kind {
                ReExport::Named => {
                    let Some(local) = binding.local_name.as_deref() else {
                        continue;
                    };
                    let source = binding.imported_name.as_deref().unwrap_or(local);
                    if let Some(key) = table.get(source) {
                        re_exported.entry(local).or_default().insert(key);
                    }
                }
                ReExport::Wildcard => {
                    for (name, key) in table.iter() {
                        re_exported.entry(name).or_default().insert(key);
                    }
                }
            }
        }
        let mut names: HashSet<&'a str> = HashSet::new();
        names.extend(defined.keys());
        names.extend(aliased.keys());
        names.extend(in_module.keys());
        names.extend(re_exported.keys());
        let mut table = HashMap::with_capacity(names.len());
        for name in names {
            // A definition and an alias decide the name, also when two
            // symbols have it. A module decides it only with one symbol.
            let found = if let Some(keys) = defined.get(name).or_else(|| aliased.get(name)) {
                match keys.as_slice() {
                    [only] => Some(*only),
                    _ => None,
                }
            } else if let Some([only]) = in_module.get(name).map(Vec::as_slice) {
                Some(*only)
            } else {
                match re_exported.get(name) {
                    Some(keys) if keys.len() == 1 => keys.iter().next().copied(),
                    _ => None,
                }
            };
            if let Some(key) = found {
                table.insert(name, key);
            }
        }
        table
    }
}
