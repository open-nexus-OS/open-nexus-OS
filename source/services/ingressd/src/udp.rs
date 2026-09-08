// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: UDP datagram relay bookkeeping (RFC-0092 §3): the peer
//! `(ip, port)` behind each forwarded datagram is remembered in a bounded
//! table so the backend's reply finds its way back; a full table evicts
//! the least recently used peer (never grows, never blocks).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: unit tests below

/// Remembered peers per UDP exposure.
pub const MAX_UDP_PEERS: usize = 16;

/// A peer endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    pub ip: [u8; 4],
    pub port: u16,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    peer: Peer,
    last_ns: u64,
}

/// LRU peer table of one UDP exposure.
#[derive(Debug, Clone, Copy)]
pub struct PeerTable<const N: usize = MAX_UDP_PEERS> {
    entries: [Option<Entry>; N],
}

impl<const N: usize> Default for PeerTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> PeerTable<N> {
    /// Empty table.
    pub const fn new() -> Self {
        Self { entries: [None; N] }
    }

    /// Records `peer` at `now_ns`; returns its slot (existing, free, or the
    /// least recently used one, evicted).
    pub fn remember(&mut self, peer: Peer, now_ns: u64) -> usize {
        if let Some(i) = self.entries.iter().position(|e| e.is_some_and(|e| e.peer == peer)) {
            self.entries[i] = Some(Entry { peer, last_ns: now_ns });
            return i;
        }
        let i = self.entries.iter().position(Option::is_none).unwrap_or_else(|| {
            self.entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.map_or(0, |e| e.last_ns))
                .map_or(0, |(i, _)| i)
        });
        self.entries[i] = Some(Entry { peer, last_ns: now_ns });
        i
    }

    /// The peer in `slot`, if any.
    pub fn peer(&self, slot: usize) -> Option<Peer> {
        self.entries.get(slot).and_then(|e| e.map(|e| e.peer))
    }

    /// Slot of `peer`, if remembered.
    pub fn slot_of(&self, peer: Peer) -> Option<usize> {
        self.entries.iter().position(|e| e.is_some_and(|e| e.peer == peer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used() {
        let mut t: PeerTable<2> = PeerTable::new();
        let a = Peer { ip: [10, 0, 2, 2], port: 40_000 };
        let b = Peer { ip: [10, 0, 2, 3], port: 40_001 };
        let c = Peer { ip: [10, 0, 2, 4], port: 40_002 };
        assert_eq!(t.remember(a, 1), 0);
        assert_eq!(t.remember(b, 2), 1);
        assert_eq!(t.remember(a, 3), 0); // refresh a
        assert_eq!(t.remember(c, 4), 1); // b was LRU
        assert_eq!(t.slot_of(b), None);
        assert_eq!(t.peer(1), Some(c));
        assert_eq!(t.slot_of(a), Some(0));
    }
}
