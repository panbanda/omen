//! A persistent ordered map: an AVL tree that copies the path to a changed key. A copy of the map
//! takes a constant time. The height of the tree is at most 1.45 times the logarithm of the number
//! of keys for any keys in any order, so each change takes a time in proportion to the logarithm of
//! the size, and the recursion of a change is not deeper than the height.

use std::cmp::Ordering;
use std::sync::Arc;

type Link<K, V> = Option<Arc<Node<K, V>>>;

struct Node<K, V> {
    key: K,
    value: V,
    height: u32,
    left: Link<K, V>,
    right: Link<K, V>,
}

pub(super) struct PersistentMap<K, V> {
    root: Link<K, V>,
}

impl<K, V> Clone for PersistentMap<K, V> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
        }
    }
}

impl<K, V> Default for PersistentMap<K, V> {
    fn default() -> Self {
        Self { root: None }
    }
}

fn height<K, V>(link: &Link<K, V>) -> u32 {
    link.as_ref().map_or(0, |node| node.height)
}

fn make<K, V>(key: K, value: V, left: Link<K, V>, right: Link<K, V>) -> Arc<Node<K, V>> {
    Arc::new(Node {
        height: 1 + height(&left).max(height(&right)),
        key,
        value,
        left,
        right,
    })
}

/// Builds a node from two subtrees whose heights differ by at most 2, and rotates when they differ
/// by 2.
fn balance<K: Clone, V: Clone>(
    key: K,
    value: V,
    left: Link<K, V>,
    right: Link<K, V>,
) -> Arc<Node<K, V>> {
    let (left_height, right_height) = (height(&left), height(&right));
    if left_height > right_height + 1 {
        let heavy = left.as_ref().expect("a taller side has a node");
        if height(&heavy.left) >= height(&heavy.right) {
            let lower = make(key, value, heavy.right.clone(), right);
            return make(
                heavy.key.clone(),
                heavy.value.clone(),
                heavy.left.clone(),
                Some(lower),
            );
        }
        let middle = heavy.right.as_ref().expect("the inner side is taller");
        let new_left = make(
            heavy.key.clone(),
            heavy.value.clone(),
            heavy.left.clone(),
            middle.left.clone(),
        );
        let new_right = make(key, value, middle.right.clone(), right);
        return make(
            middle.key.clone(),
            middle.value.clone(),
            Some(new_left),
            Some(new_right),
        );
    }
    if right_height > left_height + 1 {
        let heavy = right.as_ref().expect("a taller side has a node");
        if height(&heavy.right) >= height(&heavy.left) {
            let lower = make(key, value, left, heavy.left.clone());
            return make(
                heavy.key.clone(),
                heavy.value.clone(),
                Some(lower),
                heavy.right.clone(),
            );
        }
        let middle = heavy.left.as_ref().expect("the inner side is taller");
        let new_left = make(key, value, left, middle.left.clone());
        let new_right = make(
            heavy.key.clone(),
            heavy.value.clone(),
            middle.right.clone(),
            heavy.right.clone(),
        );
        return make(
            middle.key.clone(),
            middle.value.clone(),
            Some(new_left),
            Some(new_right),
        );
    }
    make(key, value, left, right)
}

fn insert<K: Ord + Clone, V: Clone>(link: &Link<K, V>, key: K, value: V) -> Arc<Node<K, V>> {
    let Some(node) = link else {
        return make(key, value, None, None);
    };
    match key.cmp(&node.key) {
        Ordering::Less => balance(
            node.key.clone(),
            node.value.clone(),
            Some(insert(&node.left, key, value)),
            node.right.clone(),
        ),
        Ordering::Greater => balance(
            node.key.clone(),
            node.value.clone(),
            node.left.clone(),
            Some(insert(&node.right, key, value)),
        ),
        Ordering::Equal => make(key, value, node.left.clone(), node.right.clone()),
    }
}

/// The first key and value of a tree that is not empty.
fn first<K: Clone, V: Clone>(node: &Node<K, V>) -> (K, V) {
    let mut current = node;
    while let Some(left) = current.left.as_deref() {
        current = left;
    }
    (current.key.clone(), current.value.clone())
}

