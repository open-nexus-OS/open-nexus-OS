// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What each app process is to the user, as a BAND for the memory authority
//! (TASK-0333 6e-2, RFC-0100 §3, ADR-0086). The lifecycle state and the compositor's last
//! sighting decide: the focused window's process (and one still starting) is `foreground`;
//! on screen but not focused is `visible`; hidden with `nexus.permission.BACKGROUND` is
//! `declared`; hidden in its grace is `hidden`; frozen is `suspended`; a dead process is
//! `gone`. The feed is a DIFF: after every change the broker went through, the bands that
//! differ from what memd last heard are pushed, and a push memd could not take is retried
//! on the next pass (the feed commits only what was sent). Every push names the process's
//! OWNER (TASK-0352 P2, ADR-0093): the app's kernel service id, `app:<bundle id>` as execd
//! names the process — the key gpud charges the app's surfaces by, so memd joins a charge to
//! the processes it is for. Pure, host-tested.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (the wire is `nexus_wire::memd`)
//! TEST_COVERAGE: tests below (the mapping table, the diff, the retry, the owner)

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use nexus_wire::memd::{
    BAND_DECLARED, BAND_FOREGROUND, BAND_GONE, BAND_HIDDEN, BAND_SUSPENDED, BAND_VISIBLE,
};

use crate::lifecycle::{AbilityInstance, AbilityState, Broker};

/// The band of one live instance; `None` for one without a process (a window-only launch:
/// nothing a kill could take) or one already stopped.
#[must_use]
pub fn band_of(inst: &AbilityInstance) -> Option<(u32, u8)> {
    let pid = inst.pid?;
    let band = match inst.state {
        AbilityState::Created | AbilityState::Started | AbilityState::Foreground => BAND_FOREGROUND,
        AbilityState::Background if inst.hidden_since.is_none() => BAND_VISIBLE,
        AbilityState::Background if runs_hidden(inst) => BAND_DECLARED,
        AbilityState::Background => BAND_HIDDEN,
        AbilityState::Suspended => BAND_SUSPENDED,
        AbilityState::Stopped => return None,
    };
    Some((pid, band))
}

/// The kernel service id execd gives an app's process (`app:<bundle_id>`, execd's `identity`):
/// the key the compositor reports a window by and gpud charges the app's surfaces to.
#[must_use]
pub fn app_sid(app_id: &str) -> u64 {
    let mut name = [0u8; 64];
    let app = app_id.as_bytes();
    let n = app.len().min(name.len() - 4);
    name[..4].copy_from_slice(b"app:");
    name[4..4 + n].copy_from_slice(&app[..n]);
    nexus_abi::service_id_from_name(&name[..4 + n])
}

/// RFC-0111 §6: the one grant that lets an app run while hidden.
fn runs_hidden(inst: &AbilityInstance) -> bool {
    inst.granted_caps.iter().any(|c| c == nexus_wire::policyd::PERMISSION_BACKGROUND)
}

/// One push due: the process, its band, its owner (the app's kernel service id).
pub type Push = (u32, u8, u64);

/// What memd last heard, per process: the band and the owner it was pushed with.
#[derive(Debug, Default)]
pub struct BandFeed {
    last: BTreeMap<u32, (u8, u64)>,
}

