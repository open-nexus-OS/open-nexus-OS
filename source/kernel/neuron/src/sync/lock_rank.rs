// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the lock classes of RFC-0102 and the rule that orders them (TASK-0330 P2) — pure,
//! so it runs on the host. A hart takes locks in ascending rank only; two locks of one class
//! are held together only where the class states an order (processes by pid — a parent before
//! its child —, IPC objects by address, run queues by CPU index), and nothing is taken under a
//! leaf. `Held` is what one hart holds right now; the ranked lock (`sync::ranked`) asks it
//! before every take, so no caller can skip the question.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::test_reject_lock_rank_inversion`,
//!   `test_reject_two_run_queues_descending`, `test_reject_two_endpoints_out_of_order`,
//!   `test_reject_lock_under_a_leaf`, the allowed orders, a release out of order)
//! INVARIANTS: a refusal names both locks; a `try` take cannot wait, so it is free of the order
//!   within its class and bound by the ranks like every take.
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

/// A lock class, by rank (RFC-0102 "Classes and ranks").
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Class {
    /// A table from an id to its object; held for the lookup only.
    Registry = 1,
    /// A process's capability table, thread list and lifecycle state.
    Process = 2,
    /// An address space's regions and page-table edits.
    Space = 3,
    /// A memory object's pages, commit state and seals.
    Object = 4,
    /// An endpoint's queues and waiters; a waitset; a fence; a timer.
    Ipc = 5,
    /// A thread's run state, block reason and wait links.
    Thread = 6,
    /// One hart's queues and its current task.
    RunQueue = 7,
    /// A frame magazine or bank, the kernel heap, the console ring.
    Leaf = 8,
}

impl Class {
    /// The number of classes.
    pub const COUNT: usize = 8;
    /// Every class, by rank.
    pub const ALL: [Class; Class::COUNT] = [
        Class::Registry,
        Class::Process,
        Class::Space,
        Class::Object,
        Class::Ipc,
        Class::Thread,
        Class::RunQueue,
        Class::Leaf,
    ];

    /// The class's name in a refusal and in the budget line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Class::Registry => "registry",
            Class::Process => "process",
            Class::Space => "space",
            Class::Object => "object",
            Class::Ipc => "ipc",
            Class::Thread => "thread",
            Class::RunQueue => "runq",
            Class::Leaf => "leaf",
        }
    }

    /// The class's place in a per-class table.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize - 1
    }

    /// Whether two locks of the class may be held together — in ascending key order.
    const fn pairs(self) -> bool {
        matches!(self, Class::Process | Class::Ipc | Class::RunQueue)
    }
}

/// The deepest nest a path may build: one lock per rank.
pub const MAX_HELD: usize = Class::COUNT;

/// Why a take is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A lower rank under a higher one.
    Inversion { held: Class, taking: Class },
    /// A second lock of a class that is never held twice, or one out of the class's order.
    Order { class: Class, held: usize, taking: usize },
    /// More locks than a path may nest.
    TooDeep,
}

/// What one hart holds: its locks in the order it took them.
#[derive(Clone, Copy, Debug)]
pub struct Held {
    len: u8,
    at: [(Class, usize); MAX_HELD],
}

impl Held {
    /// A hart that holds nothing.
    pub const EMPTY: Held = Held { len: 0, at: [(Class::Registry, 0); MAX_HELD] };

    fn held(&self) -> &[(Class, usize)] {
        &self.at[..self.len as usize]
    }

    /// Whether the hart holds no ranked lock — what a switch and a return to user mode require.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The highest class held.
    #[must_use]
    pub fn highest(&self) -> Option<Class> {
        self.held().iter().map(|&(class, _)| class).max()
    }

    /// May the hart wait for the lock `(class, key)`?
    pub fn check(&self, class: Class, key: usize) -> Result<(), Refusal> {
        self.check_rank(class)?;
        match self.held().iter().filter(|&&(held, _)| held == class).map(|&(_, k)| k).max() {
            Some(held) if !class.pairs() || key <= held => {
                Err(Refusal::Order { class, held, taking: key })
            }
            _ => Ok(()),
        }
    }

    /// May the hart try the lock `(class, key)` without waiting? A take that cannot wait cannot
    /// close a cycle, so a class that pairs is free of its order; the ranks bind it like every
    /// take, a class held once stays held once, and no lock is tried while it is held.
    pub fn check_try(&self, class: Class, key: usize) -> Result<(), Refusal> {
        self.check_rank(class)?;
        let same = |&&(held, k): &&(Class, usize)| held == class && (!class.pairs() || k == key);
        match self.held().iter().find(same) {
            Some(&(_, held)) => Err(Refusal::Order { class, held, taking: key }),
            None => Ok(()),
        }
    }

    fn check_rank(&self, class: Class) -> Result<(), Refusal> {
        if self.len as usize == MAX_HELD {
            return Err(Refusal::TooDeep);
        }
        match self.highest() {
            Some(held) if held > class => Err(Refusal::Inversion { held, taking: class }),
            _ => Ok(()),
        }
    }

    /// The hart took `(class, key)` — after a `check` that allowed it.
    pub fn push(&mut self, class: Class, key: usize) {
        if let Some(slot) = self.at.get_mut(self.len as usize) {
            *slot = (class, key);
            self.len += 1;
        }
    }

