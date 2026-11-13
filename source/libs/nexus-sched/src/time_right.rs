// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Time as a right (RFC-0120) — the pure core: a time right is a share of the harts'
//! time held in a derivation tree, a reservation is the part of a right one thread runs on one
//! hart with (a budget per period, earliest deadline first). A right is derived only as a subset
//! of what its parent still holds, revoked in one step (a generation; its descendants are found
//! dead when next touched, never walked), admitted against a hart's capacity when a thread
//! binds it, and moved between harts whole — a migration is admitted on the target, keeps the
//! source reserved until the migrant's deadline and never raises the migrant's density.
//! No allocation, no clock, no lock: the caller passes the time and owns the exclusion.
//! OWNERS: @kernel-team
//! PUBLIC API: TimeRights, RightId, ReservationId, TimeRightError, Charged, PPM and the bounds
//! DEPENDS_ON: core
//! INVARIANTS: every share is accounted exactly once (`audit`); a hart's reservations never
//!   exceed its capacity; a reservation never runs past its budget in a period
//! TEST_COVERAGE: tests/time_right.rs (test_reject_* per refusal, conservation over 10 000 steps)

/// One hart's whole time, the unit shares are stated in (parts per million).
pub const PPM: u32 = 1_000_000;
/// The smallest share a right or a reservation may be: a thousandth of a hart.
pub const SHARE_MIN_PPM: u32 = 1_000;
/// How deep a right may be derived (the root is 0).
pub const DEPTH_MAX: u8 = 8;
/// A reservation's period lies in `[PERIOD_MIN_NS, PERIOD_MAX_NS]`.
pub const PERIOD_MIN_NS: u64 = 1_000_000;
pub const PERIOD_MAX_NS: u64 = 1_000_000_000;
/// The smallest budget a reservation may have per period (below it a switch costs more).
pub const BUDGET_MIN_NS: u64 = 50_000;
/// Shares a hart still holds for reservations that left it, per hart (merged beyond it).
const LEAVING: usize = 4;

/// A time right: a slot of the tree and the generation it was made in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RightId {
    slot: u16,
    gen: u32,
}

/// A reservation: a slot and the generation it was made in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReservationId {
    slot: u16,
    gen: u32,
}

/// Why an operation on a time right is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeRightError {
    /// The right, or one it was derived from, was revoked — or the id is of another generation.
    Revoked,
    /// More than the right still holds itself.
    BeyondHeld,
    /// The hart's reservations would exceed its capacity.
    BeyondCapacity,
    /// Derived deeper than `DEPTH_MAX`.
    TooDeep,
    /// A share below `SHARE_MIN_PPM`, or a reservation above one hart.
    Share,
    /// A period outside its bounds, or a budget below `BUDGET_MIN_NS`.
    Period,
    /// No free slot.
    Full,
    /// The root is not revoked.
    Root,
    /// No such hart, or the hart it is on already.
    Hart,
    /// The reservation's budget of this period is spent.
    Exhausted,
    /// A replenish before the deadline.
    Early,
}

/// What a charge left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Charged {
    /// The time taken from the budget (at most what was asked).
    pub ran_ns: u64,
    /// What is left of this period's budget.
    pub left_ns: u64,
}

#[derive(Clone, Copy)]
struct Node {
    live: bool,
    gen: u32,
    parent: Option<RightId>,
    depth: u8,
    /// The share it was given, and what of it is neither derived away nor bound.
    total: u32,
    own: u32,
}

#[derive(Clone, Copy)]
struct Reservation {
    live: bool,
    gen: u32,
    right: RightId,
    share: u32,
    period_ns: u64,
    hart: u16,
    deadline_ns: u64,
    left_ns: u64,
}

#[derive(Clone, Copy)]
struct Hart {
    /// Shares still held for reservations that left: `(until, share)`.
    leaving: [(u64, u32); LEAVING],
}

const NO_NODE: Node = Node { live: false, gen: 0, parent: None, depth: 0, total: 0, own: 0 };
const NO_RESERVATION: Reservation = Reservation {
    live: false,
    gen: 0,
    right: RightId { slot: 0, gen: 0 },
    share: 0,
    period_ns: 0,
    hart: 0,
    deadline_ns: 0,
    left_ns: 0,
};

/// The time rights of a system of `H` harts: `R` rights, `V` reservations.
pub struct TimeRights<const R: usize, const V: usize, const H: usize> {
    nodes: [Node; R],
    reservations: [Reservation; V],
    harts: [Hart; H],
    /// What a hart's reservations may sum to (the rest is the fair class's floor).
    capacity: u32,
}

