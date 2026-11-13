// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Time as a right (RFC-0120, TASK-0363 P1): the contract on the host model before kernel code —
//! a right is derived only as a subset, a reservation is admitted against its hart's capacity,
//! nothing of a revoked right runs, a migration never over-commits a hart nor lets a thread run
//! past its share, and the books balance through 10 000 random steps.

use nexus_sched::time_right::{
    BUDGET_MIN_NS, DEPTH_MAX, PERIOD_MAX_NS, PERIOD_MIN_NS, PPM, SHARE_MIN_PPM,
};
use nexus_sched::{ReservationId, RightId, TimeRightError as E, TimeRights};

/// Four harts, each giving 80 % of its time to reservations.
type Model = TimeRights<48, 48, 4>;
const CAPACITY: u32 = 800_000;
const MS: u64 = 1_000_000;

fn model() -> Model {
    TimeRights::new(CAPACITY)
}

#[test]
fn the_root_holds_every_harts_capacity() {
    let m = model();
    assert_eq!(m.held(m.root()), Some((4 * CAPACITY, 4 * CAPACITY)));
    assert!(m.audit(0));
}

#[test]
fn a_derived_right_is_taken_from_what_its_parent_holds() {
    let mut m = model();
    let display = m.derive(m.root(), 1_200_000).unwrap();
    let apps = m.derive(display, 400_000).unwrap();
    assert_eq!(m.held(m.root()), Some((3_200_000, 2_000_000)));
    assert_eq!(m.held(display), Some((1_200_000, 800_000)));
    assert_eq!(m.held(apps), Some((400_000, 400_000)));
    assert!(m.audit(0));
}

#[test]
fn test_reject_delegate_beyond_held() {
    let mut m = model();
    let display = m.derive(m.root(), 600_000).unwrap();
    let app = m.derive(display, 500_000).unwrap();
    // The display path holds 100 000 itself now: it cannot hand out what it gave away.
    assert_eq!(m.derive(display, 100_001), Err(E::BeyondHeld));
    assert_eq!(m.derive(app, 500_001), Err(E::BeyondHeld));
    // Nor bind it to a thread.
    assert_eq!(m.bind(display, 100_001, 10 * MS, 0, 0), Err(E::BeyondHeld));
    // The root cannot mint: it holds the harts' capacity and no more.
    assert_eq!(m.derive(m.root(), 4 * CAPACITY), Err(E::BeyondHeld));
    assert_eq!(m.held(display), Some((600_000, 100_000)), "a refusal changes nothing");
    assert!(m.audit(0));
}

#[test]
fn test_reject_a_share_below_the_floor_and_a_tree_too_deep() {
    let mut m = model();
    assert_eq!(m.derive(m.root(), SHARE_MIN_PPM - 1), Err(E::Share));
    assert_eq!(m.derive(m.root(), 0), Err(E::Share));
    let mut at = m.root();
    for _ in 0..DEPTH_MAX {
        at = m.derive(at, 100_000).unwrap();
    }
    assert_eq!(m.derive(at, 50_000), Err(E::TooDeep));
    assert!(m.audit(0));
}

#[test]
fn test_reject_admit_beyond_capacity() {
    let mut m = model();
    let right = m.derive(m.root(), 2_000_000).unwrap();
    m.bind(right, 500_000, 10 * MS, 0, 0).unwrap();
    m.bind(right, 300_000, 10 * MS, 0, 0).unwrap();
    // Hart 0 is at its 80 %: the right holds 1.2 harts more, and still none of it fits here.
    assert_eq!(m.bind(right, SHARE_MIN_PPM, 100 * MS, 0, 0), Err(E::BeyondCapacity));
    assert_eq!(m.load(0, 0), CAPACITY);
    // It fits on another hart; and one thread never holds more than one hart.
    m.bind(right, 700_000, 10 * MS, 1, 0).unwrap();
    assert_eq!(m.bind(right, PPM + 1, 10 * MS, 2, 0), Err(E::Share));
    assert_eq!(m.bind(right, 100_000, 10 * MS, 4, 0), Err(E::Hart));
    assert!(m.audit(0));
}

#[test]
fn test_reject_a_period_or_a_budget_out_of_bounds() {
    let mut m = model();
    let right = m.derive(m.root(), 800_000).unwrap();
    assert_eq!(m.bind(right, 100_000, PERIOD_MIN_NS - 1, 0, 0), Err(E::Period));
    assert_eq!(m.bind(right, 100_000, PERIOD_MAX_NS + 1, 0, 0), Err(E::Period));
    // 1 000 ppm of a 1 ms period is 1 µs of budget: a switch costs more.
    assert!(u64::from(SHARE_MIN_PPM) * PERIOD_MIN_NS / u64::from(PPM) < BUDGET_MIN_NS);
    assert_eq!(m.bind(right, SHARE_MIN_PPM, PERIOD_MIN_NS, 0, 0), Err(E::Period));
    assert!(m.audit(0));
}

