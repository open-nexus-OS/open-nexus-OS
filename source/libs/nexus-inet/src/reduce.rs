// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Interaction rules + round-based reduction. `reduce_chunk` is the
//! ONE worker-agnostic kernel: it reduces a slice of the round's redex list
//! and collects newly created redexes — the single-thread driver, the host
//! equality-matrix test (real std threads) and the pinched workpool workers
//! all call it, so every execution mode runs the same rules. Confluence makes
//! the normal form independent of partitioning; the tests prove it.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host tests below (value, node balance, equality matrix,
//!   bounded reject, stuck detection)
//! ADR: docs/adr/0047-interaction-net-evaluator-backend.md

use alloc::vec::Vec;

use crate::arena::{port, port_node, Arena, InetError, NodeKind, Redex, NIL};

/// Per-chunk reduction output: newly created redexes (fed into the next
/// round) and the number of interactions performed (the honest dispatch
/// counter for the parallel-exec proof).
#[derive(Default)]
pub struct RoundOut {
    pub new_redexes: Vec<Redex>,
    pub reductions: u32,
}

/// Reduce `redexes[start..end]` (one worker's deterministic chunk). Every
/// interaction is local; ports written here belonged to nodes this chunk's
/// redexes consumed, so chunks never write the same port (atomics keep the
/// sharing race-free at the language level).
pub fn reduce_chunk(
    net: &Arena,
    redexes: &[Redex],
    start: usize,
    end: usize,
    out: &mut RoundOut,
) -> Result<(), InetError> {
    for r in redexes.iter().take(end).skip(start) {
        interact(net, *r, out)?;
        out.reductions += 1;
    }
    Ok(())
}

/// Apply the single interaction rule for an active pair.
fn interact(net: &Arena, r: Redex, out: &mut RoundOut) -> Result<(), InetError> {
    // Normalize so the smaller kind is `a` (halves the rule table).
    let (a, b) = if net.kind(r.0) as u8 <= net.kind(r.1) as u8 { (r.0, r.1) } else { (r.1, r.0) };
    let (ka, kb) = (net.kind(a), net.kind(b));
    match (ka, kb) {
        // ERA-ERA / ERA-NUM: both vanish.
        (NodeKind::Era, NodeKind::Era) | (NodeKind::Era, NodeKind::Num) => {
            net.free_node(a);
            net.free_node(b);
        }
        // ERA against a binary agent: erase both wires (reuse the existing
        // eraser for aux1, allocate one for aux2).
        (NodeKind::Era, NodeKind::Con)
        | (NodeKind::Era, NodeKind::Dup)
        | (NodeKind::Era, NodeKind::Add2) => {
            let p1 = net.peer(port(b, 1));
            let p2 = net.peer(port(b, 2));
            let e2 = net.alloc(NodeKind::Era, 0)?;
            if p1 != NIL {
                if let Some(rx) = net.link(port(a, 0), p1) {
                    out.new_redexes.push(rx);
                }
            } else {
                net.free_node(a);
            }
            if p2 != NIL {
                if let Some(rx) = net.link(port(e2, 0), p2) {
                    out.new_redexes.push(rx);
                }
            } else {
                net.free_node(e2);
            }
            net.free_node(b);
        }
        // ERA-ADD1: erase the result wire (reuse the eraser).
        (NodeKind::Era, NodeKind::Add1) => {
            let p1 = net.peer(port(b, 1));
            if p1 != NIL {
                if let Some(rx) = net.link(port(a, 0), p1) {
                    out.new_redexes.push(rx);
                }
            } else {
                net.free_node(a);
            }
            net.free_node(b);
        }
        // Same-kind binary agents annihilate: wire aux peers straight through.
        (NodeKind::Con, NodeKind::Con) | (NodeKind::Dup, NodeKind::Dup) => {
            for slot in 1..=2u8 {
                let pa = net.peer(port(a, slot));
                let pb = net.peer(port(b, slot));
                if pa != NIL && pb != NIL {
                    if let Some(rx) = net.link(pa, pb) {
                        out.new_redexes.push(rx);
                    }
                }
            }
            net.free_node(a);
            net.free_node(b);
        }
        // CON-DUP commutation: the classic 2×2 grid (allocates two, reuses
        // the consumed pair for the other two).
        (NodeKind::Con, NodeKind::Dup) => {
            let (a1, a2) = (net.peer(port(a, 1)), net.peer(port(a, 2)));
            let (b1, b2) = (net.peer(port(b, 1)), net.peer(port(b, 2)));
            let d1 = a; // reuse: becomes Dup
            let c1 = b; // reuse: becomes Con
            let d2 = net.alloc(NodeKind::Dup, 0)?;
            let c2 = net.alloc(NodeKind::Con, 0)?;
            net.set_kind_payload(d1, NodeKind::Dup, 0);
            net.set_kind_payload(c1, NodeKind::Con, 0);
            let mut push = |rx: Option<Redex>| {
                if let Some(rx) = rx {
                    out.new_redexes.push(rx);
                }
            };
            push(net.link(port(d1, 0), a1));
            push(net.link(port(d2, 0), a2));
            push(net.link(port(c1, 0), b1));
            push(net.link(port(c2, 0), b2));
            push(net.link(port(c1, 1), port(d1, 1)));
            push(net.link(port(c1, 2), port(d2, 1)));
            push(net.link(port(c2, 1), port(d1, 2)));
            push(net.link(port(c2, 2), port(d2, 2)));
        }
        // NUM-DUP: duplicate the number.
        (NodeKind::Dup, NodeKind::Num) => {
            let v = net.payload(b);
            let (d1, d2) = (net.peer(port(a, 1)), net.peer(port(a, 2)));
            let n2 = net.alloc(NodeKind::Num, v)?;
            if d1 != NIL {
                if let Some(rx) = net.link(port(b, 0), d1) {
                    out.new_redexes.push(rx);
                }
            } else {
                net.free_node(b);
            }
            if d2 != NIL {
                if let Some(rx) = net.link(port(n2, 0), d2) {
                    out.new_redexes.push(rx);
                }
            } else {
                net.free_node(n2);
            }
            net.free_node(a);
        }
        // NUM-ADD2: capture the first operand; the consumed Add2 node is
        // repurposed to Add1 (alloc-free — tree reductions never allocate).
        (NodeKind::Num, NodeKind::Add2) => {
            let v = net.payload(a);
            let x = net.peer(port(b, 1));
            let res = net.peer(port(b, 2));
            net.set_kind_payload(b, NodeKind::Add1, v);
            // Rewire: Add1 principal faces the second operand; aux1 = result.
            if x != NIL {
                if let Some(rx) = net.link(port(b, 0), x) {
                    out.new_redexes.push(rx);
                }
            }
            if res != NIL {
                if let Some(rx) = net.link(port(b, 1), res) {
                    out.new_redexes.push(rx);
                }
            }
            net.free_node(a);
        }
        // NUM-ADD1: fold to a number on the result wire (reuse the Num node).
        (NodeKind::Num, NodeKind::Add1) => {
            let sum = net.payload(a).wrapping_add(net.payload(b));
            let res = net.peer(port(b, 1));
            net.set_kind_payload(a, NodeKind::Num, sum);
            if res != NIL {
                // This link is the cascade: the folded number may hit the
                // waiting parent Add2/Add1 principal-to-principal.
                if let Some(rx) = net.link(port(a, 0), res) {
                    out.new_redexes.push(rx);
                }
            }
            net.free_node(b);
        }
        // No rule (e.g. NUM-NUM, NUM-CON in this minimal calculus): stuck.
        _ => return Err(InetError::Stuck),
    }
    Ok(())
}

