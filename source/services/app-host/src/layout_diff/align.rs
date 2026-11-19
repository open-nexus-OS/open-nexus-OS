// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the layout diff's alignment (TASK-0354 P8) — an ORDER-PRESERVING matching of two
//! lists: the common prefix and suffix (`trim`), then a greedy resync through what lies between
//! (`walk`): on a mismatch the nearest equal pair ahead on either side, within `LOOKAHEAD` items
//! and under `ALIGN_BUDGET` comparisons for the whole walk. What no match holds comes out as
//! gaps, a run of the old list beside a run of the new.
//!
//! The matching is monotone: two matched pairs never cross. That is all the diff's soundness
//! asks of it — any pixel outside every unmatched item's rectangle is then covered by the same
//! equal items in the same order. So every bound here may cost damage, never correctness: no
//! equal pair within the lookahead leaves the block unmatched; a spent budget leaves the rest of
//! the window unmatched (`spent`: the caller names every item of it). No table, no allocation
//! (an LCS would be O(n·m) of both for what the greedy walk proves equally).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below; the diffs built on it in `layout_diff/tests.rs` and
//!   `layout_diff/rect_tests.rs`
//! INVARIANTS: matched pairs are equal by `eq` and never cross; every item is in exactly one
//!   match or one gap; over a bound more is unmatched, never less

/// How far ahead, on either side, a mismatch looks for the next equal pair.
pub(super) const LOOKAHEAD: usize = 64;
/// Comparisons one walk spends looking ahead; past it the rest of the window is unmatched.
pub(super) const ALIGN_BUDGET: usize = 8192;

/// Whether two items are equal — `None`: out of shape, the diff gives no bound.
pub(super) type Eq<'a, T> = &'a mut dyn FnMut(&T, &T) -> Option<bool>;
/// A run of the old list beside a run of the new that no match holds; `spent`: left over by a
/// walk past its budget (nothing in it was compared). `None`: out of shape.
pub(super) type Gap<'a, T> = &'a mut dyn FnMut(&[T], &[T], bool) -> Option<()>;

