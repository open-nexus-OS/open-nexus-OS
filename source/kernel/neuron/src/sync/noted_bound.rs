// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a lower bound of pending deadlines kept by two parties under no common lock
//! (TASK-0330 P4): a waiter lowers it as it starts to wait, a walk over every waiter sets it
//! exact. Before, the walk stored its result over the one word both wrote: a waiter that
//! started between the walk's look at it and that store was in no bound, and its deadline was
//! delivered by the next walk for another waiter's. The bound is two words now — `noted`,
//! lowered by waiters and cleared when a walk begins, and `walked`, the last walk's result —
//! read as their minimum under the walks' sequence: a read that overlaps a walk is made again
//! after it.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below (host): every interleaving of a waiter, two walks and a reader
//! INVARIANTS: walks are serial (the caller's rule: the deadline walk has one walker at a
//!   time); a waiter lowers inside the hold of the lock a walk's look at it takes, the hold
//!   that makes its wait visible; a walk counts every waiter it sees, preparing or blocked; a
//!   reader holds no lock a walk takes.
//!   Then a read never returns more than the deadline of a waiter that lowered before the read
//!   began and still waits: its note is cleared only by a walk that began after it lowered —
//!   which therefore saw it — and that walk's result is the first a stable read can return.
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use core::sync::atomic::{AtomicU64, Ordering};

/// The sequence of the walks over one kind of waiter: odd while one runs.
pub struct Walks(AtomicU64);

