//! The tables of the names that the files export, for the import resolution
//! and for the call resolution.
//!
//! The table of a file gives each name that the file exports: the own
//! definitions of the file, else the union of what its re-exports give.
//! A query of a barrel thus costs one lookup, not one search through each
//! re-export of the barrel.
//!
//! The tables are persistent hash tries. The table of a file starts from the
//! largest table of the files that it re-exports with a wildcard, by
//! reference, and inserts the entries of the other tables into it. A chain
//! of barrels thus costs `O(log n)` new nodes for each barrel, not a copy of
//! each name below it.
//!
//! The files of one cycle of re-exports share one table: the union of the
//! own definitions of the files of the cycle and of what the cycle
//! re-exports. The own definitions of a file come before that union.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use parking_lot::RwLock;

/// The bits of the hash for each level of the trie.
const BITS: u32 = 5;

/// The definitions of one name, sorted and without duplicates.
pub(crate) type Values<T> = Arc<Vec<T>>;

/// A persistent map from a name to its definitions. A clone shares each node.
pub(crate) struct Table<'a, T> {
    root: Option<Arc<Node<'a, T>>>,
    len: usize,
}

impl<T> Clone for Table<'_, T> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            len: self.len,
        }
    }
}

impl<T> Default for Table<'_, T> {
    fn default() -> Self {
        Self { root: None, len: 0 }
    }
}

enum Node<'a, T> {
    /// The children of the slots that `bitmap` marks, in slot order.
    Branch {
        bitmap: u32,
        children: Vec<Arc<Node<'a, T>>>,
    },
    /// The entries whose names have the hash `hash`.
    Leaf {
        hash: u64,
        entries: Vec<(&'a str, Values<T>)>,
    },
}

#[cfg(test)]
thread_local! {
    /// The nodes that the current thread made. A test reads it as the memory of the tables.
    pub(crate) static NODES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn node<'a, T>(node: Node<'a, T>) -> Arc<Node<'a, T>> {
    #[cfg(test)]
    NODES.with(|nodes| nodes.set(nodes.get() + 1));
    Arc::new(node)
}

fn hash_of(name: &str) -> u64 {
    // `DefaultHasher::new` has fixed keys, thus the hash does not change between runs.
    let mut hasher = std::hash::DefaultHasher::new();
    name.hash(&mut hasher);
    hasher.finish()
}

fn slot(hash: u64, level: u32) -> u32 {
    ((hash >> (level * BITS).min(63)) & 31) as u32
}

impl<'a, T> Table<'a, T> {
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Values<T>> {
        let hash = hash_of(name);
        let mut current = self.root.as_deref()?;
        let mut level = 0;
        loop {
            match current {
                Node::Leaf {
                    hash: leaf_hash,
                    entries,
                } => {
                    return (*leaf_hash == hash)
                        .then(|| entries.iter().find(|(key, _)| *key == name))
                        .flatten()
                        .map(|(_, values)| values);
                }
                Node::Branch { bitmap, children } => {
                    let bit = 1u32 << slot(hash, level);
                    if bitmap & bit == 0 {
                        return None;
                    }
                    current = &children[(bitmap & (bit - 1)).count_ones() as usize];
                    level += 1;
                }
            }
        }
    }

    /// Gives `name` the definitions `values`, in place of its definitions.
    pub(crate) fn insert(&mut self, name: &'a str, values: Values<T>) {
        let (root, added) = insert_into(self.root.as_ref(), hash_of(name), name, values, 0);
        self.root = Some(root);
        self.len += usize::from(added);
    }

    /// Each entry of the table.
    pub(crate) fn entries(&self) -> Vec<(&'a str, &Values<T>)> {
        let mut found = Vec::with_capacity(self.len);
        let mut pending: Vec<&Node<'a, T>> = self.root.as_deref().into_iter().collect();
        while let Some(current) = pending.pop() {
            match current {
                Node::Leaf { entries, .. } => {
                    found.extend(entries.iter().map(|(name, values)| (*name, values)));
                }
                Node::Branch { children, .. } => pending.extend(children.iter().map(Arc::as_ref)),
            }
        }
        found
    }
}

