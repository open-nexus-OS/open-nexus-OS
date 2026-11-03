// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0333 6f-3: the kill chain over the REAL pure state machines, in EVERY order its events
//! can arrive — memd's authority, the lifecycle authority's broker and its band feed, no
//! simulation of either. memd orders the kill; abilitymgr carries it (its `mem_kill` policy, the
//! victim marked `dying` — the OS loop's two lines); then the compositor's sighting `gone`, the
//! parent's death report and up to two stray sightings (focused, visible, hidden) arrive in any
//! interleaving, and after every one of them the band diff goes to memd. Whatever the order:
//! abilitymgr orders nothing for the dying process (no thaw, no freeze), memd hears no band for
//! it but `gone`, the kill is proven exactly once — at the `gone` — and nothing of the process is
//! left behind. Found on the lowmem lane: the window's `gone` before the death report thawed a
//! reaped process and pushed it as `visible`.

use abilitymgr::bands::BandFeed;
use abilitymgr::lifecycle::{AbilityState, Broker};
use abilitymgr::mem_kill::{encode_mem_kill, handle_mem_kill, MemKillOutcome};
use abilitymgr::seen::{Seen, FREEZE_GRACE_NS};
use memd::authority::{Authority, BandEvent, Decision};
use memd::order::{Orders, Squeezed, SwappedOut};
use nexus_abi::syscall::memory::MEM_PRESSURE_EMERGENCY;
use nexus_wire::memd::{BAND_GONE, BAND_SUSPENDED};

const PID: u32 = 117;

/// The OS loop's carrier, pure: the order judged by abilitymgr's policy and, once carried, the
/// victim marked dying (abilitymgr `orders.rs`: `order_execd(OP_KILL)` succeeded).
struct Carrier<'a> {
    broker: &'a mut Broker,
}

impl Orders for Carrier<'_> {
    fn kill(&mut self, pid: u32) -> bool {
        match handle_mem_kill(self.broker, &encode_mem_kill(pid)) {
            MemKillOutcome::Victim { instance_id, .. } => {
                self.broker.mark_dying(instance_id).expect("the victim is live");
                true
            }
            _ => false,
        }
    }

    fn squeeze(&mut self, _pid: u32) -> Option<Squeezed> {
        None
    }

    fn swap_ready(&mut self) -> bool {
        false
    }

    fn swap_out(&mut self, _pid: u32) -> Option<SwappedOut> {
        None
    }
}

/// What can happen at abilitymgr after the kill was carried.
#[derive(Clone, Copy, Debug)]
enum Event {
    Sighting(Seen),
    DeathReport,
}

/// Pushes every band due to memd; answers the kill proofs it produced and the bands it heard for
/// `PID`.
fn push_bands(
    feed: &mut BandFeed,
    broker: &Broker,
    memd: &mut Authority,
    heard: &mut Vec<u8>,
) -> usize {
    let mut proofs = 0;
    for (pid, band, owner) in feed.changes(broker) {
        if pid == PID {
            heard.push(band);
        }
        if let BandEvent::KillProven { pending, .. } = memd.on_band(pid, band, owner) {
            assert_eq!(pending.pid, PID);
            proofs += 1;
        }
        feed.commit(pid, band, owner);
    }
    proofs
}

/// Every interleaving of the death report, the sighting `gone` and `strays` (in their order).
fn orders(strays: &[Seen]) -> Vec<Vec<Event>> {
    let n = strays.len() + 2;
    let mut out = Vec::new();
    for gone_at in 0..n {
        for death_at in 0..n {
            if gone_at == death_at {
                continue;
            }
            let mut strays = strays.iter().copied();
            out.push(
                (0..n)
                    .map(|i| {
                        if i == gone_at {
                            Event::Sighting(Seen::Gone)
                        } else if i == death_at {
                            Event::DeathReport
                        } else {
                            Event::Sighting(strays.next().expect("a stray"))
                        }
                    })
                    .collect(),
            );
        }
    }
    out
}

fn run(events: &[Event]) {
    let mut broker = Broker::new();
    let mut feed = BandFeed::new();
    let mut memd = Authority::new(true);
    let mut heard = Vec::new();
    // Settings runs, is hidden, and frozen after the grace — memd hears each band.
    let id = broker.launch("settings", "main").expect("launch");
    broker.bind_pid(id, PID).expect("pid");
    broker.seen(id, Seen::Focused, 1).expect("focused");
    push_bands(&mut feed, &broker, &mut memd, &mut heard);
    broker.seen(id, Seen::Hidden, 100).expect("hidden");
    push_bands(&mut feed, &broker, &mut memd, &mut heard);
    assert_eq!(broker.due_freezes(100 + FREEZE_GRACE_NS).len(), 1, "frozen after the grace");
    push_bands(&mut feed, &broker, &mut memd, &mut heard);
    assert_eq!(heard.last(), Some(&BAND_SUSPENDED));
    heard.clear();

    // memd decides; abilitymgr carries the kill.
    let decision = memd.judge(
        MEM_PRESSURE_EMERGENCY,
        memd::authority::Source::Drill,
        &mut Carrier { broker: &mut broker },
    );
    assert_eq!(
        decision,
        Decision::Kill { pid: PID, band: BAND_SUSPENDED, ordered: true },
        "{events:?}"
    );

    let mut proofs = 0;
    let mut now = 10_000;
    for event in events {
        now += 1_000;
        match *event {
            Event::Sighting(seen) => {
                if let Ok((state, order)) = broker.seen(id, seen, now) {
                    assert_eq!(order, None, "no order for a dying process: {events:?}");
                    assert_eq!(
                        state,
                        AbilityState::Suspended,
                        "only the death report moves it: {events:?}"
                    );
                }
            }
            Event::DeathReport => {
                broker.exited(PID).expect("the death report names the live instance");
            }
        }
        assert!(broker.due_freezes(now + FREEZE_GRACE_NS).is_empty(), "no freeze: {events:?}");
        proofs += push_bands(&mut feed, &broker, &mut memd, &mut heard);
    }
    assert_eq!(proofs, 1, "proven exactly once: {events:?}");
    assert_eq!(heard, [BAND_GONE], "memd hears `gone` and nothing else: {events:?}");
    assert_eq!(broker.state(id), None, "the instance left the table: {events:?}");
    assert!(feed.is_empty(), "the feed forgot the process: {events:?}");
    // The kill is over: the next level judges afresh (no kill in flight).
    let mut carrier = Carrier { broker: &mut broker };
    assert_eq!(
        memd.judge(MEM_PRESSURE_EMERGENCY, memd::authority::Source::Drill, &mut carrier),
        Decision::NoVictim
    );
}

#[test]
fn the_kill_is_proven_once_in_every_order() {
    let kinds = [Seen::Focused, Seen::Visible, Seen::Hidden];
    let mut cases = 0;
    let mut stray_sets: Vec<Vec<Seen>> = vec![Vec::new()];
    stray_sets.extend(kinds.iter().map(|k| vec![*k]));
    for a in kinds {
        for b in kinds {
            stray_sets.push(vec![a, b]);
        }
    }
    for strays in &stray_sets {
        for events in orders(strays) {
            run(&events);
            cases += 1;
        }
    }
    // 2 orders without strays, 3 × 6 with one, 9 × 12 with two.
    assert_eq!(cases, 2 + 18 + 108);
}