impl<const R: usize, const V: usize, const H: usize> TimeRights<R, V, H> {
    /// A system whose every hart gives `capacity_ppm` of its time to reservations; the root
    /// right holds all of it (`H × capacity_ppm`).
    #[must_use]
    pub fn new(capacity_ppm: u32) -> Self {
        let capacity = capacity_ppm.min(PPM);
        let total = capacity.saturating_mul(H as u32);
        let mut nodes = [NO_NODE; R];
        if let Some(root) = nodes.first_mut() {
            *root = Node { live: true, gen: 1, parent: None, depth: 0, total, own: total };
        }
        Self {
            nodes,
            reservations: [NO_RESERVATION; V],
            harts: [Hart { leaving: [(0, 0); LEAVING] }; H],
            capacity,
        }
    }

    /// The root right.
    #[must_use]
    pub fn root(&self) -> RightId {
        RightId { slot: 0, gen: 1 }
    }

    /// The node of a right that is alive: its own generation and every ancestor's still stand.
    /// At most `DEPTH_MAX + 1` steps.
    fn alive(&self, id: RightId) -> Option<usize> {
        let mut at = id;
        for _ in 0..=DEPTH_MAX {
            let node =
                self.nodes.get(usize::from(at.slot)).filter(|n| n.live && n.gen == at.gen)?;
            match node.parent {
                None => return Some(usize::from(id.slot)),
                Some(parent) => at = parent,
            }
        }
        None
    }

    /// What a right was given and what of it it still holds itself: `(total, own)` in ppm.
    #[must_use]
    pub fn held(&self, id: RightId) -> Option<(u32, u32)> {
        self.alive(id).map(|n| (self.nodes[n].total, self.nodes[n].own))
    }

    /// A free slot of the tree: an unused one, else one whose right is dead.
    fn free_node(&self) -> Option<usize> {
        self.nodes.iter().position(|n| !n.live).or_else(|| {
            (1..R)
                .find(|&k| self.alive(RightId { slot: k as u16, gen: self.nodes[k].gen }).is_none())
        })
    }

    /// Derives a right of `share_ppm` from `parent`: a subset of what the parent still holds.
    pub fn derive(&mut self, parent: RightId, share_ppm: u32) -> Result<RightId, TimeRightError> {
        let p = self.alive(parent).ok_or(TimeRightError::Revoked)?;
        if share_ppm < SHARE_MIN_PPM {
            return Err(TimeRightError::Share);
        }
        if share_ppm > self.nodes[p].own {
            return Err(TimeRightError::BeyondHeld);
        }
        let depth = self.nodes[p].depth + 1;
        if depth > DEPTH_MAX {
            return Err(TimeRightError::TooDeep);
        }
        let slot = self.free_node().ok_or(TimeRightError::Full)?;
        let gen = self.nodes[slot].gen.wrapping_add(1);
        self.nodes[p].own -= share_ppm;
        self.nodes[slot] =
            Node { live: true, gen, parent: Some(parent), depth, total: share_ppm, own: share_ppm };
        Ok(RightId { slot: slot as u16, gen })
    }

    /// Revokes a right and everything derived from it, in one step: its share is its parent's
    /// again and its generation moves on — a descendant and a reservation are found dead when
    /// next touched. A reservation of it keeps its hart reserved until its deadline.
    pub fn revoke(&mut self, id: RightId) -> Result<(), TimeRightError> {
        let n = self.alive(id).ok_or(TimeRightError::Revoked)?;
        let parent = self.nodes[n].parent.ok_or(TimeRightError::Root)?;
        let total = self.nodes[n].total;
        self.nodes[n].live = false;
        self.nodes[n].gen = self.nodes[n].gen.wrapping_add(1);
        self.nodes[usize::from(parent.slot)].own += total;
        Ok(())
    }

    /// What hart `hart` holds reserved at `now`: its reservations (a dead one until its
    /// deadline) and the shares of those that left, until theirs.
    #[must_use]
    pub fn load(&self, hart: usize, now: u64) -> u32 {
        let Some(h) = self.harts.get(hart) else { return 0 };
        let bound = self
            .reservations
            .iter()
            .filter(|r| r.live && usize::from(r.hart) == hart)
            .filter(|r| self.alive(r.right).is_some() || r.deadline_ns > now)
            .map(|r| r.share);
        let leaving = h.leaving.iter().filter(|(until, _)| *until > now).map(|(_, share)| *share);
        bound.chain(leaving).fold(0u32, u32::saturating_add)
    }