/// Build the E3 proof workload: a balanced binary add-tree of depth `depth`
/// with leaves `1..=2^depth`, its output wired to a fresh Root node.
/// Returns `(root_node, initial_redexes)`. Fails closed on arena exhaustion.
pub fn build_tree_sum(net: &Arena, depth: u32) -> Result<(u32, Vec<Redex>), InetError> {
    let mut redexes = Vec::new();
    let mut next_leaf: i64 = 1;
    let out_port = build_subtree(net, depth, &mut next_leaf, &mut redexes)?;
    let root = net.alloc(NodeKind::Root, 0)?;
    if let Some(rx) = net.link(port(root, 0), out_port) {
        redexes.push(rx);
    }
    Ok((root, redexes))
}

fn build_subtree(
    net: &Arena,
    depth: u32,
    next_leaf: &mut i64,
    redexes: &mut Vec<Redex>,
) -> Result<u64, InetError> {
    if depth == 0 {
        let n = net.alloc(NodeKind::Num, *next_leaf)?;
        *next_leaf += 1;
        return Ok(port(n, 0));
    }
    let left = build_subtree(net, depth - 1, next_leaf, redexes)?;
    let right = build_subtree(net, depth - 1, next_leaf, redexes)?;
    let add = net.alloc(NodeKind::Add2, 0)?;
    // First operand drives the principal port (NUM-ADD2 fires when the left
    // subtree has folded to a number); second operand on aux1, result aux2.
    if let Some(rx) = net.link(left, port(add, 0)) {
        redexes.push(rx);
    }
    if let Some(rx) = net.link(right, port(add, 1)) {
        redexes.push(rx);
    }
    Ok(port(add, 2))
}

