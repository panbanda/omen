//! The local names of one owner with one name, by the blocks in which they are visible.
//!
//! A deep function can bind one name many times, as each closure of a chain of closures
//! binds its parameter. A look at each binding for each use takes quadratic time. The blocks
//! of the bindings are the ranges of syntax nodes, so two blocks nest or do not overlap. Thus
//! the blocks that contain a byte form one chain, and the nearest of them is an ancestor of
//! the last block that starts at or before the byte. Binary lifting finds that ancestor in
//! logarithmic time, also when the chain is deep.

use std::ops::Range;

const NONE: u32 = u32::MAX;

/// One local name: the block in which it is visible, and the start of its first value.
pub(super) type Binding<'a> = (Option<&'a Range<usize>>, Option<usize>);

/// The entries of the index are the bindings with a block, by start, then by end from the
/// largest, then by rank. A rank is the position of a binding in the list of the owner and
/// the name, which is in source order.
pub(super) struct LocalBlocks {
    starts: Vec<usize>,
    ends: Vec<usize>,
    /// For each entry, the rank of the first binding with the same block.
    first_with_block: Vec<u32>,
    /// `up[level][entry]` is the ancestor `2^level` steps above the entry: the nearest
    /// entry that contains it, and so on.
    up: Vec<Vec<u32>>,
    /// For each entry, the smallest value key of the entry and its ancestors. See `value_key`.
    lowest_value_key: Vec<i64>,
    /// The rank of the first binding without a block.
    first_unscoped: Option<u32>,
    /// The smallest value key of the bindings without a block.
    lowest_unscoped_value_key: i64,
    /// The rank of the binding with the smallest block, the first one for equal sizes.
    smallest: Option<u32>,
}

/// A key that tells if a binding is in scope at a byte: a binding without a value is in
/// scope everywhere in its block, and a binding with a value is in scope after the start
/// of its value.
fn value_key(value_start: Option<usize>) -> i64 {
    value_start.map_or(-1, |start| start as i64)
}

