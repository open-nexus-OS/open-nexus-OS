// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The per-pass measurement cache of the layout engine: a node's size under the same
//! constraints is usually measured once per layout pass, not once per ancestor level.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `engine_tests.rs` (`a_deep_tree_measures_its_text_a_bounded_number_of_times`)
//!
//! A column measures each child's whole subtree to size it, then places the child — and placing
//! measures the grandchildren again, one level down, and so on: without the cache every text in
//! a deep tree was measured once per ancestor (the settings window: 49 texts, 1,361 text
//! measurements in one pass; 586 with the cache). The key is the node's address and its
//! constraints: the scene is not mutated during a pass, and the cache is cleared when a pass
//! starts, so an address the next scene reuses can never hit. The table is small and
//! direct-mapped (a collision overwrites, a miss measures again): measured against the frame
//! arena budget, 128 slots kept nearly all the hits and allocated less than the measurements
//! they save; a map or a larger table cost more memory than it saved.

use crate::engine::{LayoutConstraints, LayoutEngine, NodeSize};
use crate::error::LayoutError;
use alloc::vec::Vec;
use core::cell::RefCell;
use nexus_layout_types::{LayoutNode, MeasureText};

/// `(node address, max width, max height or i32::MIN, definite, fit target, fit min)`.
type Key = (usize, i32, i32, bool, i32, i32);

/// The table's slots: a power of two (see the module doc for why 128). Direct-mapped — a
/// collision overwrites (a cache, never a map: a miss measures again), so the table is one
/// allocation of a fixed size per pass.
const SLOTS: usize = 128;

/// Sizes measured in the current pass.
#[derive(Debug, Default)]
pub(crate) struct MeasureCache {
    slots: RefCell<Vec<Option<(Key, NodeSize)>>>,
}

impl MeasureCache {
    /// Forgets every size: a new pass lays out a new scene.
    pub(crate) fn clear(&self) {
        let mut slots = self.slots.borrow_mut();
        slots.iter_mut().for_each(|s| *s = None);
    }

    fn get(&self, k: &Key) -> Option<NodeSize> {
        let slots = self.slots.borrow();
        match slots.get(slot(k)) {
            Some(Some((key, size))) if key == k => Some(*size),
            _ => None,
        }
    }

    fn put(&self, k: Key, size: NodeSize) {
        let mut slots = self.slots.borrow_mut();
        if slots.is_empty() {
            slots.resize(SLOTS, None);
        }
        let i = slot(&k);
        slots[i] = Some((k, size));
    }
}

fn slot(k: &Key) -> usize {
    // FNV-style mix of the fields; the address dominates (nodes are distinct), the constraints
    // separate a node's measurements under different widths.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in [k.0 as u64, k.1 as u64, k.2 as u64, u64::from(k.3), k.4 as u64, k.5 as u64] {
        h = (h ^ v).wrapping_mul(0x0100_0000_01b3);
    }
    (h ^ (h >> 29)) as usize & (SLOTS - 1)
}

fn key(node: &LayoutNode, c: &LayoutConstraints) -> Key {
    let (fit_target, fit_min) = c.text_px.map_or((i32::MIN, i32::MIN), |f| (f.target.0, f.min.0));
    (
        node as *const LayoutNode as usize,
        c.max_width.0,
        c.max_height.map_or(i32::MIN, |h| h.0),
        c.definite,
        fit_target,
        fit_min,
    )
}

impl LayoutEngine {
    /// The size of `node` under `constraints` — from the table when this pass measured it.
    pub(crate) fn measure_node(
        &self,
        node: &LayoutNode,
        constraints: LayoutConstraints,
        depth: usize,
        measure: &dyn MeasureText,
    ) -> Result<NodeSize, LayoutError> {
        let k = key(node, &constraints);
        if let Some(size) = self.cache.get(&k) {
            return Ok(size);
        }
        let size = self.measure_node_raw(node, constraints, depth, measure)?;
        self.cache.put(k, size);
        Ok(size)
    }
}
