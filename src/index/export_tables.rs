//! The tables of the names that the files export, for the import resolution
//! and for the call resolution.
//!
//! The table of a file gives each name that the file exports. The rules are
//! the rules of `ResolveExport` of ECMAScript:
//! - An own definition or a named re-export of a file decides its name.
//! - Else the wildcard re-exports of the file give the union of what their
//!   files give. Two different definitions make the name ambiguous.
//! - A wildcard re-export of ECMAScript does not give `default`.
//! - A search that comes to a file and a name again gets nothing there. Thus
//!   in a cycle, a file that defines a name stops the search of that name.
//!
//! The tables are persistent hash tries. The table of a file starts from the
//! largest table of the files that it re-exports with a wildcard, by
//! reference, and inserts the entries of the other tables into it. A chain
//! of barrels thus costs `O(log n)` new nodes for each barrel.
//!
//! The exact union of the wildcard re-exports includes the reachability in a
//! directed acyclic graph. No algorithm is known that does it in
//! `O(n log n)` for each graph, thus the build has a cap on its work:
//! `CAP_FACTOR * size * log2(size)` entries, where `size` counts the files,
//! their own names, and their re-exports. The build goes through the files
//! in one fixed order. A file whose work is more than what remains of the
//! cap, and each file that re-exports such a file with a wildcard, keeps
//! only its own names and its named re-exports. `Tables::not_resolved` gives
//! these files. The result thus depends only on the graph.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{BuildHasherDefault, DefaultHasher};
use std::sync::Arc;

/// The definitions of one name, sorted and without duplicates.
pub(crate) type Values<T> = Arc<Vec<T>>;

/// A persistent map from a name to its definitions. A clone shares each
/// node. `DefaultHasher` has fixed keys, thus the order of the entries does
/// not change between runs.
pub(crate) type Table<'a, T> =
    rpds::HashTrieMapSync<&'a str, Values<T>, BuildHasherDefault<DefaultHasher>>;

fn empty_table<'a, T>() -> Table<'a, T> {
    Table::new_with_hasher_and_ptr_kind(BuildHasherDefault::default())
}

/// The work of the build is at most this factor times `size * log2(size)`.
/// When each file is the target of at most one wildcard re-export, a name
/// moves only from a table into one at least twice as large, thus the work
/// is at most `size * log2(size)`. The factor 4 keeps each such graph exact
/// with a margin. A graph above the cap re-exports the same names through
/// many paths.
const CAP_FACTOR: usize = 4;

/// The facts of one file that its table needs.
#[derive(Clone)]
pub(crate) struct Member<'a, T> {
    /// The names that the file defines itself, which come before each re-export.
    pub own: Vec<(&'a str, Vec<T>)>,
    /// The files that the file re-exports with a wildcard.
    pub wildcards: Vec<usize>,
    /// The re-exports of one name: the local name, the file, and the name in that file.
    pub named: Vec<(&'a str, usize, &'a str)>,
    /// The wildcard re-exports of the file do not give `default`, as in ECMAScript.
    pub hides_default: bool,
}

/// The facts of the files of one kind of table.
pub(crate) trait Exports<'a, T> {
    /// The number of the files.
    fn files(&self) -> usize;

    fn member(&self, file: usize) -> Member<'a, T>;
}

/// The table of each file.
pub(crate) struct Tables<'a, T> {
    tables: Vec<Table<'a, T>>,
    not_resolved: Vec<usize>,
    /// The entries that the build read from tables that it did not share.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) work: usize,
    /// The insertions into a table and the removals from it. Each one makes
    /// `O(log n)` new nodes.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) writes: usize,
}

impl<'a, T: Clone + Ord> Tables<'a, T> {
    pub(crate) fn build(exports: &impl Exports<'a, T>) -> Self {
        Self::build_with(exports, None)
    }

    #[cfg(test)]
    pub(crate) fn build_with_cap(exports: &impl Exports<'a, T>, cap: usize) -> Self {
        Self::build_with(exports, Some(cap))
    }

    fn build_with(exports: &impl Exports<'a, T>, cap: Option<usize>) -> Self {
        // Not in parallel: a query that runs on a thread of the pool builds
        // the tables in a `OnceLock`. A parallel build there can make the
        // same thread run a second query, which then waits for itself.
        let members: Vec<Facts<'a, T>> = (0..exports.files())
            .map(|file| Facts::of(exports.member(file)))
            .collect();
        let size: usize = members
            .iter()
            .map(|member| 1 + member.own.len() + member.wildcards.len() + member.named.len())
            .sum();
        let log = (usize::BITS - size.leading_zeros()) as usize;
        let cap = cap.unwrap_or(CAP_FACTOR * size * log);
        let mut builder = Builder {
            tables: vec![None; members.len()],
            incomplete: vec![false; members.len()],
            members,
            remaining: cap,
            work: 0,
            writes: 0,
        };
        builder.run();
        let not_resolved = (0..builder.members.len())
            .filter(|file| builder.incomplete[*file])
            .collect();
        Self {
            tables: builder
                .tables
                .into_iter()
                .map(|table| table.unwrap_or_else(empty_table))
                .collect(),
            not_resolved,
            work: builder.work,
            writes: builder.writes,
        }
    }

