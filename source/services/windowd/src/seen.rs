// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What the compositor STATES about each app window to the lifecycle authority
//! (TASK-0333 6d, RFC-0111 §4, ADR-0083): focused, visible, hidden or gone — derived from the
//! same slot facts the shell's window feed is built from, published per window as an
//! `OP_SEEN` frame keyed by the owning process's kernel service id (`app:<bundle_id>`), and
//! deduplicated against the last sighting the authority took. The compositor decides nothing
//! about lifecycle: it never knows whether an app is frozen, it only reports what is seen.
//! Pure and host-tested; the send is `compositor/runtime/windows_feed.rs`.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (derivation, dedupe, slot reuse, the frame)

use crate::window_feed::SlotInfo;
use crate::window_scene::MAX_APP_WINDOWS;

/// What is seen of one app window — the `seen` byte of `OP_SEEN` (abilitymgr's `seen::Seen`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    Focused = 0,
    Visible = 1,
    Hidden = 2,
    /// The window closed: nothing of the app is on screen any more.
    Gone = 3,
}

/// The sighting a slot's facts amount to; `None` for a slot without identity (nobody to tell).
#[must_use]
pub fn seen_of(slot: &SlotInfo) -> Option<Seen> {
    if slot.owner_sid == 0 {
        return None;
    }
    Some(if !slot.open {
        Seen::Gone
    } else if slot.minimized {
        Seen::Hidden
    } else if slot.focused {
        Seen::Focused
    } else {
        Seen::Visible
    })
}

/// The last sighting the authority TOOK per slot, keyed by owner so a slot reused by another
/// app reports again. A sighting counts as taken only when its send succeeded (`commit`); a
/// failed send leaves it due, and the next pass sends it again.
#[derive(Clone, Copy, Debug, Default)]
pub struct SeenTable {
    pub(crate) taken: [Option<(u64, Seen)>; MAX_APP_WINDOWS],
}

/// One sighting due to be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Due {
    pub slot: usize,
    pub owner_sid: u64,
    pub seen: Seen,
}

impl SeenTable {
    /// The sightings `slots` yield that the authority has not taken yet. A slot that emptied
    /// (closed, or never had identity) after a taken sighting is `Gone` for the owner it had.
    pub fn due(&self, slots: &[SlotInfo], out: &mut [Due; MAX_APP_WINDOWS]) -> usize {
        let mut n = 0;
        for (i, slot) in slots.iter().enumerate().take(MAX_APP_WINDOWS) {
            let want = match (seen_of(slot), self.taken[i]) {
                (Some(seen), _) => Some((slot.owner_sid, seen)),
                // Identity lost (the slot is free again): the last owner's window is gone.
                (None, Some((owner, taken))) if taken != Seen::Gone => Some((owner, Seen::Gone)),
                (None, _) => None,
            };
            let Some((owner_sid, seen)) = want else { continue };
            if self.taken[i] == Some((owner_sid, seen)) {
                continue;
            }
            out[n] = Due { slot: i, owner_sid, seen };
            n += 1;
        }
        n
    }

    /// The authority took `due` (its send succeeded).
    pub fn commit(&mut self, due: Due) {
        if let Some(slot) = self.taken.get_mut(due.slot) {
            *slot = Some((due.owner_sid, due.seen));
        }
    }
}

/// `[A,M,1,OP_SEEN(6), owner_sid:u64le, seen:u8]` — abilitymgr's wire (`protocol::OP_SEEN`).
#[must_use]
pub fn encode_seen(owner_sid: u64, seen: Seen) -> [u8; 13] {
    let mut out = [b'A', b'M', 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, seen as u8];
    out[4..12].copy_from_slice(&owner_sid.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(owner_sid: u64, open: bool, minimized: bool, focused: bool) -> SlotInfo {
        SlotInfo { owner_sid, open, minimized, focused, zone: 0 }
    }

    fn due_of(table: &SeenTable, slots: &[SlotInfo]) -> Vec<Due> {
        let mut out = [Due { slot: 0, owner_sid: 0, seen: Seen::Gone }; MAX_APP_WINDOWS];
        let n = table.due(slots, &mut out);
        out[..n].to_vec()
    }

    #[test]
    fn the_four_sightings_follow_the_slot_facts() {
        assert_eq!(seen_of(&slot(7, true, false, true)), Some(Seen::Focused));
        assert_eq!(seen_of(&slot(7, true, false, false)), Some(Seen::Visible));
        assert_eq!(seen_of(&slot(7, true, true, true)), Some(Seen::Hidden), "minimized hides");
        assert_eq!(seen_of(&slot(7, false, false, false)), Some(Seen::Gone));
        assert_eq!(seen_of(&slot(0, true, false, true)), None, "no identity, nobody to tell");
    }

    #[test]
    fn only_changes_are_due_and_a_failed_send_stays_due() {
        let mut table = SeenTable::default();
        let slots = [slot(7, true, false, true), slot(0, false, false, false)];
        let due = due_of(&table, &slots);
        assert_eq!(due, vec![Due { slot: 0, owner_sid: 7, seen: Seen::Focused }]);
        // Not committed (the send failed): still due.
        assert_eq!(due_of(&table, &slots).len(), 1);
        table.commit(due[0]);
        assert!(due_of(&table, &slots).is_empty(), "taken: nothing to say");
        // Minimized: one new sighting.
        let slots = [slot(7, true, true, false), slot(0, false, false, false)];
        assert_eq!(due_of(&table, &slots), vec![Due { slot: 0, owner_sid: 7, seen: Seen::Hidden }]);
    }

    #[test]
    fn a_closed_window_is_gone_once_and_a_reused_slot_reports_its_new_owner() {
        let mut table = SeenTable::default();
        let open = [slot(7, true, false, true)];
        for d in due_of(&table, &open) {
            table.commit(d);
        }
        // The slot is free again (surface gone, identity cleared): Gone for owner 7, once.
        let free = [slot(0, false, false, false)];
        let due = due_of(&table, &free);
        assert_eq!(due, vec![Due { slot: 0, owner_sid: 7, seen: Seen::Gone }]);
        table.commit(due[0]);
        assert!(due_of(&table, &free).is_empty());
        // Another app takes the slot: its own sighting is due although the state byte repeats.
        let reused = [slot(9, true, false, true)];
        assert_eq!(
            due_of(&table, &reused),
            vec![Due { slot: 0, owner_sid: 9, seen: Seen::Focused }]
        );
    }

    #[test]
    fn the_frame_matches_the_authority_wire() {
        let f = encode_seen(0x0102_0304_0506_0708, Seen::Hidden);
        assert_eq!(&f[..4], &[b'A', b'M', 1, 6]);
        assert_eq!(f[4], 0x08, "little-endian sid");
        assert_eq!(f[11], 0x01);
        assert_eq!(f[12], 2);
    }
}
