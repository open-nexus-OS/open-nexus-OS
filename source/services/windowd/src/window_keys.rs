// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd's window KEYS (RFC-0114 §7): every window a slot holds gets a fresh key — its
//! layers' id and owner, its damage's origin and its transform on every wire. Keys climb through
//! `1..=0xFFFE` (`0` names no window, `0xFFFF` is the desktop's) and skip every key a live window
//! holds: a key comes back only after every other one was handed out, long after its window's
//! last frame retired. Pure and host-tested; the slots holding the keys are
//! `compositor/runtime/slots.rs`.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (climb, skip, wrap, exhaustion)

/// The desktop's key on the wire: the one key no window takes.
pub(crate) const DESKTOP_KEY: u16 = 0xFFFF;

/// The last key a window may take.
const LAST_KEY: u16 = DESKTOP_KEY - 1;

/// The keys handed out so far: where the next search starts.
pub(crate) struct WindowKeys {
    next: u16,
}

impl WindowKeys {
    pub(crate) const fn new() -> Self {
        Self { next: 1 }
    }

    /// A key `live` does not name, climbing from the last one handed out; `None` when every
    /// window key is live (the search is bounded by the key space, never a spin).
    pub(crate) fn fresh(&mut self, live: impl Fn(u16) -> bool) -> Option<u16> {
        for _ in 0..LAST_KEY {
            let key = self.next;
            self.next = if key == LAST_KEY { 1 } else { key + 1 };
            if !live(key) {
                return Some(key);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_climb_from_one() {
        let mut keys = WindowKeys::new();
        assert_eq!(keys.fresh(|_| false), Some(1));
        assert_eq!(keys.fresh(|_| false), Some(2));
        assert_eq!(keys.fresh(|_| false), Some(3));
    }

    /// RFC-0114 §7: a live window's key is never handed to another window — after the wrap
    /// neither — and neither `0` nor the desktop's key is ever a window's.
    #[test]
    fn test_reject_window_key_collision() {
        let mut keys = WindowKeys::new();
        let live = [1u16, 2, 5];
        assert_eq!(keys.fresh(|k| live.contains(&k)), Some(3));
        assert_eq!(keys.fresh(|k| live.contains(&k)), Some(4));
        assert_eq!(keys.fresh(|k| live.contains(&k)), Some(6), "a live key is skipped");
        // Up to the last key, then around: the live keys at the bottom are skipped again.
        keys.next = LAST_KEY;
        assert_eq!(keys.fresh(|k| live.contains(&k)), Some(LAST_KEY));
        assert_eq!(keys.fresh(|k| live.contains(&k)), Some(3), "wrapped past 0, 1 and 2");
        let mut seen = 0usize;
        let mut keys = WindowKeys::new();
        while let Some(k) = keys.fresh(|_| false) {
            assert!(k != 0 && k != DESKTOP_KEY, "key {k} is no window's");
            seen += 1;
            if seen > usize::from(LAST_KEY) {
                break;
            }
        }
    }

    #[test]
    fn every_key_live_is_none() {
        let mut keys = WindowKeys::new();
        assert_eq!(keys.fresh(|_| true), None);
        // The one free key is found wherever the search starts.
        keys.next = 77;
        assert_eq!(keys.fresh(|k| k != 12), Some(12));
    }
}