/// Single-threaded round driver: reduce until no redexes remain. Bounded by
/// `max_rounds` (fail-closed `Diverged`, never a silent hang). Returns the
/// total number of interactions.
pub fn reduce_to_normal_form(
    net: &Arena,
    mut redexes: Vec<Redex>,
    max_rounds: u32,
) -> Result<u32, InetError> {
    let mut total: u32 = 0;
    for _ in 0..max_rounds {
        if redexes.is_empty() {
            return Ok(total);
        }
        let mut out = RoundOut::default();
        reduce_chunk(net, &redexes, 0, redexes.len(), &mut out)?;
        total = total.saturating_add(out.reductions);
        redexes = out.new_redexes;
    }
    if redexes.is_empty() {
        Ok(total)
    } else {
        Err(InetError::Diverged)
    }
}

/// Read the number the net folded into the root's wire (the normal-form
/// "digest" for value workloads). `None` while not a NUM (not normal yet,
/// or the net computes something else).
#[must_use]
pub fn root_value(net: &Arena, root: u32) -> Option<i64> {
    let p = net.peer(port(root, 0));
    if p == NIL {
        return None;
    }
    let n = port_node(p);
    if net.kind(n) == NodeKind::Num {
        Some(net.payload(n))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected_sum(depth: u32) -> i64 {
        let leaves = 1i64 << depth;
        leaves * (leaves + 1) / 2
    }

    #[test]
    fn tree_sum_reduces_to_the_expected_value() {
        for depth in [0u32, 1, 3, 6] {
            let net = Arena::new(4096);
            let (root, redexes) = build_tree_sum(&net, depth).expect("build");
            let reds = reduce_to_normal_form(&net, redexes, 10_000).expect("reduce");
            assert_eq!(root_value(&net, root), Some(expected_sum(depth)), "depth {depth}");
            if depth > 0 {
                assert!(reds > 0, "must actually interact");
            }
        }
    }

    #[test]
    fn reduction_recycles_nodes() {
        let net = Arena::new(1024);
        let before = net.free_len();
        let (_root, redexes) = build_tree_sum(&net, 5).expect("build");
        reduce_to_normal_form(&net, redexes, 10_000).expect("reduce");
        // Normal form = Root + one Num: everything else recycled.
        assert_eq!(net.free_len(), before - 2, "net must recycle to Root + Num");
    }

    #[test]
    fn arena_exhaustion_is_a_bounded_reject() {
        let net = Arena::new(16);
        assert_eq!(build_tree_sum(&net, 6).unwrap_err(), InetError::ArenaExhausted);
    }

    #[test]
    fn equality_matrix_workers_1_to_4_with_real_threads() {
        // Round-based parallel reduction with REAL std threads: partition each
        // round's redexes with the workpool's chunk math, reduce chunks
        // concurrently, merge. Confluence: value AND total interaction count
        // must match the single-thread run for every worker count.
        let single = {
            let net = Arena::new(8192);
            let (root, redexes) = build_tree_sum(&net, 6).expect("build");
            let reds = reduce_to_normal_form(&net, redexes, 10_000).expect("reduce");
            (root_value(&net, root), reds)
        };
        for workers in 1usize..=4 {
            let net = Arena::new(8192);
            let (root, mut redexes) = build_tree_sum(&net, 6).expect("build");
            let mut total = 0u32;
            for _round in 0..10_000 {
                if redexes.is_empty() {
                    break;
                }
                let outs: Vec<RoundOut> = std::thread::scope(|scope| {
                    let net = &net;
                    let redexes = &redexes;
                    let mut handles = Vec::new();
                    for idx in 0..workers {
                        let chunk = chunk_bounds(redexes.len(), workers, idx);
                        handles.push(scope.spawn(move || {
                            let mut out = RoundOut::default();
                            reduce_chunk(net, redexes, chunk.0, chunk.1, &mut out).expect("chunk");
                            out
                        }));
                    }
                    handles.into_iter().map(|h| h.join().expect("join")).collect()
                });
                let mut next = Vec::new();
                for out in outs {
                    total += out.reductions;
                    next.extend(out.new_redexes);
                }
                redexes = next;
            }
            assert!(redexes.is_empty(), "workers={workers} diverged");
            assert_eq!(
                (root_value(&net, root), total),
                single,
                "equality violated at workers={workers}"
            );
        }
    }

    /// Local copy of the workpool's deterministic chunk math (kept in sync by
    /// the shared contract, not by import — nexus-workpool is not a dep).
    fn chunk_bounds(total: usize, workers: usize, idx: usize) -> (usize, usize) {
        if workers == 0 || idx >= workers {
            return (0, 0);
        }
        let base = total / workers;
        let rem = total % workers;
        let extra = if idx < rem { idx } else { rem };
        let start = idx * base + extra;
        (start, start + base + usize::from(idx < rem))
    }
}
