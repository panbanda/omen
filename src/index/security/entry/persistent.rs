//! A persistent ordered map. A change copies only the path to the changed key, so a copy of the map
//! takes a constant time, and a change takes a time in proportion to the logarithm of the size.
//! The priority of a node comes from a hash of its key, so the shape of the tree is the same in each
//! run.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

type Link<K, V> = Option<Arc<Node<K, V>>>;

struct Node<K, V> {
    key: K,
    value: V,
    priority: u64,
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

fn priority_of<K: Hash>(key: &K) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

/// Splits the tree in the keys before `key`, and the other keys. With `inclusive`, the first part
/// also has `key`.
fn split<K: Ord + Clone, V: Clone>(
    node: &Link<K, V>,
    key: &K,
    inclusive: bool,
) -> (Link<K, V>, Link<K, V>) {
    let Some(node) = node else {
        return (None, None);
    };
    let goes_left = if inclusive {
        node.key <= *key
    } else {
        node.key < *key
    };
    if goes_left {
        let (middle, right) = split(&node.right, key, inclusive);
        let rebuilt = Arc::new(Node {
            key: node.key.clone(),
            value: node.value.clone(),
            priority: node.priority,
            left: node.left.clone(),
            right: middle,
        });
        (Some(rebuilt), right)
    } else {
        let (left, middle) = split(&node.left, key, inclusive);
        let rebuilt = Arc::new(Node {
            key: node.key.clone(),
            value: node.value.clone(),
            priority: node.priority,
            left: middle,
            right: node.right.clone(),
        });
        (left, Some(rebuilt))
    }
}

/// Joins two trees where each key of `left` is before each key of `right`.
fn merge<K: Clone, V: Clone>(left: Link<K, V>, right: Link<K, V>) -> Link<K, V> {
    match (left, right) {
        (None, other) | (other, None) => other,
        (Some(left), Some(right)) => {
            if left.priority >= right.priority {
                Some(Arc::new(Node {
                    key: left.key.clone(),
                    value: left.value.clone(),
                    priority: left.priority,
                    left: left.left.clone(),
                    right: merge(left.right.clone(), Some(right)),
                }))
            } else {
                Some(Arc::new(Node {
                    key: right.key.clone(),
                    value: right.value.clone(),
                    priority: right.priority,
                    left: merge(Some(left), right.left.clone()),
                    right: right.right.clone(),
                }))
            }
        }
    }
}

impl<K: Ord + Hash + Clone, V: Clone> PersistentMap<K, V> {
    pub(super) fn get(&self, key: &K) -> Option<&V> {
        let mut node = self.root.as_deref();
        while let Some(current) = node {
            node = match key.cmp(&current.key) {
                std::cmp::Ordering::Less => current.left.as_deref(),
                std::cmp::Ordering::Greater => current.right.as_deref(),
                std::cmp::Ordering::Equal => return Some(&current.value),
            };
        }
        None
    }

    pub(super) fn remove(&mut self, key: &K) {
        let (left, rest) = split(&self.root, key, false);
        let (_, right) = split(&rest, key, true);
        self.root = merge(left, right);
    }

    pub(super) fn insert(&mut self, key: K, value: V) {
        self.remove(&key);
        let (left, right) = split(&self.root, &key, false);
        let node = Some(Arc::new(Node {
            priority: priority_of(&key),
            key,
            value,
            left: None,
            right: None,
        }));
        self.root = merge(merge(left, node), right);
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
}
