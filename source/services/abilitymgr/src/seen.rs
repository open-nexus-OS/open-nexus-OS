// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The lifecycle POLICY of TASK-0333 6d (RFC-0111 §4, ADR-0083), pure and
//! host-tested: the compositor states what it sees of an app's window (`OP_SEEN`: focused,
//! visible, hidden, gone); this module turns that into the instance's lifecycle state and into
//! the ORDERS the launcher carries out — a hidden app is frozen once its grace has run out, a
//! frozen app that is seen again (or whose window is gone and must run to exit) is thawed. An
//! instance whose kill was carried (6f-3: `dying`) is moved by its death report alone — its
//! window closing first must not thaw a process the parent already reaped. The OS loop owns
//! the clock and the IPC; nothing here waits or sends.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (the decision table, the grace, the sender gate, the frame)

use alloc::vec;
use alloc::vec::Vec;

use crate::lifecycle::{AbilityInstance, AbilityState, Broker, LifecycleError};
use crate::protocol::*;

/// What the compositor sees of an app's window — the `seen` byte of `OP_SEEN`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// On screen and focused.
    Focused = 0,
    /// On screen, not focused.
    Visible = 1,
    /// Minimized: no pixel of it is on screen.
    Hidden = 2,
    /// The window closed; the app has nothing on screen any more.
    Gone = 3,
}

impl Seen {
    /// Decodes the wire byte, or `None` for a value this build does not know (fail-closed).
    pub fn from_wire(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Focused,
            1 => Self::Visible,
            2 => Self::Hidden,
            3 => Self::Gone,
            _ => return None,
        })
    }
}

/// The grace a hidden app gets before it is frozen (RFC-0111 §4): long enough to finish what
/// the hide interrupted (a write, a reply in flight), short enough that a hidden app never
/// costs the foreground a frame. The phones this OS is measured against give seconds, not
/// minutes; and a thaw is one order away, so a short grace loses nothing.
pub const FREEZE_GRACE_NS: u64 = 5_000_000_000;

/// An order for the launcher (execd), the process's parent: the only party the kernel lets
/// freeze, thaw or kill it (ADR-0082).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// Stop the process's CPU: `task_suspend`.
    Freeze { instance_id: u32, pid: u32 },
    /// Give it back: `task_resume`.
    Thaw { instance_id: u32, pid: u32 },
}

/// What one `OP_SEEN` frame came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeenOutcome {
    /// Not the 13-byte frame, or a `seen` byte this build does not know.
    Malformed,
    /// No live instance runs as that kernel service id (a window of a process this service
    /// never launched, or one whose death was already reported).
    Unknown { owner_sid: u64 },
    /// Applied: the instance's state now, and the order the launcher must carry out, if any.
    Applied { instance_id: u32, state: AbilityState, order: Option<Order> },
}

/// Whether `frame` may be taken from `sender_service_id`: `OP_SEEN` only from the compositor
/// (its kernel sender id, never a payload string); every other op passes here.
pub fn seen_frame_allowed(frame: &[u8], sender_service_id: u64, windowd_sid: u64) -> bool {
    frame.get(3) != Some(&OP_SEEN) || sender_service_id == windowd_sid
}

/// Decodes and applies one `OP_SEEN` frame; `sid_of(app_id)` names the kernel service id the
/// launcher gave the app's process (`app:<bundle_id>`), the key the compositor reports by.
pub fn handle_seen(
    broker: &mut Broker,
    frame: &[u8],
    now_ns: u64,
    sid_of: impl Fn(&str) -> u64,
) -> SeenOutcome {
    // [A,M,ver,OP_SEEN, owner_sid:u64le, seen:u8]
    if frame.len() != 13 {
        return SeenOutcome::Malformed;
    }
    let mut sid = [0u8; 8];
    sid.copy_from_slice(&frame[4..12]);
    let owner_sid = u64::from_le_bytes(sid);
    let Some(seen) = Seen::from_wire(frame[12]) else { return SeenOutcome::Malformed };
    let Some(id) = broker.instance_id_by_sid(owner_sid, sid_of) else {
        return SeenOutcome::Unknown { owner_sid };
    };
    match broker.seen(id, seen, now_ns) {
        Ok((state, order)) => SeenOutcome::Applied { instance_id: id, state, order },
        Err(_) => SeenOutcome::Unknown { owner_sid },
    }
}

/// Encodes an `OP_SEEN` frame (the compositor's side, and tests).
pub fn encode_seen(owner_sid: u64, seen: Seen) -> Vec<u8> {
    let mut out = vec![MAGIC0, MAGIC1, VERSION, OP_SEEN];
    out.extend_from_slice(&owner_sid.to_le_bytes());
    out.push(seen as u8);
    out
}