fn remove<K: Ord + Clone, V: Clone>(link: &Link<K, V>, key: &K) -> Link<K, V> {
    let node = link.as_ref()?;
    match key.cmp(&node.key) {
        Ordering::Less => Some(balance(
            node.key.clone(),
            node.value.clone(),
            remove(&node.left, key),
            node.right.clone(),
        )),
        Ordering::Greater => Some(balance(
            node.key.clone(),
            node.value.clone(),
            node.left.clone(),
            remove(&node.right, key),
        )),
        Ordering::Equal => match (&node.left, &node.right) {
            (None, _) => node.right.clone(),
            (_, None) => node.left.clone(),
            (Some(_), Some(right)) => {
                let (next_key, next_value) = first(right);
                let rest = remove(&node.right, &next_key);
                Some(balance(next_key, next_value, node.left.clone(), rest))
            }
        },
    }
}

impl<K: Ord + Clone, V: Clone> PersistentMap<K, V> {
    pub(super) fn get(&self, key: &K) -> Option<&V> {
        let mut node = self.root.as_deref();
        while let Some(current) = node {
            node = match key.cmp(&current.key) {
                Ordering::Less => current.left.as_deref(),
                Ordering::Greater => current.right.as_deref(),
                Ordering::Equal => return Some(&current.value),
            };
        }
        None
    }

    pub(super) fn remove(&mut self, key: &K) {
        if self.get(key).is_some() {
            self.root = remove(&self.root, key);
        }
    }

    pub(super) fn insert(&mut self, key: K, value: V) {
        self.root = Some(insert(&self.root, key, value));
    }

    /// The keys and the values in the order of the keys.
    pub(super) fn entries(&self) -> Vec<(K, V)> {
        self.walk(|node| (node.key.clone(), node.value.clone()))
    }

    /// The values in the order of the keys.
    pub(super) fn values(&self) -> Vec<V> {
        self.walk(|node| node.value.clone())
    }

    fn walk<T>(&self, pick: impl Fn(&Node<K, V>) -> T) -> Vec<T> {
        let mut out = Vec::new();
        let mut stack: Vec<&Node<K, V>> = Vec::new();
        let mut node = self.root.as_deref();
        loop {
            while let Some(current) = node {
                stack.push(current);
                node = current.left.as_deref();
            }
            let Some(current) = stack.pop() else {
                break;
            };
            out.push(pick(current));
            node = current.right.as_deref();
        }
        out
    }

    #[cfg(test)]
    fn height(&self) -> u32 {
        height(&self.root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: usize = 100_000;

    /// An AVL tree with n keys has a height of at most 1.4405 * log2(n + 2).
    fn limit(count: usize) -> u32 {
        (1.4405 * ((count + 2) as f64).log2()).ceil() as u32
    }

    #[test]
    fn sorted_keys_keep_the_height_logarithmic() {
        let started = std::time::Instant::now();
        let mut map: PersistentMap<String, usize> = PersistentMap::default();
        for number in 0..KEYS {
            map.insert(format!("{number:08}"), number);
        }
        assert!(map.height() <= limit(KEYS), "{}", map.height());
        let mut reversed: PersistentMap<usize, usize> = PersistentMap::default();
        for number in (0..KEYS).rev() {
            reversed.insert(number, number);
        }
        assert!(reversed.height() <= limit(KEYS), "{}", reversed.height());
        for number in (0..KEYS).filter(|number| number % 2 == 1) {
            map.remove(&format!("{number:08}"));
        }
        assert_eq!(map.values().len(), KEYS / 2);
        assert!(map.height() <= limit(KEYS / 2), "{}", map.height());
        assert!(started.elapsed().as_secs() < 30);
    }

    #[test]
    fn a_copy_does_not_change_when_the_original_changes() {
        let mut map: PersistentMap<i64, &str> = PersistentMap::default();
        map.insert(2, "b");
        map.insert(1, "a");
        let copy = map.clone();
        map.insert(3, "c");
        map.remove(&1);
        map.insert(2, "B");
        assert_eq!(copy.values(), ["a", "b"]);
        assert_eq!(map.values(), ["B", "c"]);
        assert_eq!(map.get(&1), None);
        assert_eq!(copy.get(&1), Some(&"a"));
    }

    #[test]
    fn a_mixed_sequence_matches_a_btree_map() {
        let mut map: PersistentMap<u32, u32> = PersistentMap::default();
        let mut model = std::collections::BTreeMap::new();
        let mut state = 12_345_u32;
        for _ in 0..20_000 {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let key = (state >> 16) % 500;
            if state.is_multiple_of(3) {
                map.remove(&key);
                model.remove(&key);
            } else {
                map.insert(key, state);
                model.insert(key, state);
            }
        }
        assert_eq!(
            map.entries(),
            model.into_iter().collect::<Vec<(u32, u32)>>()
        );
    }
}