/// The common prefix and suffix of `old` and `new`, as counts.
pub(super) fn trim<T>(old: &[T], new: &[T], eq: Eq<'_, T>) -> Option<(usize, usize)> {
    let common = old.len().min(new.len());
    let mut prefix = 0;
    while prefix < common && eq(&old[prefix], &new[prefix])? {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < common - prefix
        && eq(&old[old.len() - 1 - suffix], &new[new.len() - 1 - suffix])?
    {
        suffix += 1;
    }
    Some((prefix, suffix))
}

/// Where a mismatch finds its next equal pair.
enum Resync {
    /// `(old ahead, new ahead)`: the nearest equal pair.
    At(usize, usize),
    /// None within the lookahead.
    Far,
    /// The budget ran out while looking.
    Spent,
}

/// The nearest equal pair ahead of a mismatch at the heads of `old` and `new` (both non-empty):
/// the least `old ahead + new ahead`, each within [`LOOKAHEAD`].
fn nearest<T>(old: &[T], new: &[T], eq: Eq<'_, T>, budget: &mut usize) -> Option<Resync> {
    let (lo, ln) = (old.len().min(LOOKAHEAD + 1), new.len().min(LOOKAHEAD + 1));
    for d in 1..lo + ln - 1 {
        for di in d.saturating_sub(ln - 1)..=d.min(lo - 1) {
            if *budget == 0 {
                return Some(Resync::Spent);
            }
            *budget -= 1;
            if eq(&old[di], &new[d - di])? {
                return Some(Resync::At(di, d - di));
            }
        }
    }
    Some(Resync::Far)
}

/// Walks `old` and `new` in step, matching equal items in order, and hands every run no match
/// holds to `gap`. `None`: `eq` or `gap` found the lists out of shape.
pub(super) fn walk<T>(old: &[T], new: &[T], eq: Eq<'_, T>, gap: Gap<'_, T>) -> Option<()> {
    let (mut i, mut j) = (0, 0);
    // Where the open gap began.
    let (mut gi, mut gj) = (0, 0);
    let mut budget = ALIGN_BUDGET;
    let mut spent = false;
    while i < old.len() && j < new.len() {
        if eq(&old[i], &new[j])? {
            if (gi, gj) != (i, j) {
                gap(&old[gi..i], &new[gj..j], false)?;
            }
            (i, j) = (i + 1, j + 1);
            (gi, gj) = (i, j);
            continue;
        }
        match nearest(&old[i..], &new[j..], eq, &mut budget)? {
            // The pair itself is matched by the next step.
            Resync::At(di, dj) => (i, j) = (i + di, j + dj),
            // No equal pair in the block: all of it is unmatched.
            Resync::Far => {
                (i, j) = ((i + LOOKAHEAD + 1).min(old.len()), (j + LOOKAHEAD + 1).min(new.len()));
            }
            Resync::Spent => {
                spent = true;
                break;
            }
        }
    }
    if gi < old.len() || gj < new.len() {
        gap(&old[gi..], &new[gj..], spent)?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gaps of a walk over two lists of numbers: `(old run, new run, spent)`.
    fn gaps(old: &[u32], new: &[u32]) -> Vec<(Vec<u32>, Vec<u32>, bool)> {
        let mut out = Vec::new();
        let mut eq = |a: &u32, b: &u32| Some(a == b);
        let mut gap = |o: &[u32], n: &[u32], spent: bool| {
            out.push((o.to_vec(), n.to_vec(), spent));
            Some(())
        };
        walk(old, new, &mut eq, &mut gap).expect("in shape");
        out
    }

    #[test]
    fn equal_lists_leave_no_gap() {
        assert!(gaps(&[1, 2, 3], &[1, 2, 3]).is_empty());
        assert!(gaps(&[], &[]).is_empty());
    }

    #[test]
    fn a_change_an_insert_and_a_removal_are_three_gaps() {
        let old = [1, 2, 3, 4, 5, 6, 7, 8];
        let new = [1, 20, 3, 4, 40, 41, 5, 6, 8];
        assert_eq!(
            gaps(&old, &new),
            vec![
                (vec![2], vec![20], false),
                (vec![], vec![40, 41], false),
                (vec![7], vec![], false),
            ]
        );
    }

    #[test]
    fn the_rest_of_either_list_is_a_gap() {
        assert_eq!(gaps(&[1, 2], &[1, 2, 3, 4]), vec![(vec![], vec![3, 4], false)]);
        assert_eq!(gaps(&[1, 2, 3], &[1]), vec![(vec![2, 3], vec![], false)]);
        assert_eq!(gaps(&[], &[9]), vec![(vec![], vec![9], false)]);
    }

    #[test]
    fn trim_counts_the_common_ends_without_overlapping_them() {
        let mut eq = |a: &u32, b: &u32| Some(a == b);
        assert_eq!(trim(&[1, 2, 9, 4, 5], &[1, 2, 7, 8, 4, 5], &mut eq), Some((2, 2)));
        // The shorter list's items are counted once: prefix first.
        assert_eq!(trim(&[1, 1], &[1, 1, 1], &mut eq), Some((2, 0)));
        let mut odd = |_: &u32, _: &u32| None;
        assert_eq!(trim(&[1], &[1], &mut odd), None, "out of shape");
    }

    /// A pair past the lookahead is not found: its block is unmatched on both sides, and the
    /// walk goes on behind it.
    #[test]
    fn test_reject_a_match_past_the_lookahead() {
        let far = LOOKAHEAD as u32 + 5;
        let old: Vec<u32> = (0..far).map(|k| 1000 + k).chain([7, 8]).collect();
        let new: Vec<u32> = vec![7, 8];
        let found = gaps(&old, &new);
        assert_eq!(found.len(), 1, "one gap holds both lists: {found:?}");
        assert_eq!((found[0].0.len(), found[0].1.len()), (old.len(), 2));
        assert!(!found[0].2, "the budget was not the reason");
    }

    /// Past the budget nothing is compared any more: the rest is one gap marked `spent`, the
    /// equal items in it included.
    #[test]
    fn test_reject_matching_past_the_budget() {
        let block = LOOKAHEAD as u32 + 1;
        // Three blocks without an equal pair cost more than the budget; then equal items.
        let old: Vec<u32> = (0..3 * block).map(|k| 10_000 + k).chain(1..=4).collect();
        let new: Vec<u32> = (0..3 * block).map(|k| 20_000 + k).chain(1..=4).chain([99]).collect();
        let found = gaps(&old, &new);
        let (o, n, spent) = found.last().expect("a gap");
        assert!(*spent, "the walk ran out of comparisons");
        assert!(o.ends_with(&[1, 2, 3, 4]) && n.ends_with(&[1, 2, 3, 4, 99]));
        let named: usize = found.iter().map(|g| g.0.len()).sum();
        assert_eq!(named, old.len(), "every old item is in a gap: nothing was matched");
    }
}