    /// The hart released `(class, key)` — in any order: the latest such entry leaves.
    pub fn pop(&mut self, class: Class, key: usize) {
        let len = self.len as usize;
        if let Some(found) = self.at[..len].iter().rposition(|&entry| entry == (class, key)) {
            self.at.copy_within(found + 1..len, found);
            self.len -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(held: &mut Held, class: Class, key: usize) -> Result<(), Refusal> {
        held.check(class, key)?;
        held.push(class, key);
        Ok(())
    }

    #[test]
    fn a_send_and_a_fault_nest_in_rank_order() {
        // The wake path: an endpoint, the receiver's thread, a run queue, a leaf.
        let mut held = Held::EMPTY;
        for class in [Class::Ipc, Class::Thread, Class::RunQueue, Class::Leaf] {
            assert_eq!(take(&mut held, class, 0x100), Ok(()));
        }
        assert_eq!(held.highest(), Some(Class::Leaf));
        // The fault path: the space, the object, a leaf for the frame.
        let mut held = Held::EMPTY;
        for class in [Class::Space, Class::Object, Class::Leaf] {
            assert_eq!(take(&mut held, class, 0x200), Ok(()));
        }
    }

    #[test]
    fn test_reject_lock_rank_inversion() {
        let mut held = Held::EMPTY;
        take(&mut held, Class::Thread, 7).unwrap();
        assert_eq!(
            held.check(Class::Ipc, 0x1000),
            Err(Refusal::Inversion { held: Class::Thread, taking: Class::Ipc })
        );
        // A take that cannot wait is bound by the ranks all the same.
        assert_eq!(
            held.check_try(Class::Process, 1),
            Err(Refusal::Inversion { held: Class::Thread, taking: Class::Process })
        );
        // The registry is released before the object it found is locked.
        let mut held = Held::EMPTY;
        take(&mut held, Class::Ipc, 0x1000).unwrap();
        assert_eq!(
            held.check(Class::Registry, 1),
            Err(Refusal::Inversion { held: Class::Ipc, taking: Class::Registry })
        );
    }

    #[test]
    fn test_reject_two_run_queues_descending() {
        let mut held = Held::EMPTY;
        take(&mut held, Class::RunQueue, 3).unwrap();
        assert_eq!(
            held.check(Class::RunQueue, 1),
            Err(Refusal::Order { class: Class::RunQueue, held: 3, taking: 1 })
        );
        assert!(held.check(Class::RunQueue, 3).is_err(), "the same queue twice");
        assert_eq!(take(&mut held, Class::RunQueue, 5), Ok(()));
        // A thief never waits: it may try a lower hart's queues, never its own again.
        assert_eq!(held.check_try(Class::RunQueue, 0), Ok(()));
        assert!(held.check_try(Class::RunQueue, 5).is_err());
    }

    #[test]
    fn test_reject_two_endpoints_out_of_order() {
        let mut held = Held::EMPTY;
        take(&mut held, Class::Ipc, 0x2000).unwrap();
        assert_eq!(
            held.check(Class::Ipc, 0x1000),
            Err(Refusal::Order { class: Class::Ipc, held: 0x2000, taking: 0x1000 })
        );
        assert_eq!(take(&mut held, Class::Ipc, 0x3000), Ok(()));
        // A parent before its child: pids only grow.
        let mut held = Held::EMPTY;
        take(&mut held, Class::Process, 4).unwrap();
        assert_eq!(take(&mut held, Class::Process, 9), Ok(()));
        assert!(held.check(Class::Process, 2).is_err());
    }

    #[test]
    fn test_reject_two_of_a_class_that_is_held_once() {
        for class in [Class::Registry, Class::Space, Class::Object, Class::Thread] {
            let mut held = Held::EMPTY;
            take(&mut held, class, 1).unwrap();
            assert_eq!(
                held.check(class, 2),
                Err(Refusal::Order { class, held: 1, taking: 2 }),
                "{}",
                class.name()
            );
        }
    }

    #[test]
    fn test_reject_lock_under_a_leaf() {
        let mut held = Held::EMPTY;
        take(&mut held, Class::Leaf, 1).unwrap();
        for class in Class::ALL {
            assert!(held.check(class, 2).is_err(), "{} under a leaf", class.name());
            assert!(held.check_try(class, 2).is_err(), "{} tried under a leaf", class.name());
        }
    }

    #[test]
    fn a_release_out_of_order_leaves_the_rest_held() {
        let mut held = Held::EMPTY;
        take(&mut held, Class::Ipc, 0x10).unwrap();
        take(&mut held, Class::Thread, 3).unwrap();
        take(&mut held, Class::RunQueue, 0).unwrap();
        held.pop(Class::Ipc, 0x10);
        assert_eq!(held.highest(), Some(Class::RunQueue));
        assert!(held.check(Class::Thread, 4).is_err(), "the run queue is still held");
        held.pop(Class::RunQueue, 0);
        held.pop(Class::Thread, 3);
        assert!(held.is_empty());
        assert_eq!(held.check(Class::Registry, 1), Ok(()));
    }

    #[test]
    fn test_reject_a_nest_deeper_than_the_ranks() {
        let mut held = Held::EMPTY;
        for (key, class) in Class::ALL.into_iter().enumerate() {
            take(&mut held, class, key).unwrap();
        }
        assert_eq!(held.check(Class::Leaf, 99), Err(Refusal::TooDeep));
    }
}