    /// The definitions that the name `name` of `file` gives.
    pub(crate) fn get(&self, file: usize, name: &str) -> Option<&Values<T>> {
        self.tables.get(file)?.get(name)
    }

    /// The files that keep only their own names and their named re-exports,
    /// because their tables need more work than the cap gives.
    pub(crate) fn not_resolved(&self) -> &[usize] {
        &self.not_resolved
    }
}

/// The facts of one file, with the definitions of each name sorted.
struct Facts<'a, T> {
    own: Vec<(&'a str, Values<T>)>,
    /// Sorted and without duplicates.
    wildcards: Vec<usize>,
    named: Vec<(&'a str, usize, &'a str)>,
    hides_default: bool,
}

impl<'a, T: Clone + Ord> Facts<'a, T> {
    fn of(member: Member<'a, T>) -> Self {
        let mut own: BTreeMap<&'a str, Vec<T>> = BTreeMap::new();
        for (name, values) in member.own {
            own.entry(name).or_default().extend(values);
        }
        let mut wildcards = member.wildcards;
        wildcards.sort_unstable();
        wildcards.dedup();
        Self {
            own: own
                .into_iter()
                .map(|(name, values)| (name, Arc::new(sorted(values))))
                .collect(),
            wildcards,
            named: member.named,
            hides_default: member.hides_default,
        }
    }

    /// The files that the file re-exports, sorted and without duplicates.
    fn targets(&self) -> Vec<usize> {
        let mut targets: Vec<usize> = self
            .wildcards
            .iter()
            .copied()
            .chain(self.named.iter().map(|(_, target, _)| *target))
            .collect();
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    /// Tells if a wildcard re-export of the file gives `name`.
    fn passes(&self, name: &str) -> bool {
        !(self.hides_default && name == "default")
    }
}

fn sorted<T: Ord>(mut values: Vec<T>) -> Vec<T> {
    values.sort_unstable();
    values.dedup();
    values
}

const UNSEEN: usize = usize::MAX;

/// The state of the search for strongly connected components.
struct Search {
    indices: Vec<usize>,
    lowest: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    /// The file, its targets, and the position of the next target.
    calls: Vec<(usize, Vec<usize>, usize)>,
    next_index: usize,
}

impl Search {
    fn enter(&mut self, file: usize, targets: Vec<usize>) {
        self.indices[file] = self.next_index;
        self.lowest[file] = self.next_index;
        self.next_index += 1;
        self.on_stack[file] = true;
        self.stack.push(file);
        self.calls.push((file, targets, 0));
    }
}

struct Builder<'a, T> {
    members: Vec<Facts<'a, T>>,
    tables: Vec<Option<Table<'a, T>>>,
    incomplete: Vec<bool>,
    remaining: usize,
    work: usize,
    writes: usize,
}

impl<'a, T: Clone + Ord> Builder<'a, T> {
    /// Builds the table of each file. The search is the algorithm of Tarjan
    /// for strongly connected components, without recursion, because a
    /// chain of barrels can be long. A component gets its tables when the
    /// search leaves it, thus each file that it re-exports outside of it has
    /// a table then.
    fn run(&mut self) {
        let count = self.members.len();
        let mut search = Search {
            indices: vec![UNSEEN; count],
            lowest: vec![0; count],
            on_stack: vec![false; count],
            stack: Vec::new(),
            calls: Vec::new(),
            next_index: 0,
        };
        for start in 0..count {
            if search.indices[start] != UNSEEN {
                continue;
            }
            search.enter(start, self.members[start].targets());
            while let Some((file, targets, next)) = search.calls.last_mut() {
                let file = *file;
                if let Some(target) = targets.get(*next).copied() {
                    *next += 1;
                    if search.indices[target] == UNSEEN {
                        search.enter(target, self.members[target].targets());
                    } else if search.on_stack[target] {
                        search.lowest[file] = search.lowest[file].min(search.indices[target]);
                    }
                    continue;
                }
                search.calls.pop();
                if let Some((parent, _, _)) = search.calls.last() {
                    let parent = *parent;
                    search.lowest[parent] = search.lowest[parent].min(search.lowest[file]);
                }
                if search.lowest[file] == search.indices[file] {
                    let split = search
                        .stack
                        .iter()
                        .rposition(|member| *member == file)
                        .expect("the file is on the stack");
                    let mut component = search.stack.split_off(split);
                    for member in &component {
                        search.on_stack[*member] = false;
                    }
                    component.sort_unstable();
                    self.component(&component);
                }
            }
        }
    }

