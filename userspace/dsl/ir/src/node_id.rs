// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Stable view-node identity.
//!
//! `nodeId = hash64(component symbol ∥ structural path ∥ optional user key)`,
//! computed at build time and **persisted in the IR** so the retained instance
//! tree, AOT output, goldens, and a11y references agree across rebuilds.
//! Collection items derive their id at runtime from the parent's id and the
//! evaluated `.key(expr)` value with the same function.
//!
//! Algorithm: FNV-1a 64 over a length-prefixed byte stream. Not cryptographic —
//! identity, not integrity (integrity is `programHash`). The algorithm is part
//! of the IR contract and must never change within a schema major.

/// FNV-1a 64 offset basis / prime.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Incremental FNV-1a 64 hasher over length-prefixed segments.
#[derive(Clone, Copy)]
pub struct NodeIdHasher(u64);

impl NodeIdHasher {
    #[must_use]
    pub fn new() -> Self {
        Self(FNV_OFFSET)
    }

    fn bytes(mut self, bytes: &[u8]) -> Self {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
        self
    }

    /// Appends one segment (length-prefixed so `("ab","c")` ≠ `("a","bc")`).
    #[must_use]
    pub fn segment(self, segment: &[u8]) -> Self {
        let len = u32::try_from(segment.len()).unwrap_or(u32::MAX);
        self.bytes(&len.to_le_bytes()).bytes(segment)
    }

    /// Appends a structural child index.
    #[must_use]
    pub fn index(self, index: u32) -> Self {
        self.bytes(&index.to_le_bytes())
    }

    #[must_use]
    pub fn finish(self) -> u64 {
        self.0
    }
}

impl Default for NodeIdHasher {
    fn default() -> Self {
        Self::new()
    }
}

/// Build-time id for a static view node.
#[must_use]
pub fn static_node_id(component_name: &str, structural_path: &[u32]) -> u64 {
    let mut h = NodeIdHasher::new().segment(component_name.as_bytes());
    for &idx in structural_path {
        h = h.index(idx);
    }
    h.finish()
}

/// Runtime id for a keyed collection item: parent template id ∥ key bytes.
#[must_use]
pub fn keyed_item_id(template_node_id: u64, key_bytes: &[u8]) -> u64 {
    NodeIdHasher::new()
        .segment(&template_node_id.to_le_bytes())
        .segment(key_bytes)
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_distinct() {
        let a = static_node_id("UserListPage", &[0, 1]);
        let b = static_node_id("UserListPage", &[0, 1]);
        let c = static_node_id("UserListPage", &[0, 2]);
        let d = static_node_id("DetailPage", &[0, 1]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn length_prefix_prevents_segment_gluing() {
        let ab_c = NodeIdHasher::new().segment(b"ab").segment(b"c").finish();
        let a_bc = NodeIdHasher::new().segment(b"a").segment(b"bc").finish();
        assert_ne!(ab_c, a_bc);
    }

    #[test]
    fn keyed_items_differ_by_key_and_parent() {
        let t1 = static_node_id("P", &[0]);
        let t2 = static_node_id("P", &[1]);
        assert_ne!(keyed_item_id(t1, b"7"), keyed_item_id(t1, b"8"));
        assert_ne!(keyed_item_id(t1, b"7"), keyed_item_id(t2, b"7"));
    }
}