#[test]
fn a_reservation_runs_its_budget_once_a_period() {
    let mut m = model();
    let right = m.derive(m.root(), 500_000).unwrap();
    // 25 % of a hart every 8 ms: 2 ms of budget.
    let r = m.bind(right, 250_000, 8 * MS, 2, 0).unwrap();
    assert_eq!(m.state(r), Some((2, 8 * MS, 2 * MS, 2 * MS)));
    assert_eq!(m.charge(r, 1_500_000, MS).map(|c| c.left_ns), Ok(500_000));
    // Asked for more than is left: it gets what is left, and then nothing until the deadline.
    let last = m.charge(r, 900_000, 3 * MS).unwrap();
    assert_eq!((last.ran_ns, last.left_ns), (500_000, 0));
    assert_eq!(m.charge(r, 1, 4 * MS), Err(E::Exhausted));
    assert_eq!(m.replenish(r, 7 * MS), Err(E::Early));
    assert_eq!(m.replenish(r, 8 * MS), Ok(16 * MS));
    assert_eq!(m.charge(r, MS, 9 * MS).map(|c| c.left_ns), Ok(MS));
    // Idle over three deadlines: one budget, never three.
    assert_eq!(m.replenish(r, 41 * MS), Ok(48 * MS));
    assert_eq!(m.state(r).map(|s| s.2), Some(2 * MS));
    assert!(m.audit(41 * MS));
}

#[test]
fn test_reject_run_after_revoke() {
    let mut m = model();
    let display = m.derive(m.root(), 1_000_000).unwrap();
    let app = m.derive(display, 400_000).unwrap();
    let widget = m.derive(app, 100_000).unwrap();
    let r_app = m.bind(app, 200_000, 10 * MS, 0, 0).unwrap();
    let r_widget = m.bind(widget, 100_000, 10 * MS, 1, 0).unwrap();
    m.charge(r_app, MS, MS).unwrap();
    // One step: the display path has its share back; nothing below `app` was walked.
    m.revoke(app).unwrap();
    assert_eq!(m.held(display), Some((1_000_000, 1_000_000)));
    assert_eq!(m.held(app), None);
    assert_eq!(m.held(widget), None, "a right derived from a revoked right is revoked");
    // Nothing of it runs, is replenished, moved or derived from — on first touch.
    assert_eq!(m.charge(r_app, 1, 2 * MS), Err(E::Revoked));
    assert_eq!(m.charge(r_widget, 1, 2 * MS), Err(E::Revoked));
    assert_eq!(m.replenish(r_app, 10 * MS), Err(E::Revoked));
    assert_eq!(m.migrate(r_widget, 2, 2 * MS), Err(E::Revoked));
    assert_eq!(m.derive(widget, 10_000), Err(E::Revoked));
    assert_eq!(m.bind(app, 10_000, 10 * MS, 3, 2 * MS), Err(E::Revoked));
    assert_eq!(m.revoke(app), Err(E::Revoked), "once");
    assert_eq!(m.revoke(m.root()), Err(E::Root));
    // A revoked reservation holds its hart until its deadline — what it ran ahead is not handed
    // out twice — and no longer.
    assert_eq!(m.load(0, 9 * MS), 200_000);
    assert_eq!(m.load(0, 10 * MS), 0);
    assert!(m.audit(2 * MS) && m.audit(10 * MS));
}

#[test]
fn a_slot_of_a_revoked_right_is_reused_and_an_old_id_stays_dead() {
    let mut m: TimeRights<3, 2, 1> = TimeRights::new(CAPACITY);
    let a = m.derive(m.root(), 400_000).unwrap();
    let b = m.derive(a, 200_000).unwrap();
    assert_eq!(m.derive(m.root(), 100_000), Err(E::Full));
    m.revoke(a).unwrap();
    // Both slots are free again: `a`'s own, and `b`'s, whose right died with `a`.
    let c = m.derive(m.root(), 300_000).unwrap();
    let d = m.derive(c, 100_000).unwrap();
    assert_eq!(m.held(a), None);
    assert_eq!(m.held(b), None, "an id of an earlier generation never names the new right");
    assert_eq!(m.held(d), Some((100_000, 100_000)));
    assert!(m.audit(0));
}

#[test]
fn test_reject_migrate_beyond_the_targets_capacity() {
    let mut m = model();
    let right = m.derive(m.root(), 2_400_000).unwrap();
    let mover = m.bind(right, 300_000, 10 * MS, 0, 0).unwrap();
    m.bind(right, 600_000, 10 * MS, 1, 0).unwrap();
    assert_eq!(m.migrate(mover, 1, MS), Err(E::BeyondCapacity));
    assert_eq!(m.migrate(mover, 0, MS), Err(E::Hart));
    assert_eq!(m.state(mover).map(|s| s.0), Some(0), "a refused move leaves it where it was");
    // Admitted on hart 2: the source stays reserved until the migrant's deadline.
    m.migrate(mover, 2, MS).unwrap();
    assert_eq!((m.load(0, MS), m.load(2, MS)), (300_000, 300_000));
    assert_eq!(m.bind(right, 600_000, 10 * MS, 0, MS), Err(E::BeyondCapacity));
    assert!(m.audit(MS));
    m.bind(right, 600_000, 10 * MS, 0, 10 * MS).unwrap();
    assert!(m.audit(10 * MS));
}