    fn table(&self, file: usize) -> &Table<'a, T> {
        self.tables[file]
            .as_ref()
            .expect("a file outside the component has a table")
    }

    fn size(&self, file: usize) -> usize {
        self.table(file).size()
    }

    /// The definitions that the named re-exports of `file` give, for each
    /// local name. A file in `inside` has no table yet and gives nothing.
    fn named_values(&self, file: usize, inside: &HashSet<usize>) -> BTreeMap<&'a str, Vec<T>> {
        let mut found: BTreeMap<&'a str, Vec<T>> = BTreeMap::new();
        for (local, target, source) in &self.members[file].named {
            let values = found.entry(local).or_default();
            if !inside.contains(target) {
                if let Some(given) = self.table(*target).get(*source) {
                    values.extend(given.iter().cloned());
                }
            }
        }
        found
    }

    /// Puts the named re-exports and then the own names of `file` into
    /// `table`, and gives `file` the table.
    fn finish(&mut self, file: usize, mut table: Table<'a, T>, named: BTreeMap<&'a str, Vec<T>>) {
        for (local, values) in named {
            table.insert_mut(local, Arc::new(sorted(values)));
            self.writes += 1;
        }
        for (name, values) in &self.members[file].own {
            table.insert_mut(name, Arc::clone(values));
            self.writes += 1;
        }
        self.tables[file] = Some(table);
    }

    /// Takes `cost` from the cap, or tells that the cap does not have it.
    fn charge(&mut self, cost: usize) -> bool {
        if cost > self.remaining {
            return false;
        }
        self.remaining -= cost;
        self.work += cost;
        true
    }

    /// Adds `added` to the definitions of each name in `table`.
    fn merge(&mut self, table: &mut Table<'a, T>, added: BTreeMap<&'a str, Vec<T>>) {
        for (name, mut values) in added {
            let existing = table.get(name);
            if let Some(existing) = existing {
                values.extend(existing.iter().cloned());
            }
            let values = sorted(values);
            if values.is_empty() || existing.is_some_and(|existing| **existing == values) {
                continue;
            }
            table.insert_mut(name, Arc::new(values));
            self.writes += 1;
        }
    }

    /// The largest table of `targets` (the first file of the largest ones),
    /// and the cost of the entries of the other tables.
    fn base_and_cost(
        &self,
        targets: impl Iterator<Item = usize> + Clone,
    ) -> (Option<usize>, usize) {
        let base = targets
            .clone()
            .max_by_key(|target| (self.size(*target), std::cmp::Reverse(*target)));
        let cost = targets
            .filter(|target| Some(*target) != base)
            .map(|target| self.size(target))
            .sum();
        (base, cost)
    }

    /// The table of `base`, without `default` if `hides_default`.
    fn start(&mut self, base: Option<usize>, hides_default: bool) -> Table<'a, T> {
        let mut table = base.map_or_else(empty_table, |base| self.table(base).clone());
        if hides_default && table.remove_mut("default") {
            self.writes += 1;
        }
        table
    }

    fn component(&mut self, files: &[usize]) {
        let inside: HashSet<usize> = files.iter().copied().collect();
        let cycle = files.len() > 1 || self.members[files[0]].targets().contains(&files[0]);
        if cycle {
            self.cycle(files, &inside);
        } else {
            self.single(files[0], &inside);
        }
    }

    fn abstain(&mut self, files: &[usize], inside: &HashSet<usize>) {
        for file in files {
            self.incomplete[*file] = true;
            let named = self.named_values(*file, inside);
            self.finish(*file, empty_table(), named);
        }
    }

    /// The table of a file outside of each cycle.
    fn single(&mut self, file: usize, inside: &HashSet<usize>) {
        let member = &self.members[file];
        let hides_default = member.hides_default;
        let stars = member.wildcards.clone();
        if stars.iter().any(|target| self.incomplete[*target]) {
            return self.abstain(&[file], inside);
        }
        let (base, cost) = self.base_and_cost(stars.iter().copied());
        if !self.charge(cost) {
            return self.abstain(&[file], inside);
        }
        let mut added: BTreeMap<&'a str, Vec<T>> = BTreeMap::new();
        for target in stars.iter().filter(|target| Some(**target) != base) {
            for (name, values) in self.table(*target).iter() {
                if self.members[file].passes(name) {
                    added
                        .entry(name)
                        .or_default()
                        .extend(values.iter().cloned());
                }
            }
        }
        let mut table = self.start(base, hides_default);
        self.merge(&mut table, added);
        let named = self.named_values(file, inside);
        self.finish(file, table, named);
    }

    /// The tables of the files of one cycle.
    fn cycle(&mut self, files: &[usize], inside: &HashSet<usize>) {
        let outside: BTreeSet<usize> = files
            .iter()
            .flat_map(|file| self.members[*file].wildcards.iter().copied())
            .filter(|target| !inside.contains(target))
            .collect();
        if outside.iter().any(|target| self.incomplete[*target]) {
            return self.abstain(files, inside);
        }
        let named_inside = files.iter().any(|file| {
            self.members[*file]
                .named
                .iter()
                .any(|(_, target, _)| inside.contains(target))
        });
        let built = if named_inside {
            self.search_each_name(files, inside, &outside)
        } else {
            self.share_names(files, inside, &outside)
        };
        match built {
            Some(tables) => {
                for (file, table) in files.iter().zip(tables) {
                    self.tables[*file] = Some(table);
                }
            }
            None => self.abstain(files, inside),
        }
    }

    /// The tables of a cycle whose files re-export each other only with
    /// wildcards. Each file of the cycle reaches each other file. Thus a
    /// name with one source in the cycle gives that source in each file,
    /// and the files share these names. Only a name with two sources, of
    /// which one is a definition in the cycle, needs a search for each source.
    fn share_names(
        &mut self,
        files: &[usize],
        inside: &HashSet<usize>,
        outside: &BTreeSet<usize>,
    ) -> Option<Vec<Table<'a, T>>> {
        let (base, cost) = self.base_and_cost(outside.iter().copied());
        if !self.charge(cost) {
            return None;
        }
        // The names that each file of the cycle decides itself.
        let locals: HashMap<usize, BTreeMap<&'a str, Vec<T>>> = files
            .iter()
            .map(|file| {
                let mut local = self.named_values(*file, inside);
                for (name, values) in &self.members[*file].own {
                    local.insert(name, values.to_vec());
                }
                (*file, local)
            })
            .collect();
        let mut definers: BTreeMap<&'a str, Vec<usize>> = BTreeMap::new();
        for file in files {
            for name in locals[file].keys() {
                definers.entry(name).or_default().push(*file);
            }
        }
        let mut given: BTreeMap<&'a str, Vec<T>> = BTreeMap::new();
        for target in outside.iter().filter(|target| Some(**target) != base) {
            for (name, values) in self.table(*target).iter() {
                given
                    .entry(name)
                    .or_default()
                    .extend(values.iter().cloned());
            }
        }
        let base_table = base.map(|base| self.table(base));
        let has_default = definers.contains_key("default")
            || given.contains_key("default")
            || base_table.is_some_and(|table| table.contains_key("default"));
        let mut searched: BTreeSet<&'a str> = definers
            .iter()
            .filter(|(name, found)| {
                found.len() > 1
                    || given.contains_key(*name)
                    || base_table.is_some_and(|table| table.contains_key(**name))
            })
            .map(|(name, _)| *name)
            .collect();
        if has_default {
            searched.insert("default");
        }
        let mut shared = self.start(base, false);
        if shared.remove_mut("default") {
            self.writes += 1;
        }
        let mut added: BTreeMap<&'a str, Vec<T>> = given
            .into_iter()
            .filter(|(name, _)| !searched.contains(name))
            .collect();
        for (name, found) in &definers {
            if !searched.contains(name) {
                added.insert(name, locals[&found[0]][name].clone());
            }
        }
        self.merge(&mut shared, added);
        let mut predecessors: HashMap<usize, Vec<usize>> = HashMap::new();
        for file in files {
            for target in &self.members[*file].wildcards {
                if inside.contains(target) {
                    predecessors.entry(*target).or_default().push(*file);
                }
            }
        }
        let edges: usize = predecessors.values().map(Vec::len).sum();
        let mut found: HashMap<(usize, &'a str), Vec<T>> = HashMap::new();
        for name in &searched {
            let is_definer = |file: usize| locals[&file].contains_key(name);
            // A source is a definition in the cycle, or a file of the cycle
            // that re-exports a file outside of it that gives the name.
            let mut sources: Vec<(usize, Vec<T>)> = definers
                .get(name)
                .into_iter()
                .flatten()
                .map(|file| (*file, locals[file][name].clone()))
                .collect();
            for file in files.iter().filter(|file| !is_definer(**file)) {
                let member = &self.members[*file];
                if !member.passes(name) {
                    continue;
                }
                for target in member
                    .wildcards
                    .iter()
                    .filter(|target| !inside.contains(target))
                {
                    if let Some(values) = self.table(*target).get(*name) {
                        sources.push((*file, values.to_vec()));
                    }
                }
            }
            if !self.charge(sources.len() * (edges + files.len())) {
                return None;
            }
            // Each file that reaches a source without a definition of the
            // name on the way gets the source.
            for (source, values) in sources {
                let mut seen: HashSet<usize> = HashSet::from([source]);
                if !is_definer(source) {
                    found
                        .entry((source, name))
                        .or_default()
                        .extend(values.iter().cloned());
                }
                let mut pending = vec![source];
                while let Some(current) = pending.pop() {
                    for before in predecessors.get(&current).into_iter().flatten() {
                        if is_definer(*before)
                            || !self.members[*before].passes(name)
                            || !seen.insert(*before)
                        {
                            continue;
                        }
                        found
                            .entry((*before, name))
                            .or_default()
                            .extend(values.iter().cloned());
                        pending.push(*before);
                    }
                }
            }
        }
        let mut tables = Vec::with_capacity(files.len());
        for file in files {
            let mut table = shared.clone();
            for name in &searched {
                match found.remove(&(*file, *name)) {
                    Some(values) if !values.is_empty() => {
                        table.insert_mut(name, Arc::new(sorted(values)));
                        self.writes += 1;
                    }
                    _ => {
                        if table.remove_mut(*name) {
                            self.writes += 1;
                        }
                    }
                }
            }
            let named = self.named_values(*file, inside);
            for (local, values) in named {
                table.insert_mut(local, Arc::new(sorted(values)));
                self.writes += 1;
            }
            for (name, values) in &self.members[*file].own {
                table.insert_mut(name, Arc::clone(values));
                self.writes += 1;
            }
            tables.push(table);
        }
        Some(tables)
    }

    /// The tables of a cycle with a named re-export of a file of the cycle.
    /// The search follows `ResolveExport` for each file and each name.
    fn search_each_name(
        &mut self,
        files: &[usize],
        inside: &HashSet<usize>,
        outside: &BTreeSet<usize>,
    ) -> Option<Vec<Table<'a, T>>> {
        let cost: usize = outside.iter().map(|target| self.size(*target)).sum();
        if !self.charge(cost) {
            return None;
        }
        let mut universe: BTreeSet<&'a str> = BTreeSet::new();
        for file in files {
            let member = &self.members[*file];
            universe.extend(member.own.iter().map(|(name, _)| *name));
            universe.extend(member.named.iter().map(|(local, _, _)| *local));
        }
        for target in outside {
            universe.extend(self.table(*target).keys().copied());
        }
        let mut tables = Vec::with_capacity(files.len());
        for file in files {
            let mut table = empty_table();
            for name in &universe {
                let mut values: Vec<T> = Vec::new();
                let mut seen: HashSet<(usize, &'a str)> = HashSet::from([(*file, *name)]);
                let mut pending = vec![(*file, *name)];
                while let Some((current, wanted)) = pending.pop() {
                    if !self.charge(1) {
                        return None;
                    }
                    let member = &self.members[current];
                    if let Some((_, own)) = member.own.iter().find(|(own, _)| *own == wanted) {
                        values.extend(own.iter().cloned());
                        continue;
                    }
                    let mut next: Vec<(usize, &'a str)> = Vec::new();
                    let named: Vec<_> = member
                        .named
                        .iter()
                        .filter(|(local, _, _)| *local == wanted)
                        .collect();
                    if !named.is_empty() {
                        for (_, target, source) in named {
                            next.push((*target, *source));
                        }
                    } else if member.passes(wanted) {
                        next.extend(member.wildcards.iter().map(|target| (*target, wanted)));
                    }
                    for (target, wanted) in next {
                        if !inside.contains(&target) {
                            if let Some(given) = self.table(target).get(wanted) {
                                values.extend(given.iter().cloned());
                            }
                        } else if seen.insert((target, wanted)) {
                            pending.push((target, wanted));
                        }
                    }
                }
                let values = sorted(values);
                if !values.is_empty() {
                    table.insert_mut(*name, Arc::new(values));
                    self.writes += 1;
                }
            }
            tables.push(table);
        }
        Some(tables)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A graph of files with the facts that a test gives.
    struct Graph(Vec<Member<'static, usize>>);

    impl Exports<'static, usize> for Graph {
        fn files(&self) -> usize {
            self.0.len()
        }

        fn member(&self, file: usize) -> Member<'static, usize> {
            self.0[file].clone()
        }
    }

    fn file(
        own: &[(&'static str, usize)],
        wildcards: &[usize],
        named: &[(&'static str, usize, &'static str)],
    ) -> Member<'static, usize> {
        Member {
            own: own.iter().map(|(name, id)| (*name, vec![*id])).collect(),
            wildcards: wildcards.to_vec(),
            named: named.to_vec(),
            hides_default: true,
        }
    }

    fn names(tables: &Tables<'_, usize>, file: usize, name: &str) -> Vec<usize> {
        tables
            .get(file, name)
            .map(|values| values.to_vec())
            .unwrap_or_default()
    }

    #[test]
    fn test_own_names_shadow_and_two_definitions_are_ambiguous_in_a_cycle() {
        // a -> b -> c -> a. The search of `x` from b ends at c, which defines
        // `x`, thus b does not reach the `x` of a.
        let tables = Tables::build(&Graph(vec![
            file(&[("x", 10), ("y", 11)], &[1], &[]),
            file(&[("y", 12)], &[2], &[]),
            file(&[("x", 13)], &[0], &[]),
            file(&[], &[0, 2], &[]),
        ]));
        assert_eq!(names(&tables, 0, "x"), [10]);
        assert_eq!(names(&tables, 1, "x"), [13]);
        assert_eq!(names(&tables, 2, "x"), [13]);
        assert_eq!(names(&tables, 1, "y"), [12]);
        assert_eq!(names(&tables, 2, "y"), [11]);
        assert_eq!(names(&tables, 3, "x"), [10, 13]);
        assert!(tables.not_resolved().is_empty());
    }

    /// Cycles of wildcard re-exports, with the answer of `ResolveExport`
    /// for each file and the name `x`.
    #[test]
    fn test_cycle_cases_give_the_answer_of_resolve_export() {
        type Case = (Vec<Member<'static, usize>>, [&'static [usize]; 3]);
        let cases: Vec<Case> = vec![
            // A -> B -> C -> A; B and C define `x`. A sees B only.
            (
                vec![
                    file(&[], &[1], &[]),
                    file(&[("x", 1)], &[2], &[]),
                    file(&[("x", 2)], &[0], &[]),
                ],
                [&[1], &[1], &[2]],
            ),
            // A <-> B, A -> C; only C defines `x`.
            (
                vec![
                    file(&[], &[1, 2], &[]),
                    file(&[], &[0], &[]),
                    file(&[("x", 2)], &[], &[]),
                ],
                [&[2], &[2], &[2]],
            ),
            // A <-> B, A -> C, B -> C; B and C define `x`.
            (
                vec![
                    file(&[], &[1, 2], &[]),
                    file(&[("x", 1)], &[0, 2], &[]),
                    file(&[("x", 2)], &[], &[]),
                ],
                [&[1, 2], &[1], &[2]],
            ),
            // A <-> B <-> C; A and C define `x`. B sees both.
            (
                vec![
                    file(&[("x", 0)], &[1], &[]),
                    file(&[], &[0, 2], &[]),
                    file(&[("x", 2)], &[1], &[]),
                ],
                [&[0], &[0, 2], &[2]],
            ),
            // A -> B -> C -> A; no file defines `x`.
            (
                vec![
                    file(&[], &[1], &[]),
                    file(&[], &[2], &[]),
                    file(&[], &[0], &[]),
                ],
                [&[], &[], &[]],
            ),
        ];
        for (members, expected) in cases {
            let tables = Tables::build(&Graph(members));
            let found: Vec<Vec<usize>> = (0..3).map(|member| names(&tables, member, "x")).collect();
            assert_eq!(found, expected.map(<[usize]>::to_vec));
        }
    }

    #[test]
    fn test_a_definition_in_a_cycle_stops_the_search() {
        // a -> b -> c -> a, and c -> d. b defines `x`, and d defines `x`.
        let tables = Tables::build(&Graph(vec![
            file(&[], &[1], &[]),
            file(&[("x", 20)], &[2], &[]),
            file(&[], &[0, 3], &[]),
            file(&[("x", 30)], &[], &[]),
        ]));
        assert_eq!(names(&tables, 0, "x"), [20]);
        assert_eq!(names(&tables, 1, "x"), [20]);
        assert_eq!(names(&tables, 2, "x"), [20, 30]);
    }

    #[test]
    fn test_a_cycle_gives_each_name_that_one_file_gives() {
        let tables = Tables::build(&Graph(vec![
            file(&[("a", 1)], &[1], &[]),
            file(&[("b", 2)], &[2, 3], &[]),
            file(&[("c", 3)], &[0], &[]),
            file(&[("d", 4)], &[], &[]),
        ]));
        for member in 0..3 {
            assert_eq!(names(&tables, member, "a"), [1]);
            assert_eq!(names(&tables, member, "c"), [3]);
            assert_eq!(names(&tables, member, "d"), [4]);
        }
    }

    #[test]
    fn test_a_named_re_export_comes_before_a_wildcard() {
        let tables = Tables::build(&Graph(vec![
            file(&[("x", 1)], &[], &[]),
            file(&[("x", 2)], &[], &[]),
            file(&[], &[0], &[("x", 1, "x")]),
            file(&[], &[0], &[("x", 1, "missing")]),
        ]));
        assert_eq!(names(&tables, 2, "x"), [2]);
        assert_eq!(names(&tables, 3, "x"), Vec::<usize>::new());
    }

    #[test]
    fn test_a_wildcard_does_not_give_default() {
        let mut python = file(&[], &[0], &[]);
        python.hides_default = false;
        let tables = Tables::build(&Graph(vec![
            file(&[("default", 1), ("f", 2)], &[], &[]),
            file(&[], &[0], &[]),
            file(&[], &[3], &[]),
            file(&[("default", 5)], &[2], &[]),
            python,
        ]));
        assert_eq!(names(&tables, 1, "default"), Vec::<usize>::new());
        assert_eq!(names(&tables, 1, "f"), [2]);
        assert_eq!(names(&tables, 2, "default"), Vec::<usize>::new());
        assert_eq!(names(&tables, 3, "default"), [5]);
        assert_eq!(names(&tables, 4, "default"), [1]);
    }

    #[test]
    fn test_an_ambiguous_name_stays_ambiguous_through_a_wildcard() {
        let tables = Tables::build(&Graph(vec![
            file(&[("x", 1)], &[], &[]),
            file(&[("x", 2)], &[], &[]),
            file(&[], &[0, 1], &[]),
            file(&[("x", 3)], &[], &[]),
            file(&[], &[2, 3], &[]),
            file(&[], &[0, 0], &[]),
        ]));
        assert_eq!(names(&tables, 4, "x"), [1, 2, 3]);
        assert_eq!(names(&tables, 5, "x"), [1]);
    }

    #[test]
    fn test_named_re_exports_in_a_cycle() {
        let tables = Tables::build(&Graph(vec![
            file(&[], &[1], &[("y", 1, "x")]),
            file(&[("x", 7)], &[0], &[]),
            // A named re-export that names itself again gives nothing.
            file(&[], &[], &[("z", 3, "z")]),
            file(&[], &[], &[("z", 2, "z")]),
            // A file that re-exports itself.
            file(&[("w", 9)], &[4], &[]),
        ]));
        assert_eq!(names(&tables, 0, "y"), [7]);
        assert_eq!(names(&tables, 0, "x"), [7]);
        assert_eq!(names(&tables, 1, "y"), [7]);
        assert_eq!(names(&tables, 1, "x"), [7]);
        assert_eq!(names(&tables, 2, "z"), Vec::<usize>::new());
        assert_eq!(names(&tables, 3, "z"), Vec::<usize>::new());
        assert_eq!(names(&tables, 4, "w"), [9]);
        assert!(tables.not_resolved().is_empty());
    }

    #[derive(Debug, PartialEq)]
    enum Resolution {
        Missing,
        Ambiguous,
        Found(usize),
    }

    /// `ResolveExport` of ECMAScript, as the specification writes it.
    fn resolve_export(
        graph: &Graph,
        file: usize,
        name: &'static str,
        seen: &mut Vec<(usize, &'static str)>,
    ) -> Resolution {
        if seen.contains(&(file, name)) {
            return Resolution::Missing;
        }
        seen.push((file, name));
        let member = &graph.0[file];
        if let Some((_, ids)) = member.own.iter().find(|(own, _)| *own == name) {
            return Resolution::Found(ids[0]);
        }
        if let Some((_, target, source)) = member.named.iter().find(|(local, _, _)| *local == name)
        {
            return resolve_export(graph, *target, source, seen);
        }
        if name == "default" {
            return Resolution::Missing;
        }
        let mut found = Resolution::Missing;
        for target in &member.wildcards {
            match resolve_export(graph, *target, name, seen) {
                Resolution::Ambiguous => return Resolution::Ambiguous,
                Resolution::Missing => {}
                Resolution::Found(id) => match found {
                    Resolution::Found(earlier) if earlier != id => return Resolution::Ambiguous,
                    _ => found = Resolution::Found(id),
                },
            }
        }
        found
    }

    /// The tables agree with `ResolveExport` on many small graphs with
    /// cycles, named re-exports, and `default`.
    #[test]
    fn test_tables_agree_with_resolve_export() {
        const NAMES: [&str; 4] = ["a", "b", "c", "default"];
        let mut seed: u64 = 7;
        let mut random = |below: usize| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 33) as usize) % below
        };
        for _ in 0..3000 {
            let count = 2 + random(5);
            let mut files = Vec::new();
            for position in 0..count {
                let mut own = Vec::new();
                let mut named = Vec::new();
                for (offset, name) in NAMES.iter().enumerate() {
                    match random(6) {
                        0 => own.push((*name, position * 10 + offset)),
                        1 => named.push((*name, random(count), NAMES[random(NAMES.len())])),
                        _ => {}
                    }
                }
                let wildcards = (0..count).filter(|_| random(3) == 0).collect::<Vec<_>>();
                files.push(file(&own, &wildcards, &named));
            }
            let graph = Graph(files);
            let tables = Tables::build(&graph);
            for position in 0..count {
                for name in NAMES {
                    let expected = resolve_export(&graph, position, name, &mut Vec::new());
                    let found = match names(&tables, position, name).as_slice() {
                        [] => Resolution::Missing,
                        [only] => Resolution::Found(*only),
                        _ => Resolution::Ambiguous,
                    };
                    assert_eq!(
                        found,
                        expected,
                        "file {position}, name {name}, graph {:?}",
                        graph
                            .0
                            .iter()
                            .map(|member| (&member.own, &member.wildcards, &member.named))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }

    #[test]
    fn test_a_file_above_the_cap_keeps_its_own_names() {
        let graph = Graph(vec![
            file(&[("a", 1)], &[], &[]),
            file(&[("b", 2)], &[], &[]),
            file(&[("own", 3)], &[0, 1], &[("n", 0, "a")]),
            file(&[], &[2], &[]),
            file(&[], &[0], &[]),
        ]);
        let tables = Tables::build_with_cap(&graph, 0);
        assert_eq!(tables.not_resolved(), [2, 3]);
        assert_eq!(names(&tables, 2, "own"), [3]);
        assert_eq!(names(&tables, 2, "n"), [1]);
        assert_eq!(names(&tables, 2, "a"), Vec::<usize>::new());
        assert_eq!(names(&tables, 3, "own"), Vec::<usize>::new());
        assert_eq!(names(&tables, 4, "a"), [1]);
        let tables = Tables::build(&graph);
        assert!(tables.not_resolved().is_empty());
        assert_eq!(names(&tables, 3, "b"), [2]);
    }

    #[test]
    fn test_a_cycle_above_the_cap_keeps_its_own_names() {
        let graph = Graph(vec![
            file(&[("a", 1)], &[], &[]),
            file(&[("b", 2)], &[], &[]),
            file(&[("own", 3)], &[0, 1, 3], &[]),
            file(&[], &[2], &[("n", 0, "a")]),
        ]);
        let tables = Tables::build_with_cap(&graph, 0);
        assert_eq!(tables.not_resolved(), [2, 3]);
        assert_eq!(names(&tables, 2, "own"), [3]);
        assert_eq!(names(&tables, 3, "n"), [1]);
        assert_eq!(names(&tables, 3, "own"), Vec::<usize>::new());
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;
    use crate::index::timing::{
        assert_scales_linearly, assert_scales_linearly_above, on_one_thread, Stopwatch,
    };

    struct Graph(Vec<Member<'static, usize>>);

    impl Exports<'static, usize> for Graph {
        fn files(&self) -> usize {
            self.0.len()
        }

        fn member(&self, file: usize) -> Member<'static, usize> {
            self.0[file].clone()
        }
    }

    /// The names of the cases. A table holds `&str`, thus the names live as
    /// long as the test.
    fn name(position: usize) -> &'static str {
        static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        NAMES.get_or_init(|| {
            (0..1 << 20)
                .map(|position| format!("n{position}"))
                .collect()
        })[position]
            .as_str()
    }

    fn file(own: Option<usize>, wildcards: Vec<usize>) -> Member<'static, usize> {
        Member {
            own: own.map(|id| (name(id), vec![id])).into_iter().collect(),
            wildcards,
            named: Vec::new(),
            hides_default: true,
        }
    }

    /// The facts of one build: the time, the work, the writes, and the files
    /// that keep only their own names.
    struct Run {
        elapsed: std::time::Duration,
        work: usize,
        writes: usize,
        not_resolved: usize,
    }

    fn run(graph: &Graph, check: impl Fn(&Tables<'static, usize>)) -> Run {
        let started = Stopwatch::start();
        let tables = Tables::build(graph);
        let elapsed = started.elapsed();
        check(&tables);
        Run {
            elapsed,
            work: tables.work,
            writes: tables.writes,
            not_resolved: tables.not_resolved().len(),
        }
    }

    /// Each file defines one name and re-exports the file before it.
    fn chain(size: usize) -> Run {
        let graph = Graph(
            (0..size)
                .map(|position| {
                    file(
                        Some(position),
                        position.checked_sub(1).into_iter().collect(),
                    )
                })
                .collect(),
        );
        run(&graph, |tables| {
            assert_eq!(
                tables.get(size - 1, name(0)).map(|values| values[0]),
                Some(0)
            );
        })
    }

    /// One file re-exports `size` files that define one name each.
    fn wide(size: usize) -> Run {
        let mut files: Vec<_> = (0..size)
            .map(|position| file(Some(position), Vec::new()))
            .collect();
        files.push(file(None, (0..size).collect()));
        run(&Graph(files), |tables| {
            assert_eq!(
                tables.get(size, name(size - 1)).map(|values| values[0]),
                Some(size - 1)
            );
        })
    }

    /// A binary tree of barrels over `size` files that define one name each.
    fn tree(size: usize) -> Run {
        let mut files: Vec<_> = (0..size)
            .map(|position| file(Some(position), Vec::new()))
            .collect();
        let mut level: Vec<usize> = (0..size).collect();
        while level.len() > 1 {
            let mut next = Vec::new();
            for pair in level.chunks(2) {
                next.push(files.len());
                files.push(file(None, pair.to_vec()));
            }
            level = next;
        }
        let root = files.len() - 1;
        run(&Graph(files), |tables| {
            assert_eq!(
                tables.get(root, name(size - 1)).map(|values| values[0]),
                Some(size - 1)
            );
        })
    }

    /// `layers` layers of eight files. Each file defines one name and
    /// re-exports each file of the layer below it. Each table thus has the
    /// names of each layer below, and an exact build reads `O(layers^2)`
    /// entries.
    fn overlapping(layers: usize) -> Run {
        const WIDTH: usize = 8;
        let mut files = Vec::new();
        for layer in 0..layers {
            for _ in 0..WIDTH {
                let below = layer
                    .checked_sub(1)
                    .map(|below| (below * WIDTH..(below + 1) * WIDTH).collect())
                    .unwrap_or_default();
                files.push(file(Some(files.len()), below));
            }
        }
        run(&Graph(files), |tables| {
            assert_eq!(tables.get(WIDTH, name(0)).map(|values| values[0]), Some(0));
        })
    }

    /// The cases resolve at a small size, and the dense case does not stop.
    #[test]
    fn test_timing_cases_build() {
        assert_eq!(chain(3).not_resolved, 0);
        assert_eq!(wide(3).not_resolved, 0);
        assert_eq!(tree(5).not_resolved, 0);
        assert_eq!(overlapping(3).not_resolved, 0);
    }

    /// The writes and the work at `2 * size` are at most `2.5` times those at `size`.
    fn assert_counts_scale(label: &str, size: usize, case: impl Fn(usize) -> Run + Sync) {
        let small = on_one_thread(|| case(size));
        let large = on_one_thread(|| case(2 * size));
        for (what, small, large) in [
            ("writes", small.writes, large.writes),
            ("work", small.work, large.work),
        ] {
            assert!(
                large * 10 <= small * 25,
                "{label}: {what} {small} at {size}, {large} at {}",
                2 * size
            );
        }
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_tables_of_a_chain_build_in_linear_time() {
        assert_scales_linearly("chain", 200_000, |size| chain(size).elapsed);
        assert_counts_scale("chain", 200_000, chain);
        assert_eq!(chain(200_000).work, 0);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_tables_of_a_wide_barrel_build_in_linear_time() {
        assert_scales_linearly("wide barrel", 200_000, |size| wide(size).elapsed);
        assert_counts_scale("wide barrel", 200_000, wide);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_tables_of_a_tree_build_in_n_log_n_time() {
        // Each name is in the table of each barrel above it, thus the tables
        // hold `size * log2(size)` entries. From `2^17` to `2^18` the work
        // grows 2.1 times and the memory grows to 800 MB; the time grew 3
        // times on CI. At this size the memory is small. The floor is low so
        // that the check of the time still fails for a quadratic build.
        let size = 1 << 15;
        let floor = std::time::Duration::from_millis(40);
        assert_scales_linearly_above("tree", size, floor, |size| tree(size).elapsed);
        assert_counts_scale("tree", size, tree);
        assert_eq!(tree(2 * size).not_resolved, 0);
    }

    /// The cap stops the dense case: its work grows as `n log n`, and some
    /// files keep only their own names.
    #[test]
    #[ignore = "timing: run in release"]
    fn timing_tables_of_overlapping_barrels_stay_under_the_cap() {
        assert_scales_linearly("overlapping barrels", 4_000, |layers| {
            overlapping(layers).elapsed
        });
        assert_counts_scale("overlapping barrels", 4_000, overlapping);
        assert!(overlapping(4_000).not_resolved > 0);
    }
}