impl LocalBlocks {
    /// The index of the bindings. None when two blocks overlap and do not nest, which the
    /// ranges of syntax nodes cannot do. The caller then looks at each binding.
    pub(super) fn new(bindings: &[Binding<'_>]) -> Option<Self> {
        let mut order: Vec<u32> = Vec::new();
        let mut first_unscoped = None;
        let mut lowest_unscoped_value_key = i64::MAX;
        let mut smallest: Option<(usize, u32)> = None;
        for (rank, (block, value_start)) in bindings.iter().enumerate() {
            let rank = u32::try_from(rank).ok()?;
            match block {
                Some(block) => {
                    order.push(rank);
                    if smallest.is_none_or(|(size, _)| block.len() < size) {
                        smallest = Some((block.len(), rank));
                    }
                }
                None => {
                    first_unscoped.get_or_insert(rank);
                    lowest_unscoped_value_key =
                        lowest_unscoped_value_key.min(value_key(*value_start));
                }
            }
        }
        let block_of = |rank: u32| bindings[rank as usize].0.expect("a block");
        order.sort_by_key(|rank| {
            let block = block_of(*rank);
            (block.start, std::cmp::Reverse(block.end), *rank)
        });
        let starts: Vec<usize> = order.iter().map(|rank| block_of(*rank).start).collect();
        let ends: Vec<usize> = order.iter().map(|rank| block_of(*rank).end).collect();
        let mut parents = vec![NONE; order.len()];
        let mut first_with_block = vec![0; order.len()];
        let mut lowest_value_key = vec![i64::MAX; order.len()];
        let mut open: Vec<usize> = Vec::new();
        for entry in 0..order.len() {
            while let Some(&top) = open.last() {
                if starts[top] <= starts[entry] && ends[entry] <= ends[top] {
                    break;
                }
                if ends[top] > starts[entry] {
                    return None;
                }
                open.pop();
            }
            let own_key = value_key(bindings[order[entry] as usize].1);
            match open.last() {
                Some(&parent) => {
                    parents[entry] = parent as u32;
                    lowest_value_key[entry] = own_key.min(lowest_value_key[parent]);
                    let same = starts[parent] == starts[entry] && ends[parent] == ends[entry];
                    first_with_block[entry] = if same {
                        first_with_block[parent]
                    } else {
                        order[entry]
                    };
                }
                None => {
                    lowest_value_key[entry] = own_key;
                    first_with_block[entry] = order[entry];
                }
            }
            open.push(entry);
        }
        let mut up = vec![parents];
        while up
            .last()
            .is_some_and(|level| level.iter().any(|entry| *entry != NONE))
        {
            let below = up.last().expect("a level");
            let next = below
                .iter()
                .map(|entry| match *entry {
                    NONE => NONE,
                    entry => below[entry as usize],
                })
                .collect();
            up.push(next);
        }
        Some(Self {
            starts,
            ends,
            first_with_block,
            up,
            lowest_value_key,
            first_unscoped,
            lowest_unscoped_value_key,
            smallest: smallest.map(|(_, rank)| rank),
        })
    }

    /// The entry of the nearest block that contains the byte.
    fn nearest_entry(&self, at: usize) -> Option<usize> {
        let after = self.starts.partition_point(|start| *start <= at);
        let mut entry = after.checked_sub(1)?;
        if self.ends[entry] > at {
            return Some(entry);
        }
        // The ends do not decrease from an entry to its ancestors. Thus the ancestors that end
        // at or before the byte are the nearest ones, and the answer is the parent of the
        // farthest of them.
        for level in self.up.iter().rev() {
            let above = level[entry];
            if above != NONE && self.ends[above as usize] <= at {
                entry = above as usize;
            }
        }
        match self.up[0][entry] {
            NONE => None,
            parent => Some(parent as usize),
        }
    }

    /// The rank of the binding that a use at the byte sees: the binding with the nearest
    /// block that contains the byte, then the first binding without a block. Without a byte,
    /// the binding with the smallest block. Of equal candidates, the first one.
    pub(super) fn nearest(&self, at: Option<usize>) -> Option<usize> {
        let rank = match at {
            None => self.smallest.or(self.first_unscoped),
            Some(at) => self
                .nearest_entry(at)
                .map(|entry| self.first_with_block[entry])
                .or(self.first_unscoped),
        };
        rank.map(|rank| rank as usize)
    }

    /// Tells if a binding is in scope at the byte: its block contains the byte, or it has
    /// no block, and it has no value or its value starts before the byte.
    pub(super) fn has_binding_in_scope(&self, at: usize) -> bool {
        let at_key = at as i64;
        self.lowest_unscoped_value_key < at_key
            || self
                .nearest_entry(at)
                .is_some_and(|entry| self.lowest_value_key[entry] < at_key)
    }

    #[cfg(test)]
    fn entries(&self) -> usize {
        self.starts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The search of `Resolver::local`, one binding at a time.
    fn nearest_by_scan(bindings: &[Binding<'_>], at: Option<usize>) -> Option<usize> {
        bindings
            .iter()
            .enumerate()
            .filter(|(_, (block, _))| {
                at.is_none_or(|at| block.is_none_or(|block| block.contains(&at)))
            })
            .min_by_key(|(_, (block, _))| block.map_or(usize::MAX, |block| block.len()))
            .map(|(rank, _)| rank)
    }

    /// The check of `Resolver::is_local_value`, one binding at a time.
    fn in_scope_by_scan(bindings: &[Binding<'_>], at: usize) -> bool {
        bindings.iter().any(|(block, value_start)| {
            value_start.is_none_or(|value| value < at)
                && block.is_none_or(|block| block.contains(&at))
        })
    }

    /// Blocks that nest or do not overlap, from a seed: each block is a node of a random tree
    /// of nodes over the bytes `0..width`, some of them empty, and some of them twice.
    fn nested_blocks(seed: u64, count: usize, width: usize) -> Vec<Range<usize>> {
        let mut state = seed;
        let mut next = |bound: usize| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 33) as usize) % bound.max(1)
        };
        let mut blocks: Vec<Range<usize>> = std::iter::once(0..width).collect();
        while blocks.len() < count {
            let parent = blocks[next(blocks.len())].clone();
            let block = match next(4) {
                0 => parent.clone(),
                1 => {
                    let at = parent.start + next(parent.len() + 1);
                    at..at
                }
                _ => {
                    let start = parent.start + next(parent.len() + 1);
                    let end = start + next(parent.end - start + 1);
                    start..end
                }
            };
            // A new block must nest with or keep apart from each other block.
            let fits = blocks.iter().all(|other| {
                block.end <= other.start
                    || other.end <= block.start
                    || (other.start <= block.start && block.end <= other.end)
                    || (block.start <= other.start && other.end <= block.end)
            });
            if fits {
                blocks.push(block);
            }
        }
        blocks
    }

    #[test]
    fn the_index_gives_the_binding_of_the_scan_for_each_byte() {
        for seed in 0..300 {
            let width = 40;
            let blocks = nested_blocks(seed, 1 + (seed as usize % 25), width);
            let mut state = seed;
            let mut next = |bound: usize| {
                state = state
                    .wrapping_mul(2_862_933_555_777_941_757)
                    .wrapping_add(3_037_000_493);
                ((state >> 33) as usize) % bound
            };
            let mut bindings: Vec<Binding<'_>> = Vec::new();
            for block in &blocks {
                let value = (next(3) > 0).then(|| next(width + 2));
                bindings.push((Some(block), value));
                if next(5) == 0 {
                    let value = (next(2) > 0).then(|| next(width + 2));
                    bindings.push((None, value));
                }
            }
            // The order of the bindings is the order of the source, not of the blocks.
            for index in (1..bindings.len()).rev() {
                bindings.swap(index, next(index + 1));
            }
            let index = LocalBlocks::new(&bindings).expect("nested blocks");
            assert_eq!(
                index.entries(),
                bindings.iter().filter(|(block, _)| block.is_some()).count()
            );
            assert_eq!(
                index.nearest(None),
                nearest_by_scan(&bindings, None),
                "seed {seed}"
            );
            for at in 0..=width + 1 {
                assert_eq!(
                    index.nearest(Some(at)),
                    nearest_by_scan(&bindings, Some(at)),
                    "seed {seed} at {at}"
                );
                assert_eq!(
                    index.has_binding_in_scope(at),
                    in_scope_by_scan(&bindings, at),
                    "seed {seed} at {at}"
                );
            }
        }
    }

    #[test]
    fn a_deep_chain_of_blocks_gives_the_nearest_block_after_the_inner_ones() {
        // Each block `i..2 * depth - i` holds the next one, and a use after the inner blocks
        // is at the byte `2 * depth - i - 1`.
        let depth = 1_000;
        let blocks: Vec<Range<usize>> = (0..depth).map(|level| level..2 * depth - level).collect();
        let bindings: Vec<Binding<'_>> = blocks.iter().map(|block| (Some(block), None)).collect();
        let index = LocalBlocks::new(&bindings).expect("nested blocks");
        for level in 0..depth {
            let at = 2 * depth - level - 1;
            assert_eq!(index.nearest(Some(at)), Some(level));
        }
        assert_eq!(index.nearest(Some(2 * depth)), None);
        assert_eq!(index.nearest(None), Some(depth - 1));
    }

    #[test]
    fn blocks_that_overlap_without_nesting_give_no_index() {
        let first = 0..10;
        let second = 5..15;
        assert!(LocalBlocks::new(&[(Some(&first), None), (Some(&second), None)]).is_none());
    }

    #[test]
    fn without_blocks_the_first_binding_is_in_scope_where_its_value_allows() {
        let bindings: [Binding<'_>; 2] = [(None, Some(10)), (None, Some(4))];
        let index = LocalBlocks::new(&bindings).expect("no blocks");
        assert_eq!(index.nearest(Some(2)), Some(0));
        assert_eq!(index.nearest(None), Some(0));
        assert!(!index.has_binding_in_scope(4));
        assert!(index.has_binding_in_scope(5));
    }
}