#[test]
fn a_migrant_never_arrives_denser_than_its_share() {
    let mut m = model();
    let right = m.derive(m.root(), 1_000_000).unwrap();
    // 40 % every 10 ms: 4 ms of budget. It ran 1 ms and is moved at 9 ms: 3 ms left for the
    // last millisecond would be three harts' worth on the target — it starts a new period.
    let late = m.bind(right, 400_000, 10 * MS, 0, 0).unwrap();
    m.charge(late, MS, MS).unwrap();
    m.migrate(late, 1, 9 * MS).unwrap();
    assert_eq!(m.state(late), Some((1, 19 * MS, 4 * MS, 4 * MS)));
    // One that kept pace keeps its period: 2 ms left for the last 5 ms is its 40 %.
    let paced = m.bind(right, 400_000, 10 * MS, 2, 0).unwrap();
    m.charge(paced, 2 * MS, 5 * MS).unwrap();
    m.migrate(paced, 3, 5 * MS).unwrap();
    assert_eq!(m.state(paced), Some((3, 10 * MS, 2 * MS, 4 * MS)));
    assert!(m.audit(9 * MS));
}

#[test]
fn an_unbound_share_is_the_rights_again() {
    let mut m = model();
    let right = m.derive(m.root(), 500_000).unwrap();
    let r = m.bind(right, 500_000, 10 * MS, 3, 0).unwrap();
    assert_eq!(m.held(right), Some((500_000, 0)));
    m.unbind(r, 4 * MS).unwrap();
    assert_eq!(m.held(right), Some((500_000, 500_000)));
    assert_eq!(m.unbind(r, 4 * MS), Err(E::Revoked), "the id is spent");
    assert_eq!(m.charge(r, 1, 4 * MS), Err(E::Revoked));
    assert_eq!(m.load(3, 4 * MS), 500_000);
    assert_eq!(m.load(3, 10 * MS), 0);
    assert!(m.audit(4 * MS));
}

/// xorshift64*: the same steps on every run.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// A reservation the walk follows: when it was bound and what it ran since.
struct Followed {
    id: ReservationId,
    share: u32,
    period: u64,
    bound_at: u64,
    ran: u64,
}

/// TRACK-TIME-AS-RESOURCE gate G3: 10 000 random derive, revoke, bind, unbind, migrate (a steal
/// is one), charge and replenish steps while time moves on. After every step the books balance
/// (`audit`: each share held once, no hart past its capacity), and no reservation has run more
/// than its share of the time since it was bound plus one budget — through every migration.
#[test]
fn conservation_over_10_000_random_steps() {
    let mut m = model();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut rights: Vec<RightId> = vec![m.root()];
    let mut res: Vec<Followed> = Vec::new();
    let mut now = 0u64;
    let (mut done, mut refused) = ([0u32; 7], [0u32; 7]);
    for _ in 0..10_000 {
        now += rng.below(3 * MS);
        let op = rng.below(7) as usize;
        let right = rights[rng.below(rights.len() as u64) as usize];
        let pick = (!res.is_empty()).then(|| rng.below(res.len() as u64) as usize);
        let ok = match (op, pick) {
            (0, _) => {
                m.derive(right, 1_000 + rng.below(900_000) as u32).map(|id| rights.push(id)).is_ok()
            }
            (1, _) => m.revoke(right).is_ok(),
            (2, _) => {
                let (share, period) =
                    (10_000 + rng.below(700_000) as u32, (2 + rng.below(40)) * MS);
                m.bind(right, share, period, rng.below(4) as usize, now)
                    .map(|id| res.push(Followed { id, share, period, bound_at: now, ran: 0 }))
                    .is_ok()
            }
            (3, Some(k)) => m.unbind(res[k].id, now).is_ok(),
            (4, Some(k)) => m.migrate(res[k].id, rng.below(4) as usize, now).is_ok(),
            (5, Some(k)) => {
                m.charge(res[k].id, rng.below(6 * MS), now).map(|c| res[k].ran += c.ran_ns).is_ok()
            }
            (6, Some(k)) => m.replenish(res[k].id, now).is_ok(),
            _ => false,
        };
        if ok {
            done[op] += 1
        } else {
            refused[op] += 1
        }
        assert!(m.audit(now), "the books at {now} after op {op}");
        res.retain(|f| m.state(f.id).is_some());
        rights.retain(|id| m.held(*id).is_some());
        for f in &res {
            let budget = u128::from(f.share) * u128::from(f.period) / u128::from(PPM);
            let fair = u128::from(f.share) * u128::from(now - f.bound_at) / u128::from(PPM);
            assert!(u128::from(f.ran) <= fair + budget, "ran {} of {fair} + {budget}", f.ran);
        }
    }
    // The walk exercised every operation both ways (a refusal is part of the contract).
    assert!(done.iter().all(|&n| n > 100), "done {done:?}");
    assert!(refused.iter().all(|&n| n > 0), "refused {refused:?}");
}