impl Walks {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// A walk begins: reads wait until [`Self::end`]. Before the first [`Bound::clear`].
    pub fn begin(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    /// The walk is over. After the last [`Bound::publish`].
    pub fn end(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    /// What `read` returns with no walk running beside it: a read that overlaps one is made
    /// again after it. The reader holds no lock a walk takes, so the walk ends; it waits as it
    /// would for the walk's lock, and gets a value it can keep — a placeholder ("due now") read
    /// by a hart about to sleep became its armed deadline's shadow, and the deadline it had
    /// armed was delivered at its next tick.
    pub fn stable(&self, read: impl Fn() -> u64) -> u64 {
        loop {
            if let Some(seen) = self.open() {
                let value = read();
                if self.kept(seen) {
                    return value;
                }
            }
            core::hint::spin_loop();
        }
    }

    /// A read begins: the sequence, `None` inside a walk.
    fn open(&self) -> Option<u64> {
        let seen = self.0.load(Ordering::SeqCst);
        (seen & 1 == 0).then_some(seen)
    }

    /// Whether no walk began since [`Self::open`] returned `seen`.
    fn kept(&self, seen: u64) -> bool {
        self.0.load(Ordering::SeqCst) == seen
    }
}

/// One bound: of a hart, or of all.
pub struct Bound {
    noted: AtomicU64,
    walked: AtomicU64,
    scratch: AtomicU64,
}

impl Bound {
    pub const fn new() -> Self {
        Self {
            noted: AtomicU64::new(u64::MAX),
            walked: AtomicU64::new(u64::MAX),
            scratch: AtomicU64::new(u64::MAX),
        }
    }

    /// A waiter's deadline `ns`, as it starts to wait (the module's invariant names the hold).
    pub fn lower(&self, ns: u64) {
        self.noted.fetch_min(ns, Ordering::SeqCst);
    }

    /// A walk begins ([`Walks::begin`] first): what waiters note from here on is kept.
    pub fn clear(&self) {
        self.scratch.store(u64::MAX, Ordering::SeqCst);
        self.noted.store(u64::MAX, Ordering::SeqCst);
    }

    /// The walk saw a waiter with deadline `ns` still pending.
    pub fn note(&self, ns: u64) {
        self.scratch.fetch_min(ns, Ordering::SeqCst);
    }

    /// The walk's result becomes the bound ([`Walks::end`] after).
    pub fn publish(&self) {
        self.walked.store(self.scratch.load(Ordering::SeqCst), Ordering::SeqCst);
    }

    /// The bound, inside [`Walks::stable`] (`u64::MAX`: no deadline pending).
    pub fn get(&self) -> u64 {
        self.noted.load(Ordering::SeqCst).min(self.walked.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::{Bound, Walks};
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicU64, Ordering};

    const DEADLINE: u64 = 700;

    /// One step of a party; a party's steps run in order, the parties in any order.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Step {
        /// The waiter's hold: its wait becomes visible and it lowers the bound.
        Wait,
        WalkBegin,
        WalkClear,
        /// The walk's look at the waiter, under the waiter's lock.
        WalkLook,
        WalkPublish,
        WalkEnd,
        ReadSeq,
        ReadBound,
        ReadCheck,
    }

    const WALK: [Step; 5] =
        [Step::WalkBegin, Step::WalkClear, Step::WalkLook, Step::WalkPublish, Step::WalkEnd];

    struct World {
        walks: Walks,
        bound: Bound,
        waiting: bool,
        read: Option<u64>,
        seq_seen: Option<u64>,
        value_seen: u64,
    }

    impl World {
        fn new() -> Self {
            Self {
                walks: Walks::new(),
                bound: Bound::new(),
                waiting: false,
                read: None,
                seq_seen: None,
                value_seen: 0,
            }
        }

        fn run(&mut self, step: Step) {
            match step {
                Step::Wait => {
                    self.waiting = true;
                    self.bound.lower(DEADLINE);
                }
                Step::WalkBegin => self.walks.begin(),
                Step::WalkClear => self.bound.clear(),
                Step::WalkLook => {
                    if self.waiting {
                        self.bound.note(DEADLINE);
                    }
                }
                Step::WalkPublish => self.bound.publish(),
                Step::WalkEnd => self.walks.end(),
                // The reader is `Walks::stable` cut at its three loads.
                Step::ReadSeq => self.seq_seen = self.walks.open(),
                Step::ReadBound => self.value_seen = self.bound.get(),
                // A read beside a walk has no result: the reader makes it again.
                Step::ReadCheck => {
                    let kept = self.seq_seen.is_some_and(|seen| self.walks.kept(seen));
                    self.read = kept.then_some(self.value_seen);
                }
            }
        }
    }

    /// Every order of the parties' steps (each party's own order kept).
    fn interleavings(
        parties: &[&[Step]],
        prefix: &mut Vec<Step>,
        at: &mut [usize],
        out: &mut Vec<Vec<Step>>,
    ) {
        let mut done = true;
        for p in 0..parties.len() {
            if at[p] < parties[p].len() {
                done = false;
                prefix.push(parties[p][at[p]]);
                at[p] += 1;
                interleavings(parties, prefix, at, out);
                at[p] -= 1;
                prefix.pop();
            }
        }
        if done {
            out.push(prefix.clone());
        }
    }

    fn all(parties: &[&[Step]]) -> Vec<Vec<Step>> {
        let mut out = Vec::new();
        interleavings(parties, &mut Vec::new(), &mut alloc::vec![0; parties.len()], &mut out);
        out
    }

    /// A waiter starts beside two walks, a reader reads beside all of it: a read that began
    /// after the waiter lowered is never above its deadline — whichever walk looked at the
    /// waiter before it waited, cleared its note, or published over it.
    #[test]
    fn a_read_after_a_waiter_lowered_is_never_above_its_deadline() {
        let two_walks: Vec<Step> = WALK.iter().chain(WALK.iter()).copied().collect();
        let reader = [Step::ReadSeq, Step::ReadBound, Step::ReadCheck];
        let orders = all(&[&[Step::Wait], &two_walks, &reader]);
        assert!(orders.len() > 1_000);
        let mut results = 0;
        for order in orders {
            let waited = order.iter().position(|&s| s == Step::Wait);
            let began = order.iter().position(|&s| s == Step::ReadSeq);
            let mut world = World::new();
            for &step in &order {
                world.run(step);
            }
            if let Some(read) = world.read.filter(|_| waited < began) {
                assert!(read <= DEADLINE, "{order:?} read {read}");
                results += 1;
            }
        }
        // Before the walks, between them and after: every place a read has a result.
        assert!(results >= 18, "{results}");
    }

    /// The old shape — one word, the walk's result stored over it — loses the waiter that
    /// starts inside a walk; the two-word bound keeps it in the same order.
    #[test]
    fn test_reject_one_word_bound_loses_a_waiter_that_starts_inside_a_walk() {
        let word = AtomicU64::new(u64::MAX);
        // The walk looked (nobody waits), the waiter lowers, the walk stores its result.
        let looked = u64::MAX;
        word.fetch_min(DEADLINE, Ordering::SeqCst);
        word.store(looked, Ordering::SeqCst);
        assert!(word.load(Ordering::SeqCst) > DEADLINE);
        let (walks, bound) = (Walks::new(), Bound::new());
        walks.begin();
        bound.clear();
        bound.lower(DEADLINE);
        bound.publish();
        walks.end();
        assert_eq!(walks.stable(|| bound.get()), DEADLINE);
    }

    /// A read inside a walk has no result, and one a walk began or ended over is not kept;
    /// after the walk it is exact — a bound nobody waits on is none.
    #[test]
    fn test_reject_a_read_beside_a_walk_and_a_walk_with_no_waiter_clears_the_bound() {
        let (walks, bound) = (Walks::new(), Bound::new());
        bound.lower(DEADLINE);
        assert_eq!(walks.stable(|| bound.get()), DEADLINE);
        let before = walks.open().unwrap();
        walks.begin();
        bound.clear();
        assert_eq!(walks.open(), None);
        assert!(!walks.kept(before));
        bound.publish();
        walks.end();
        assert!(!walks.kept(before));
        assert_eq!(walks.stable(|| bound.get()), u64::MAX);
    }
}