/// The node that has the entry `name` in place of the node `current`, and
/// tells if the name is new.
fn insert_into<'a, T>(
    current: Option<&Arc<Node<'a, T>>>,
    hash: u64,
    name: &'a str,
    values: Values<T>,
    level: u32,
) -> (Arc<Node<'a, T>>, bool) {
    let Some(current) = current else {
        let leaf = Node::Leaf {
            hash,
            entries: vec![(name, values)],
        };
        return (node(leaf), true);
    };
    match current.as_ref() {
        Node::Leaf {
            hash: leaf_hash,
            entries,
        } if *leaf_hash == hash => {
            let mut entries = entries.clone();
            let added = match entries.iter_mut().find(|(key, _)| *key == name) {
                Some(entry) => {
                    entry.1 = values;
                    false
                }
                None => {
                    entries.push((name, values));
                    true
                }
            };
            (node(Node::Leaf { hash, entries }), added)
        }
        Node::Leaf {
            hash: leaf_hash, ..
        } => {
            // Two hashes differ in one slot of a level before the bits end.
            let branch = Node::Branch {
                bitmap: 1 << slot(*leaf_hash, level),
                children: vec![Arc::clone(current)],
            };
            insert_into(Some(&node(branch)), hash, name, values, level)
        }
        Node::Branch { bitmap, children } => {
            let bit = 1u32 << slot(hash, level);
            let position = (bitmap & (bit - 1)).count_ones() as usize;
            let mut children = children.clone();
            let added = if bitmap & bit == 0 {
                let (child, added) = insert_into(None, hash, name, values, level + 1);
                children.insert(position, child);
                added
            } else {
                let (child, added) =
                    insert_into(Some(&children[position]), hash, name, values, level + 1);
                children[position] = child;
                added
            };
            let branch = Node::Branch {
                bitmap: bitmap | bit,
                children,
            };
            (node(branch), added)
        }
    }
}

/// The facts of one file that its table needs.
pub(crate) struct Member<'a, T> {
    /// The names that the file defines itself, which come before each re-export.
    pub own: Vec<(&'a str, Vec<T>)>,
    /// The files that the file re-exports with a wildcard.
    pub wildcards: Vec<usize>,
    /// The re-exports of one name: the local name, the file, and the name in that file.
    pub named: Vec<(&'a str, usize, &'a str)>,
}

/// The rules of one kind of table.
pub(crate) trait Exports<'a, T> {
    /// The files that the file re-exports, sorted and without duplicates.
    fn targets(&self, file: usize) -> Vec<usize>;

    fn member(&self, file: usize) -> Member<'a, T>;

    /// The part of the definitions of a name of one file that a file which
    /// re-exports the name gets.
    fn contribution<'v>(&self, values: &'v [T]) -> &'v [T];
}

pub(crate) type Tables<'a, T> = HashMap<usize, Table<'a, T>>;

/// The table of `start`. The tables of the files that it reaches are in
/// `memo` after the call. The search is the algorithm of Tarjan for strongly
/// connected components, without recursion, because a chain of barrels can
/// be long. A component gets its table when the search leaves it, thus each
/// file that it re-exports outside of it has a table then.
pub(crate) fn table<'a, T, E>(
    exports: &E,
    memo: &RwLock<Tables<'a, T>>,
    start: usize,
) -> Table<'a, T>
where
    T: Clone + Ord,
    E: Exports<'a, T>,
{
    if let Some(found) = memo.read().get(&start) {
        return found.clone();
    }
    struct Frame {
        file: usize,
        targets: Vec<usize>,
        next: usize,
    }
    let mut built: Tables<'a, T> = HashMap::new();
    let mut indices: HashMap<usize, usize> = HashMap::new();
    let mut lowest: HashMap<usize, usize> = HashMap::new();
    let mut on_stack: HashSet<usize> = HashSet::new();
    let mut component: Vec<usize> = Vec::new();
    let mut calls: Vec<Frame> = Vec::new();
    let is_known = |file: usize, built: &Tables<'a, T>| {
        built.contains_key(&file) || memo.read().contains_key(&file)
    };
    let enter = |file: usize,
                 calls: &mut Vec<Frame>,
                 component: &mut Vec<usize>,
                 indices: &mut HashMap<usize, usize>,
                 lowest: &mut HashMap<usize, usize>,
                 on_stack: &mut HashSet<usize>| {
        let index = indices.len();
        indices.insert(file, index);
        lowest.insert(file, index);
        on_stack.insert(file);
        component.push(file);
        calls.push(Frame {
            file,
            targets: exports.targets(file),
            next: 0,
        });
    };
    enter(
        start,
        &mut calls,
        &mut component,
        &mut indices,
        &mut lowest,
        &mut on_stack,
    );
    while let Some(frame) = calls.last_mut() {
        if frame.next < frame.targets.len() {
            let target = frame.targets[frame.next];
            frame.next += 1;
            let file = frame.file;
            if is_known(target, &built) {
                continue;
            }
            match indices.get(&target) {
                None => enter(
                    target,
                    &mut calls,
                    &mut component,
                    &mut indices,
                    &mut lowest,
                    &mut on_stack,
                ),
                Some(index) if on_stack.contains(&target) => {
                    let low = lowest[&file].min(*index);
                    lowest.insert(file, low);
                }
                Some(_) => {}
            }
            continue;
        }
        let frame = calls.pop().expect("a frame");
        let file = frame.file;
        if let Some(parent) = calls.last() {
            let low = lowest[&parent.file].min(lowest[&file]);
            lowest.insert(parent.file, low);
        }
        if lowest[&file] == indices[&file] {
            let split = component
                .iter()
                .rposition(|member| *member == file)
                .expect("the file is on the stack");
            let mut members = component.split_off(split);
            for member in &members {
                on_stack.remove(member);
            }
            members.sort_unstable();
            let tables = build_component(exports, &members, |target| {
                built
                    .get(&target)
                    .cloned()
                    .or_else(|| memo.read().get(&target).cloned())
                    .unwrap_or_default()
            });
            built.extend(members.into_iter().zip(tables));
        }
    }
    let found = built.get(&start).cloned().unwrap_or_default();
    memo.write().extend(built);
    found
}