    fn admit(&self, hart: usize, share: u32, now: u64) -> Result<(), TimeRightError> {
        if hart >= H {
            return Err(TimeRightError::Hart);
        }
        if self.load(hart, now).saturating_add(share) > self.capacity {
            return Err(TimeRightError::BeyondCapacity);
        }
        Ok(())
    }

    fn budget(share: u32, period_ns: u64) -> u64 {
        (u128::from(share) * u128::from(period_ns) / u128::from(PPM)) as u64
    }

    /// Binds `share_ppm` of `right` to a thread on `hart`: a reservation with a budget of
    /// `share × period` per period, its first deadline one period from `now`.
    pub fn bind(
        &mut self,
        right: RightId,
        share_ppm: u32,
        period_ns: u64,
        hart: usize,
        now: u64,
    ) -> Result<ReservationId, TimeRightError> {
        let n = self.alive(right).ok_or(TimeRightError::Revoked)?;
        if !(SHARE_MIN_PPM..=PPM).contains(&share_ppm) {
            return Err(TimeRightError::Share);
        }
        let budget = Self::budget(share_ppm, period_ns);
        if !(PERIOD_MIN_NS..=PERIOD_MAX_NS).contains(&period_ns) || budget < BUDGET_MIN_NS {
            return Err(TimeRightError::Period);
        }
        if share_ppm > self.nodes[n].own {
            return Err(TimeRightError::BeyondHeld);
        }
        self.admit(hart, share_ppm, now)?;
        let slot = self.free_reservation(now).ok_or(TimeRightError::Full)?;
        let gen = self.reservations[slot].gen.wrapping_add(1);
        self.nodes[n].own -= share_ppm;
        self.reservations[slot] = Reservation {
            live: true,
            gen,
            right,
            share: share_ppm,
            period_ns,
            hart: hart as u16,
            deadline_ns: now.saturating_add(period_ns),
            left_ns: budget,
        };
        Ok(ReservationId { slot: slot as u16, gen })
    }

    /// A free reservation slot: an unused one, else a dead one past its deadline.
    fn free_reservation(&self, now: u64) -> Option<usize> {
        self.reservations.iter().position(|r| !r.live).or_else(|| {
            self.reservations
                .iter()
                .position(|r| self.alive(r.right).is_none() && r.deadline_ns <= now)
        })
    }

    fn reservation(&self, id: ReservationId) -> Result<usize, TimeRightError> {
        self.reservations
            .get(usize::from(id.slot))
            .filter(|r| r.live && r.gen == id.gen)
            .map(|_| usize::from(id.slot))
            .ok_or(TimeRightError::Revoked)
    }

    /// The reservation's slot while its right stands; a reservation of a revoked right is
    /// dropped here (its hart stays reserved until its deadline) and refused.
    fn standing(&mut self, id: ReservationId, now: u64) -> Result<usize, TimeRightError> {
        let slot = self.reservation(id)?;
        if self.alive(self.reservations[slot].right).is_some() {
            return Ok(slot);
        }
        self.leave(slot, now);
        Err(TimeRightError::Revoked)
    }

    /// Frees reservation `slot`; its hart keeps its share reserved until its deadline — what it
    /// ran ahead of its share in this period is not handed out twice.
    fn leave(&mut self, slot: usize, now: u64) {
        let r = self.reservations[slot];
        self.reservations[slot].live = false;
        if r.deadline_ns <= now {
            return;
        }
        let leaving = &mut self.harts[usize::from(r.hart)].leaving;
        if let Some(free) = leaving.iter_mut().find(|(until, _)| *until <= now) {
            *free = (r.deadline_ns, r.share);
            return;
        }
        // No free entry: merge into the one that ends last (held longer, never shorter).
        if let Some(last) = leaving.iter_mut().max_by_key(|(until, _)| *until) {
            *last = (last.0.max(r.deadline_ns), last.1.saturating_add(r.share));
        }
    }

    /// Ends a reservation: its share is its right's again.
    pub fn unbind(&mut self, id: ReservationId, now: u64) -> Result<(), TimeRightError> {
        let slot = self.standing(id, now)?;
        let r = self.reservations[slot];
        if let Some(n) = self.alive(r.right) {
            self.nodes[n].own += r.share;
        }
        self.leave(slot, now);
        Ok(())
    }

