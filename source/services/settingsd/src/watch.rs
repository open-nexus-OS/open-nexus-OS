// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0078 watch spine (amended by RFC-0083) — the bounded
//! subscriber table behind settingsd's `OP_WATCH`/`OP_EVENT`. Pure and
//! host-tested: the os_lite loop plugs in the actual IPC send. Delivery is
//! SELF-HEALING: a failed send flags the subscriber for resync, and the next
//! change re-sends ALL of its matching current values (not just the changed
//! key) — a dropped event can therefore never become permanent drift.
//! Registration answers with the same full sync (the boot-restore path).
//! Healing by re-WATCH is forbidden: every registration cap-moves a fresh
//! channel and would leak table slots.
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

    /// Notifies every matching subscriber of an APPLIED change. `current` is
    /// the registry's full current state (key, value) — a subscriber whose
    /// earlier delivery was dropped (resync flagged) is HEALED here: it gets
    /// ALL of its matching current values instead of only the changed key, so
    /// the dropped value cannot survive as silent drift. `send` posts one
    /// encoded frame to a channel slot and reports success.
    pub fn notify(
        &mut self,
        key: &str,
        value: &str,
        current: &[(&str, &str)],
        mut send: impl FnMut(u32, &[u8]) -> bool,
    ) {
        for slot in self.slots.iter_mut() {
            let Some(w) = slot else { continue };
            let prefix = &w.prefix[..usize::from(w.prefix_len)];
            if !key.as_bytes().starts_with(prefix) {
                continue;
            }
            let delivered = if w.resync {
                // Heal: full matching state, each frame resync-flagged (the
                // receiver treats them as idempotent snapshot values).
                Self::send_matching(prefix, current, wire::EVENT_FLAG_RESYNC, &mut send, w.chan)
            } else {
                Self::send_one(w.chan, 0, key, value, &mut send)
            };
            if delivered {
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

    /// Full state sync to ONE subscriber (RFC-0083 registration burst): every
    /// current value matching its prefix, resync-flagged. This is the boot
    /// restore — a fresh watcher starts converged instead of waiting for the
    /// next change. Failures flag resync; `notify` heals them later.
    /// Returns `(matching, delivered)` so the caller can emit one honest
    /// diagnostic line (a silent burst was undiagnosable from a boot log).
    pub fn sync(
        &mut self,
        chan: u32,
        current: &[(&str, &str)],
        mut send: impl FnMut(u32, &[u8]) -> bool,
    ) -> (usize, usize) {
        for slot in self.slots.iter_mut() {
            let Some(w) = slot else { continue };
            if w.chan != chan {
                continue;
            }
            let prefix = &w.prefix[..usize::from(w.prefix_len)];
            let (matching, delivered) = Self::send_matching_counted(
                prefix,
                current,
                wire::EVENT_FLAG_RESYNC,
                &mut send,
                w.chan,
            );
            if matching == delivered {
                w.resync = false;
                w.failures = 0;
            } else {
                w.resync = true;
                w.failures = w.failures.saturating_add(1);
            }
            return (matching, delivered);
        }
        (0, 0)
    }

    /// Sends every `current` entry matching `prefix`; true only if ALL landed.
    fn send_matching(
        prefix: &[u8],
        current: &[(&str, &str)],
        flags: u8,
        send: &mut impl FnMut(u32, &[u8]) -> bool,
        chan: u32,
    ) -> bool {
        let (matching, delivered) = Self::send_matching_counted(prefix, current, flags, send, chan);
        matching == delivered
    }

    /// [`send_matching`] with `(matching, delivered)` counts.
    fn send_matching_counted(
        prefix: &[u8],
        current: &[(&str, &str)],
        flags: u8,
        send: &mut impl FnMut(u32, &[u8]) -> bool,
        chan: u32,
    ) -> (usize, usize) {
        let (mut matching, mut delivered) = (0usize, 0usize);
        for (k, v) in current {
            if !k.as_bytes().starts_with(prefix) {
                continue;
            }
            matching += 1;
            if Self::send_one(chan, flags, k, v, send) {
                delivered += 1;
            }
        }
        (matching, delivered)
    }

    fn send_one(
        chan: u32,
        flags: u8,
        key: &str,
        value: &str,
        send: &mut impl FnMut(u32, &[u8]) -> bool,
    ) -> bool {
        let mut frame = [0u8; 600];
        let Some(n) = wire::encode_event(flags, key, value, &mut frame) else {
            return true; // oversized key/value cannot happen via the registry
        };
        send(chan, &frame[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    /// A registry-like current-state snapshot for the heal/burst paths.
    const CURRENT: &[(&str, &str)] = &[
        ("ui.theme.mode", "dark"),
        ("ui.locale", "de-DE"),
        ("time.zone", "Europe/Berlin"),
        ("input.keymap", "de"),
    ];

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
        t.notify("input.keymap", "de", CURRENT, sent(&mut events));
        t.notify("ui.locale", "en-US", CURRENT, sent(&mut events));
        t.notify("time.zone", "UTC", CURRENT, sent(&mut events));
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
        t.notify("time.format", "12h", CURRENT, sent(&mut events));
        assert_eq!(events, std::vec![(0, 0, "time.format".into())]);
    }

    /// RFC-0083 heal: after a dropped delivery the subscriber's NEXT event is
    /// the full matching state (resync-flagged), not just the changed key —
    /// the dropped value can never survive as silent drift.
    #[test]
    fn a_dropped_send_heals_with_the_full_matching_state() {
        let mut t = WatchTable::new();
        assert!(t.register(5, "ui."));
        // First delivery fails → the watcher is out of sync.
        t.notify("ui.locale", "de", CURRENT, |_, _| false);
        // Next change: BOTH ui.* current values arrive, resync-flagged.
        let mut events = Vec::new();
        t.notify("ui.theme.mode", "light", CURRENT, sent(&mut events));
        let keys: Vec<&str> = events.iter().map(|(_, _, k)| k.as_str()).collect();
        assert_eq!(keys, ["ui.theme.mode", "ui.locale"], "full ui.* state, not one key");
        assert!(events.iter().all(|(_, f, _)| *f == wire::EVENT_FLAG_RESYNC));
        // Healed: the change after that is a plain single event again.
        events.clear();
        t.notify("ui.locale", "fr", CURRENT, sent(&mut events));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, 0);
    }

    /// RFC-0083 registration burst: a fresh watcher starts CONVERGED — it
    /// immediately receives every current value under its prefix. This is the
    /// boot-restore path (no polling probe needed by subscribers).
    #[test]
    fn registration_sync_delivers_current_matching_values() {
        let mut t = WatchTable::new();
        assert!(t.register(3, "ui."));
        let mut events = Vec::new();
        t.sync(3, CURRENT, sent(&mut events));
        let keys: Vec<&str> = events.iter().map(|(_, _, k)| k.as_str()).collect();
        assert_eq!(keys, ["ui.theme.mode", "ui.locale"]);
        assert!(events.iter().all(|(_, f, _)| *f == wire::EVENT_FLAG_RESYNC));
        // A failed burst flags resync; the next notify heals (full state).
        assert!(t.register(4, "time."));
        t.sync(4, CURRENT, |_, _| false);
        events.clear();
        t.notify("time.format", "12h", CURRENT, sent(&mut events));
        let for_4: Vec<&str> =
            events.iter().filter(|(c, _, _)| *c == 4).map(|(_, _, k)| k.as_str()).collect();
        assert_eq!(for_4, ["time.zone"], "healed with the full time.* state");
    }

    #[test]
    fn persistent_failure_reclaims_the_slot() {
        let mut t = WatchTable::new();
        assert!(t.register(5, "ui."));
        for _ in 0..8 {
            t.notify("ui.locale", "xx", CURRENT, |_, _| false);
        }
        let mut events = Vec::new();
        t.notify("ui.locale", "yy", CURRENT, sent(&mut events));
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
