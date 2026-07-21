// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0078 watch spine — the bounded subscriber table behind
//! settingsd's `OP_WATCH`/`OP_EVENT`. Pure and host-tested: the os_lite loop
//! plugs in the actual IPC send; failures set the per-subscriber resync flag
//! (drop-oldest semantics) and repeated failures reclaim the slot.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: Unit tests below (register/replace/overflow, prefix match,
//! resync + reclaim).
//! RFC: docs/rfcs/RFC-0078-settings-region-keys-watch.md

use nexus_wire::settingsd as wire;

/// RFC-0078 bound: at most this many concurrent watch subscribers.
pub const MAX_WATCHERS: usize = 8;
/// Consecutive send failures before a subscriber slot is reclaimed
/// (a closed/full-forever channel must not occupy the bounded table).
const RECLAIM_AFTER_FAILURES: u8 = 8;

#[derive(Clone, Copy)]
struct Watcher {
    /// The subscriber's push-channel SEND cap slot (cap-moved in OP_WATCH).
    chan: u32,
    prefix: [u8; wire::WATCH_PREFIX_MAX],
    prefix_len: u8,
    /// Set when a delivery was dropped; cleared on the next successful send
    /// (which carries `EVENT_FLAG_RESYNC`).
    resync: bool,
    failures: u8,
}

/// The bounded subscriber table.
pub struct WatchTable {
    slots: [Option<Watcher>; MAX_WATCHERS],
}

impl Default for WatchTable {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchTable {
    #[must_use]
    pub const fn new() -> Self {
        Self { slots: [None; MAX_WATCHERS] }
    }

    /// Registers (or re-prefixes) the subscriber on `chan`. Returns `false`
    /// when the table is full (the caller answers an honest reject).
    pub fn register(&mut self, chan: u32, prefix: &str) -> bool {
        let bytes = prefix.as_bytes();
        if bytes.is_empty() || bytes.len() > wire::WATCH_PREFIX_MAX {
            return false;
        }
        let mut buf = [0u8; wire::WATCH_PREFIX_MAX];
        buf[..bytes.len()].copy_from_slice(bytes);
        let watcher = Watcher {
            chan,
            prefix: buf,
            prefix_len: bytes.len() as u8,
            resync: false,
            failures: 0,
        };
        // Same channel re-watches replace the prefix (RFC-0078).
        if let Some(slot) = self.slots.iter_mut().flatten().find(|w| w.chan == chan) {
            *slot = watcher;
            return true;
        }
        if let Some(free) = self.slots.iter_mut().find(|s| s.is_none()) {
            *free = Some(watcher);
            return true;
        }
        false
    }

    /// Notifies every matching subscriber of an APPLIED change. `send` posts
    /// one encoded frame to a channel slot and reports success; failures set
    /// the resync flag (and eventually reclaim the slot).
    pub fn notify(&mut self, key: &str, value: &str, mut send: impl FnMut(u32, &[u8]) -> bool) {
        for slot in self.slots.iter_mut() {
            let Some(w) = slot else { continue };
            let prefix = &w.prefix[..usize::from(w.prefix_len)];
            if !key.as_bytes().starts_with(prefix) {
                continue;
            }
            let flags = if w.resync { wire::EVENT_FLAG_RESYNC } else { 0 };
            let mut frame = [0u8; 600];
            let Some(n) = wire::encode_event(flags, key, value, &mut frame) else {
                continue; // oversized key/value cannot happen via the registry
            };
            if send(w.chan, &frame[..n]) {
                w.resync = false;
                w.failures = 0;
            } else {
                w.resync = true;
                w.failures = w.failures.saturating_add(1);
                if w.failures >= RECLAIM_AFTER_FAILURES {
                    *slot = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    fn sent(
        events: &mut Vec<(u32, u8, std::string::String)>,
    ) -> impl FnMut(u32, &[u8]) -> bool + '_ {
        |chan, frame| {
            let (flags, key, _v) = wire::decode_event(frame).expect("event decodes");
            events.push((chan, flags, key.into()));
            true
        }
    }

    #[test]
    fn prefix_matching_routes_only_matching_keys() {
        let mut t = WatchTable::new();
        assert!(t.register(7, "input."));
        assert!(t.register(9, "time."));
        let mut events = Vec::new();
        t.notify("input.keymap", "de", sent(&mut events));
        t.notify("ui.locale", "en-US", sent(&mut events));
        t.notify("time.zone", "UTC", sent(&mut events));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, 7);
        assert_eq!(events[1].0, 9);
    }

    #[test]
    fn rewatch_replaces_prefix_and_overflow_rejects() {
        let mut t = WatchTable::new();
        for i in 0..MAX_WATCHERS as u32 {
            assert!(t.register(i, "ui."));
        }
        assert!(!t.register(99, "ui."), "table full");
        // Re-watch on an existing channel replaces, not adds.
        assert!(t.register(0, "time."));
        let mut events = Vec::new();
        t.notify("time.format", "12h", sent(&mut events));
        assert_eq!(events, std::vec![(0, 0, "time.format".into())]);
    }

    #[test]
    fn failed_sends_set_resync_then_reclaim() {
        let mut t = WatchTable::new();
        assert!(t.register(5, "ui."));
        // First delivery fails → next carries the resync flag.
        t.notify("ui.locale", "de", |_, _| false);
        let mut events = Vec::new();
        t.notify("ui.locale", "en", sent(&mut events));
        assert_eq!(events[0].1, wire::EVENT_FLAG_RESYNC);
        // A recovered channel clears the flag again.
        events.clear();
        t.notify("ui.locale", "fr", sent(&mut events));
        assert_eq!(events[0].1, 0);
        // Persistent failure reclaims the slot (bounded table hygiene).
        for _ in 0..8 {
            t.notify("ui.locale", "xx", |_, _| false);
        }
        events.clear();
        t.notify("ui.locale", "yy", sent(&mut events));
        assert!(events.is_empty(), "dead subscriber reclaimed");
    }

    #[test]
    fn test_reject_invalid_prefixes() {
        let mut t = WatchTable::new();
        assert!(!t.register(1, ""));
        let long = core::str::from_utf8(&[b'a'; wire::WATCH_PREFIX_MAX + 1]).unwrap();
        assert!(!t.register(1, long));
    }
}