    /// Moves a reservation to hart `to` (a migration, or an idle hart's steal): admitted on the
    /// target like a bind; the source stays reserved until the migrant's deadline; and a
    /// migrant whose remaining budget is denser than its share over what is left of its period
    /// starts a new period at `now` instead — the target's residents never pay for it.
    pub fn migrate(
        &mut self,
        id: ReservationId,
        to: usize,
        now: u64,
    ) -> Result<(), TimeRightError> {
        let slot = self.standing(id, now)?;
        let mut r = self.reservations[slot];
        if usize::from(r.hart) == to {
            return Err(TimeRightError::Hart);
        }
        self.admit(to, r.share, now)?;
        self.leave(slot, now);
        let window = r.deadline_ns.saturating_sub(now);
        if u128::from(r.left_ns) * u128::from(PPM) > u128::from(r.share) * u128::from(window) {
            r.deadline_ns = now.saturating_add(r.period_ns);
            r.left_ns = Self::budget(r.share, r.period_ns);
        }
        r.hart = to as u16;
        self.reservations[slot] = r;
        Ok(())
    }

    /// Takes `ns` of run time from the reservation's budget of this period (at most what is
    /// left). Refused when the budget is spent — the thread runs in the fair class until its
    /// deadline — and when the right was revoked.
    pub fn charge(
        &mut self,
        id: ReservationId,
        ns: u64,
        now: u64,
    ) -> Result<Charged, TimeRightError> {
        let slot = self.standing(id, now)?;
        let r = &mut self.reservations[slot];
        if r.left_ns == 0 {
            return Err(TimeRightError::Exhausted);
        }
        let ran_ns = ns.min(r.left_ns);
        r.left_ns -= ran_ns;
        Ok(Charged { ran_ns, left_ns: r.left_ns })
    }

    /// Starts the reservation's next period at or after its deadline: a full budget, the
    /// deadline moved on by whole periods past `now`; what was unused is gone.
    pub fn replenish(&mut self, id: ReservationId, now: u64) -> Result<u64, TimeRightError> {
        let slot = self.standing(id, now)?;
        let r = &mut self.reservations[slot];
        if now < r.deadline_ns {
            return Err(TimeRightError::Early);
        }
        let periods = (now - r.deadline_ns) / r.period_ns + 1;
        r.deadline_ns = r.deadline_ns.saturating_add(periods.saturating_mul(r.period_ns));
        r.left_ns = Self::budget(r.share, r.period_ns);
        Ok(r.deadline_ns)
    }

    /// A reservation's state: `(hart, deadline, left, budget)`.
    #[must_use]
    pub fn state(&self, id: ReservationId) -> Option<(usize, u64, u64, u64)> {
        let slot = self.reservation(id).ok()?;
        let r = &self.reservations[slot];
        self.alive(r.right)?;
        Some((usize::from(r.hart), r.deadline_ns, r.left_ns, Self::budget(r.share, r.period_ns)))
    }

    /// Checks the books at `now`: every share of the root is held exactly once — by a right
    /// itself, or bound in a reservation of a standing right — each right's parts add up to
    /// what it was given, and no hart is reserved past its capacity. `false`: an invariant broke.
    #[must_use]
    pub fn audit(&self, now: u64) -> bool {
        let standing = |k: usize| {
            self.nodes[k].live
                && self.alive(RightId { slot: k as u16, gen: self.nodes[k].gen }).is_some()
        };
        let mut all: u64 = 0;
        for k in (0..R).filter(|&k| standing(k)) {
            let id = RightId { slot: k as u16, gen: self.nodes[k].gen };
            let derived: u64 = (0..R)
                .filter(|&c| standing(c) && self.nodes[c].parent == Some(id))
                .map(|c| u64::from(self.nodes[c].total))
                .sum();
            let bound: u64 = self
                .reservations
                .iter()
                .filter(|r| r.live && r.right == id)
                .map(|r| u64::from(r.share))
                .sum();
            let own = u64::from(self.nodes[k].own);
            if own + derived + bound != u64::from(self.nodes[k].total) {
                return false;
            }
            all += own + bound;
        }
        let budgets =
            self.reservations.iter().filter(|r| r.live).all(|r| {
                r.left_ns <= Self::budget(r.share, r.period_ns) && usize::from(r.hart) < H
            });
        let root = self.nodes.first().map_or(0, |n| u64::from(n.total));
        budgets && all == root && (0..H).all(|h| self.load(h, now) <= self.capacity)
    }
}
