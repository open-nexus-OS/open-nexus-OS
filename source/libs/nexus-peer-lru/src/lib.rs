// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bounded LRU cache for DSoftBus discovery peers (no_std)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (Phase 1)
//! TEST_COVERAGE: 3 tests (insert, evict, lookup)
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//! RFC: docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md
//!
//! Notes:
//! - This is a minimal LRU for discovery peer caching in OS/QEMU bring-up.
//! - Fixed capacity (MAX_PEERS = 16) to keep memory bounded.
//! - No timestamps; uses simple access ordering.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

pub const MAX_PEERS: usize = 16;

/// Peer entry in the discovery cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerEntry {
    pub device_id: String,
    pub port: u16,
    pub noise_static: [u8; 32],
    pub services: Vec<String>,
}

impl PeerEntry {
    pub fn new(
        device_id: String,
        port: u16,
        noise_static: [u8; 32],
        services: Vec<String>,
    ) -> Self {
        Self { device_id, port, noise_static, services }
    }
}

/// Bounded LRU cache for discovery peers.
pub struct PeerLru {
    entries: Vec<PeerEntry>,
    capacity: usize,
}

impl PeerLru {
    pub fn new(capacity: usize) -> Self {
        Self { entries: Vec::new(), capacity }
    }

    pub fn with_default_capacity() -> Self {
        Self::new(MAX_PEERS)
    }

    /// Insert or update a peer. Moves to front (most recently used).
    pub fn insert(&mut self, peer: PeerEntry) {
        // Check if peer already exists (by device_id)
        if let Some(pos) = self.entries.iter().position(|p| p.device_id == peer.device_id) {
            // Remove old entry
            self.entries.remove(pos);
        } else if self.entries.len() >= self.capacity {
            // Evict least recently used (last in list)
            self.entries.pop();
        }
        // Insert at front (most recently used)
        self.entries.insert(0, peer);
    }

    /// Get a peer by device_id. Moves to front if found.
    pub fn get(&mut self, device_id: &str) -> Option<&PeerEntry> {
        if let Some(pos) = self.entries.iter().position(|p| p.device_id == device_id) {
            // Move to front
            let entry = self.entries.remove(pos);
            self.entries.insert(0, entry);
            return self.entries.first();
        }
        None
    }

    /// Get a peer without updating LRU order.
    pub fn peek(&self, device_id: &str) -> Option<&PeerEntry> {
        self.entries.iter().find(|p| p.device_id == device_id)
    }

    /// Get all peers (most recent first).
    pub fn peers(&self) -> &[PeerEntry] {
        &self.entries
    }

    /// Get peer count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_insert_and_get() {
        let mut lru = PeerLru::new(3);
        let peer1 = PeerEntry::new("node1".into(), 1000, [0x11; 32], alloc::vec!["svc1".into()]);
        let peer2 = PeerEntry::new("node2".into(), 2000, [0x22; 32], alloc::vec!["svc2".into()]);

        lru.insert(peer1.clone());
        lru.insert(peer2.clone());

        assert_eq!(lru.len(), 2);
        assert_eq!(lru.get("node1").unwrap().port, 1000);
        assert_eq!(lru.get("node2").unwrap().port, 2000);
    }

    #[test]
    fn lru_eviction() {
        let mut lru = PeerLru::new(2);
        let peer1 = PeerEntry::new("node1".into(), 1000, [0x11; 32], alloc::vec![]);
        let peer2 = PeerEntry::new("node2".into(), 2000, [0x22; 32], alloc::vec![]);
        let peer3 = PeerEntry::new("node3".into(), 3000, [0x33; 32], alloc::vec![]);

        lru.insert(peer1.clone());
        lru.insert(peer2.clone());
        assert_eq!(lru.len(), 2);

        // Insert peer3, should evict peer1 (least recently used)
        lru.insert(peer3.clone());
        assert_eq!(lru.len(), 2);
        assert!(lru.peek("node1").is_none());
        assert!(lru.peek("node2").is_some());
        assert!(lru.peek("node3").is_some());
    }

    #[test]
    fn lru_update_moves_to_front() {
        let mut lru = PeerLru::new(3);
        let peer1 = PeerEntry::new("node1".into(), 1000, [0x11; 32], alloc::vec![]);
        let peer2 = PeerEntry::new("node2".into(), 2000, [0x22; 32], alloc::vec![]);
        let peer1_updated = PeerEntry::new("node1".into(), 1001, [0x11; 32], alloc::vec![]);

        lru.insert(peer1.clone());
        lru.insert(peer2.clone());

        // Update peer1 (should move to front)
        lru.insert(peer1_updated.clone());

        assert_eq!(lru.peers()[0].device_id, "node1");
        assert_eq!(lru.peers()[0].port, 1001);
    }
}