impl Broker {
    /// The live instance whose process runs as kernel service id `sid`.
    pub fn instance_id_by_sid(&self, sid: u64, sid_of: impl Fn(&str) -> u64) -> Option<u32> {
        self.instances.values().find(|i| sid_of(&i.app_id) == sid).map(|i| i.instance_id)
    }

    /// The decision table: what the compositor sees → the instance's state and the launcher's
    /// order. A window on screen is `Foreground` (focused) or `Background` (visible); a hidden
    /// window leaves the instance in the background with the grace clock running; a frozen
    /// instance seen again is thawed into the state it is seen in; a frozen instance whose
    /// window is gone is thawed too — it must run to exit, and the death report ends it.
    pub fn seen(
        &mut self,
        id: u32,
        seen: Seen,
        now_ns: u64,
    ) -> Result<(AbilityState, Option<Order>), LifecycleError> {
        use AbilityState::*;
        let inst = self.instances.get(&id).ok_or(LifecycleError::UnknownInstance)?;
        if inst.dying {
            // The kill was carried: the window closes before the death report arrives.
            return Ok((inst.state, None));
        }
        let pid = inst.pid;
        if inst.state == Created {
            // Its window is on screen: the ability started.
            self.transition(id, Started)?;
        }
        let from = self.state(id).ok_or(LifecycleError::UnknownInstance)?;
        let thaw = pid.map(|pid| Order::Thaw { instance_id: id, pid });
        let (to, order) = match (seen, from) {
            (Seen::Focused, Suspended) => (Some(Foreground), thaw),
            (Seen::Focused, Started | Background) => (Some(Foreground), None),
            (Seen::Visible | Seen::Gone, Suspended) => (Some(Background), thaw),
            (Seen::Visible | Seen::Hidden, Started | Foreground) => (Some(Background), None),
            _ => (None, None),
        };
        if let Some(to) = to {
            self.transition(id, to)?;
        }
        let inst = self.instances.get_mut(&id).ok_or(LifecycleError::UnknownInstance)?;
        inst.hidden_since = match (seen, inst.state) {
            (Seen::Hidden, Background) => Some(inst.hidden_since.unwrap_or(now_ns)),
            _ => None,
        };
        Ok((inst.state, order))
    }

    /// RFC-0111 §6: an instance the permission authority granted `nexus.permission.BACKGROUND`
    /// (a non-empty `background = [...]` in its manifest) is never frozen for being hidden;
    /// every other hidden instance is (deny-by-default).
    fn freezes_when_hidden(inst: &AbilityInstance) -> bool {
        !inst.granted_caps.iter().any(|c| c == nexus_wire::policyd::PERMISSION_BACKGROUND)
    }

    /// The instances hidden past their grace at `now_ns`: each goes to `Suspended`, and its
    /// process (if it has one) is to be frozen. The grace is consumed: a thaw starts a new one.
    pub fn due_freezes(&mut self, now_ns: u64) -> Vec<Order> {
        let due: Vec<u32> = self
            .instances
            .values()
            .filter(|i| {
                i.state == AbilityState::Background
                    && !i.dying
                    && Self::freezes_when_hidden(i)
                    && i.hidden_since.is_some_and(|t| now_ns.saturating_sub(t) >= FREEZE_GRACE_NS)
            })
            .map(|i| i.instance_id)
            .collect();
        let mut orders = Vec::with_capacity(due.len());
        for id in due {
            if self.transition(id, AbilityState::Suspended).is_err() {
                continue;
            }
            if let Some(inst) = self.instances.get_mut(&id) {
                inst.hidden_since = None;
                if let Some(pid) = inst.pid {
                    orders.push(Order::Freeze { instance_id: id, pid });
                }
            }
        }
        orders
    }