impl BandFeed {
    /// A feed that has told memd nothing yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { last: BTreeMap::new() }
    }

    /// The pushes due: every process whose band differs from what memd last heard, and
    /// `BAND_GONE` (with the owner memd heard) for every process memd heard of that the broker
    /// no longer holds.
    #[must_use]
    pub fn changes(&self, broker: &Broker) -> Vec<Push> {
        let now: BTreeMap<u32, (u8, u64)> = broker
            .instances()
            .filter_map(|inst| {
                band_of(inst).map(|(pid, band)| (pid, (band, app_sid(&inst.app_id))))
            })
            .collect();
        let mut due: Vec<Push> = now
            .iter()
            .filter(|(pid, (band, _))| self.last.get(*pid).map(|(b, _)| b) != Some(band))
            .map(|(pid, (band, owner))| (*pid, *band, *owner))
            .collect();
        due.extend(
            self.last
                .iter()
                .filter(|(pid, _)| !now.contains_key(pid))
                .map(|(pid, (_, owner))| (*pid, BAND_GONE, *owner)),
        );
        due
    }

    /// memd took the push: remember it (a `gone` forgets the process).
    pub fn commit(&mut self, pid: u32, band: u8, owner: u64) {
        if band == BAND_GONE {
            self.last.remove(&pid);
        } else {
            self.last.insert(pid, (band, owner));
        }
    }

    /// Processes memd has heard of.
    #[must_use]
    pub fn len(&self) -> usize {
        self.last.len()
    }

    /// `true` when memd has heard of no process.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.last.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seen::Seen;

    fn launched(broker: &mut Broker, app: &str, pid: u32, caps: &[&str]) -> u32 {
        let id = broker.launch_with_caps(app, "main", caps).expect("launch");
        broker.bind_pid(id, pid).expect("bind");
        id
    }

    #[test]
    fn the_mapping_table() {
        let mut broker = Broker::new();
        let fg = launched(&mut broker, "notes", 41, &[]);
        let vis = launched(&mut broker, "mail", 42, &[]);
        let hid = launched(&mut broker, "chat", 43, &[]);
        let dec = launched(
            &mut broker,
            "player",
            44,
            &["nexus.permission.WINDOW", nexus_wire::policyd::PERMISSION_BACKGROUND],
        );
        let frozen = launched(&mut broker, "maps", 45, &[]);
        let no_pid = broker.launch("search", "main").expect("window-only launch");
        // Starting: foreground (never a victim).
        assert_eq!(band_of(broker.instance(fg).unwrap()), Some((41, BAND_FOREGROUND)));
        assert_eq!(band_of(broker.instance(no_pid).unwrap()), None, "no process, no band");
        broker.seen(fg, Seen::Focused, 1).expect("seen");
        broker.seen(vis, Seen::Visible, 1).expect("seen");
        broker.seen(hid, Seen::Hidden, 1).expect("seen");
        broker.seen(dec, Seen::Hidden, 1).expect("seen");
        broker.seen(frozen, Seen::Hidden, 1).expect("seen");
        let _ = broker.due_freezes(1 + crate::seen::FREEZE_GRACE_NS);
        let band = |id| band_of(broker.instance(id).unwrap()).map(|(_, b)| b);
        assert_eq!(band(fg), Some(BAND_FOREGROUND));
        assert_eq!(band(vis), Some(BAND_VISIBLE));
        assert_eq!(band(hid), Some(BAND_SUSPENDED), "past the grace without the grant: frozen");
        assert_eq!(band(dec), Some(BAND_DECLARED), "the grant: hidden, running, declared");
        assert_eq!(band(frozen), Some(BAND_SUSPENDED));
        // Before the grace ran out a plain hidden app is `hidden`.
        let mut b2 = Broker::new();
        let h = launched(&mut b2, "chat", 50, &[]);
        b2.seen(h, Seen::Hidden, 1).expect("seen");
        assert_eq!(band_of(b2.instance(h).unwrap()), Some((50, BAND_HIDDEN)));
    }

    #[test]
    fn the_feed_pushes_differences_and_a_death_as_gone() {
        let mut broker = Broker::new();
        let a = launched(&mut broker, "notes", 41, &[]);
        let b = launched(&mut broker, "mail", 42, &[]);
        let (notes, mail) = (app_sid("notes"), app_sid("mail"));
        assert_ne!(notes, mail);
        assert_eq!(notes, nexus_abi::service_id_from_name(b"app:notes"), "execd's name");
        let mut feed = BandFeed::new();
        let mut due = feed.changes(&broker);
        due.sort_unstable();
        assert_eq!(due, [(41, BAND_FOREGROUND, notes), (42, BAND_FOREGROUND, mail)]);
        for (pid, band, owner) in due {
            feed.commit(pid, band, owner);
        }
        assert!(feed.changes(&broker).is_empty(), "nothing changed, nothing due");
        broker.seen(a, Seen::Focused, 1).expect("seen");
        broker.seen(b, Seen::Hidden, 1).expect("seen");
        assert_eq!(feed.changes(&broker), [(42, BAND_HIDDEN, mail)], "only the changed one");
        feed.commit(42, BAND_HIDDEN, mail);
        broker.exited(42).expect("death");
        assert_eq!(feed.changes(&broker), [(42, BAND_GONE, mail)], "gone with its owner");
        feed.commit(42, BAND_GONE, mail);
        assert_eq!(feed.len(), 1);
        assert!(feed.changes(&broker).is_empty());
    }

    #[test]
    fn a_push_memd_could_not_take_stays_due() {
        let mut broker = Broker::new();
        let a = launched(&mut broker, "notes", 41, &[]);
        let notes = app_sid("notes");
        let mut feed = BandFeed::new();
        assert_eq!(feed.changes(&broker), [(41, BAND_FOREGROUND, notes)]);
        // Not committed (the send failed): still due, and a later change is what is due then.
        broker.seen(a, Seen::Hidden, 1).expect("seen");
        assert_eq!(feed.changes(&broker), [(41, BAND_HIDDEN, notes)]);
        feed.commit(41, BAND_HIDDEN, notes);
        assert!(feed.changes(&broker).is_empty());
    }
}