/// The tables of the files of one component, in the order of `members`.
/// `table_of` gives the table of a file outside the component.
fn build_component<'a, T, E>(
    exports: &E,
    members: &[usize],
    table_of: impl Fn(usize) -> Table<'a, T>,
) -> Vec<Table<'a, T>>
where
    T: Clone + Ord,
    E: Exports<'a, T>,
{
    let inside: HashSet<usize> = members.iter().copied().collect();
    let facts: Vec<Member<'a, T>> = members.iter().map(|file| exports.member(*file)).collect();
    let own: HashMap<usize, HashMap<&'a str, &Vec<T>>> = members
        .iter()
        .zip(&facts)
        .map(|(file, member)| {
            let names = member
                .own
                .iter()
                .map(|(name, values)| (*name, values))
                .collect();
            (*file, names)
        })
        .collect();
    let cycle = members.len() > 1
        || facts.iter().any(|member| {
            member
                .wildcards
                .iter()
                .any(|target| inside.contains(target))
                || member
                    .named
                    .iter()
                    .any(|(_, target, _)| inside.contains(target))
        });
    let mut outside: Vec<usize> = facts
        .iter()
        .flat_map(|member| member.wildcards.iter().copied())
        .filter(|target| !inside.contains(target))
        .collect();
    outside.sort_unstable();
    outside.dedup();
    let outside: Vec<(usize, Table<'a, T>)> = outside
        .into_iter()
        .map(|target| (target, table_of(target)))
        .collect();
    // The largest table is the base, by reference. The other entries go into it.
    let base = outside
        .iter()
        .enumerate()
        .max_by_key(|(position, (_, table))| (table.len(), std::cmp::Reverse(*position)))
        .map(|(position, _)| position);
    let mut added: HashMap<&'a str, Vec<T>> = HashMap::new();
    for (position, (_, table)) in outside.iter().enumerate() {
        if Some(position) == base {
            continue;
        }
        for (name, values) in table.entries() {
            let given = exports.contribution(values);
            if !given.is_empty() {
                added.entry(name).or_default().extend_from_slice(given);
            }
        }
    }
    for member in &facts {
        for (local, target, source) in &member.named {
            let given: Vec<T> = if inside.contains(target) {
                own[target]
                    .get(source)
                    .map(|values| exports.contribution(values).to_vec())
                    .unwrap_or_default()
            } else {
                table_of(*target)
                    .get(source)
                    .map(|values| exports.contribution(values).to_vec())
                    .unwrap_or_default()
            };
            if !given.is_empty() {
                added.entry(local).or_default().extend(given);
            }
        }
    }
    if cycle {
        for member in &facts {
            for (name, values) in &member.own {
                let given = exports.contribution(values);
                if !given.is_empty() {
                    added.entry(name).or_default().extend_from_slice(given);
                }
            }
        }
    }
    let mut shared = base.map_or_else(Table::default, |position| outside[position].1.clone());
    for (name, mut values) in added {
        let existing = shared.get(name).cloned();
        if let Some(existing) = &existing {
            values.extend_from_slice(exports.contribution(existing));
        }
        values.sort_unstable();
        values.dedup();
        if existing.as_deref() != Some(&values) {
            shared.insert(name, Arc::new(values));
        }
    }
    facts
        .into_iter()
        .map(|member| {
            let mut table = shared.clone();
            for (name, mut values) in member.own {
                values.sort_unstable();
                values.dedup();
                table.insert(name, Arc::new(values));
            }
            table
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_get_and_share() {
        let mut first: Table<'_, usize> = Table::default();
        let names: Vec<String> = (0..2000).map(|position| format!("n{position}")).collect();
        for (position, name) in names.iter().enumerate() {
            first.insert(name, Arc::new(vec![position]));
        }
        let mut second = first.clone();
        second.insert("n7", Arc::new(vec![9]));
        second.insert("extra", Arc::new(vec![1]));
        assert_eq!(first.len(), 2000);
        assert_eq!(second.len(), 2001);
        assert_eq!(first.get("n7").map(|values| values.to_vec()), Some(vec![7]));
        assert_eq!(
            second.get("n7").map(|values| values.to_vec()),
            Some(vec![9])
        );
        assert_eq!(first.get("extra"), None);
        assert_eq!(first.entries().len(), 2000);
        for (position, name) in names.iter().enumerate() {
            assert_eq!(first.get(name).map(|values| values[0]), Some(position));
        }
    }
}