    /// The earliest moment a hidden instance's grace runs out (`None`: nothing is hidden) —
    /// the deadline the OS loop arms its one-shot timer at.
    pub fn next_freeze_deadline(&self) -> Option<u64> {
        self.instances
            .values()
            .filter(|i| {
                i.state == AbilityState::Background && !i.dying && Self::freezes_when_hidden(i)
            })
            .filter_map(|i| i.hidden_since)
            .map(|t| t.saturating_add(FREEZE_GRACE_NS))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    /// A stand-in for the kernel's service-id derivation: distinct per app id.
    fn sid_of(app: &str) -> u64 {
        0x1000 + app.bytes().map(u64::from).sum::<u64>()
    }

    fn launched(broker: &mut Broker, app: &str, pid: u32) -> u32 {
        let id = broker.launch(app, "main").expect("launch");
        broker.bind_pid(id, pid).expect("bind");
        id
    }

    #[test]
    fn test_reject_background_undeclared() {
        // RFC-0111 §6: hidden past the grace, an app WITHOUT the background grant is frozen
        // (deny-by-default) and one WITH it is left running — its clock never arms the timer.
        let mut broker = Broker::new();
        let plain = launched(&mut broker, "notes", 41);
        let bg = broker
            .launch_with_caps(
                "player",
                "main",
                &["nexus.permission.WINDOW", nexus_wire::policyd::PERMISSION_BACKGROUND],
            )
            .expect("launch");
        broker.bind_pid(bg, 42).expect("bind");
        broker.seen(plain, Seen::Hidden, 100).expect("seen");
        broker.seen(bg, Seen::Hidden, 100).expect("seen");
        assert_eq!(broker.next_freeze_deadline(), Some(100 + FREEZE_GRACE_NS));
        assert_eq!(
            broker.due_freezes(100 + FREEZE_GRACE_NS),
            [Order::Freeze { instance_id: plain, pid: 41 }]
        );
        assert_eq!(broker.state(bg), Some(AbilityState::Background), "declared: still running");
        assert_eq!(broker.next_freeze_deadline(), None, "the declared app arms no clock");
        assert!(broker.due_freezes(u64::MAX).is_empty());
    }

    #[test]
    fn a_focused_window_takes_a_created_instance_to_foreground_without_an_order() {
        let mut broker = Broker::new();
        let id = launched(&mut broker, "notes", 70);
        let out = handle_seen(&mut broker, &encode_seen(sid_of("notes"), Seen::Focused), 1, sid_of);
        assert_eq!(
            out,
            SeenOutcome::Applied { instance_id: id, state: AbilityState::Foreground, order: None }
        );
        assert_eq!(broker.next_freeze_deadline(), None);
    }

    #[test]
    fn hidden_runs_the_grace_and_the_due_freeze_names_the_process() {
        let mut broker = Broker::new();
        let id = launched(&mut broker, "notes", 70);
        broker.seen(id, Seen::Focused, 1).unwrap();
        let (state, order) = broker.seen(id, Seen::Hidden, 100).unwrap();
        assert_eq!((state, order), (AbilityState::Background, None));
        assert_eq!(broker.next_freeze_deadline(), Some(100 + FREEZE_GRACE_NS));
        // A repeated hidden report does not restart the grace.
        broker.seen(id, Seen::Hidden, 200).unwrap();
        assert_eq!(broker.next_freeze_deadline(), Some(100 + FREEZE_GRACE_NS));
        assert!(broker.due_freezes(100 + FREEZE_GRACE_NS - 1).is_empty(), "not yet");
        assert_eq!(
            broker.due_freezes(100 + FREEZE_GRACE_NS),
            vec![Order::Freeze { instance_id: id, pid: 70 }]
        );
        assert_eq!(broker.state(id), Some(AbilityState::Suspended));
        assert_eq!(broker.next_freeze_deadline(), None, "the grace is consumed");
        assert!(broker.due_freezes(u64::MAX).is_empty(), "frozen once");
    }

    #[test]
    fn seen_again_before_the_grace_cancels_the_freeze() {
        let mut broker = Broker::new();
        let id = launched(&mut broker, "notes", 70);
        broker.seen(id, Seen::Focused, 1).unwrap();
        broker.seen(id, Seen::Hidden, 100).unwrap();
        let (state, order) = broker.seen(id, Seen::Visible, 200).unwrap();
        assert_eq!((state, order), (AbilityState::Background, None));
        assert_eq!(broker.next_freeze_deadline(), None);
        assert!(broker.due_freezes(u64::MAX).is_empty());
    }

    #[test]
    fn a_frozen_instance_seen_again_is_thawed_into_what_it_is_seen_as() {
        let mut broker = Broker::new();
        let id = launched(&mut broker, "notes", 70);
        broker.seen(id, Seen::Focused, 1).unwrap();
        broker.seen(id, Seen::Hidden, 100).unwrap();
        broker.due_freezes(100 + FREEZE_GRACE_NS);
        let thaw = Some(Order::Thaw { instance_id: id, pid: 70 });
        assert_eq!(
            broker.seen(id, Seen::Focused, 7_000).unwrap(),
            (AbilityState::Foreground, thaw)
        );
        // Frozen again, then seen visible (not focused): Background, thawed.
        broker.seen(id, Seen::Hidden, 8_000).unwrap();
        broker.due_freezes(8_000 + FREEZE_GRACE_NS);
        assert_eq!(
            broker.seen(id, Seen::Visible, 20_000).unwrap(),
            (AbilityState::Background, thaw)
        );
        // A thawed, visible instance has no grace running.
        assert_eq!(broker.next_freeze_deadline(), None);
    }

    #[test]
    fn a_frozen_instance_whose_window_is_gone_is_thawed_so_it_can_exit() {
        let mut broker = Broker::new();
        let id = launched(&mut broker, "notes", 70);
        broker.seen(id, Seen::Focused, 1).unwrap();
        broker.seen(id, Seen::Hidden, 100).unwrap();
        broker.due_freezes(100 + FREEZE_GRACE_NS);
        assert_eq!(
            broker.seen(id, Seen::Gone, 9_000).unwrap(),
            (AbilityState::Background, Some(Order::Thaw { instance_id: id, pid: 70 }))
        );
        // A running instance whose window is gone needs no order: its exit is the report.
        assert_eq!(broker.seen(id, Seen::Gone, 9_001).unwrap(), (AbilityState::Background, None));
    }

    #[test]
    fn test_reject_thaw_of_a_dying_instance_whose_window_closes_first() {
        // TASK-0333 6f-3 (lowmem lane): memd's kill was carried; windowd reports the window gone
        // before execd's death report — no thaw for a process the parent already reaped.
        let mut broker = Broker::new();
        let id = launched(&mut broker, "notes", 70);
        broker.seen(id, Seen::Focused, 1).unwrap();
        broker.seen(id, Seen::Hidden, 100).unwrap();
        broker.due_freezes(100 + FREEZE_GRACE_NS);
        broker.mark_dying(id).unwrap();
        assert_eq!(broker.seen(id, Seen::Gone, 9_000).unwrap(), (AbilityState::Suspended, None));
        assert_eq!(broker.seen(id, Seen::Focused, 9_001).unwrap(), (AbilityState::Suspended, None));
        // The death report ends it.
        assert_eq!(broker.exited(70).map(|i| i.instance_id), Ok(id));
        assert_eq!(broker.state(id), None);
    }

    #[test]
    fn test_reject_freeze_of_an_instance_killed_in_its_grace() {
        let mut broker = Broker::new();
        let id = launched(&mut broker, "notes", 70);
        broker.seen(id, Seen::Focused, 1).unwrap();
        broker.seen(id, Seen::Hidden, 100).unwrap();
        broker.mark_dying(id).unwrap();
        assert_eq!(broker.next_freeze_deadline(), None, "no clock for a dying instance");
        assert!(broker.due_freezes(u64::MAX).is_empty(), "never frozen after its kill");
        assert_eq!(broker.state(id), Some(AbilityState::Background));
    }

    #[test]
    fn a_window_only_instance_moves_state_but_orders_nothing() {
        let mut broker = Broker::new();
        let id = broker.launch("chat", "main").unwrap();
        broker.seen(id, Seen::Focused, 1).unwrap();
        broker.seen(id, Seen::Hidden, 100).unwrap();
        assert!(broker.due_freezes(100 + FREEZE_GRACE_NS).is_empty(), "no process, no order");
        assert_eq!(broker.state(id), Some(AbilityState::Suspended));
        assert_eq!(
            broker.seen(id, Seen::Focused, 10_000).unwrap(),
            (AbilityState::Foreground, None)
        );
    }

    #[test]
    fn test_reject_seen_from_anyone_but_the_compositor() {
        let frame = encode_seen(0x42, Seen::Hidden);
        let (windowd, other) = (0x1111u64, 0x2222u64);
        assert!(seen_frame_allowed(&frame, windowd, windowd));
        assert!(!seen_frame_allowed(&frame, other, windowd), "a forged sighting is refused");
        assert!(seen_frame_allowed(&crate::wire::encode_launch("chat", "main"), other, windowd));
    }

    #[test]
    fn test_reject_malformed_and_unknown_sightings() {
        let mut broker = Broker::new();
        launched(&mut broker, "notes", 70);
        let good = encode_seen(sid_of("notes"), Seen::Focused);
        assert_eq!(handle_seen(&mut broker, &good[..12], 1, sid_of), SeenOutcome::Malformed);
        let mut bad_seen = good.clone();
        bad_seen[12] = 9;
        assert_eq!(handle_seen(&mut broker, &bad_seen, 1, sid_of), SeenOutcome::Malformed);
        let stranger = encode_seen(sid_of("stranger"), Seen::Focused);
        assert_eq!(
            handle_seen(&mut broker, &stranger, 1, sid_of),
            SeenOutcome::Unknown { owner_sid: sid_of("stranger") }
        );
        assert_eq!(broker.state(1), Some(AbilityState::Created), "nothing moved");
        let _ = String::new();
    }

    #[test]
    fn the_seen_frame_round_trips() {
        let frame = encode_seen(0x0102_0304_0506_0708, Seen::Gone);
        assert_eq!(frame.len(), 13);
        assert_eq!(&frame[..4], &[MAGIC0, MAGIC1, VERSION, OP_SEEN]);
        assert_eq!(frame[12], 3);
        for s in [Seen::Focused, Seen::Visible, Seen::Hidden, Seen::Gone] {
            assert_eq!(Seen::from_wire(s as u8), Some(s));
        }
        assert_eq!(Seen::from_wire(4), None);
    }
}
