// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the pure rules of a thread's floating-point registers (RFC-0119) — the unit's
//! switch in a status word, what a hart does when a thread traps and when it returns to user
//! mode, and which instruction is the unit's. NOT target-gated (`mod trap` is riscv/none-only),
//! so the rules run on host against a model of harts and threads; `trap::fp` executes the
//! steps on the target.
//! A thread has no state until its first floating-point instruction. From then on its
//! registers are written to its state when it traps having changed them, and loaded before it
//! runs unless the hart still holds exactly that state: a state carries a token that is new
//! after every write, a hart remembers the token its registers hold.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below: the rules, `test_reject_*`, random schedules over harts and
//!   threads
//! INVARIANTS: a thread without a state never runs with the unit on; a token is unique across
//!   harts and never 0; a load happens exactly when the hart holds another token
//! RFC: docs/rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const UNIT_SHIFT: usize = 13;
const UNIT_MASK: usize = 3 << UNIT_SHIFT;

/// The floating-point unit's switch in a thread's status word (`sstatus.FS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// An instruction of the unit traps.
    Off,
    /// On, the registers as a new state has them.
    Initial,
    /// On, the registers equal the thread's state.
    Clean,
    /// On, the thread wrote a register since its state was written.
    Dirty,
}

impl Unit {
    /// The switch a status word carries.
    #[must_use]
    pub const fn of(status: usize) -> Self {
        match (status & UNIT_MASK) >> UNIT_SHIFT {
            0 => Self::Off,
            1 => Self::Initial,
            2 => Self::Clean,
            _ => Self::Dirty,
        }
    }

    /// `status` with this switch.
    #[must_use]
    pub const fn set_in(self, status: usize) -> usize {
        (status & !UNIT_MASK) | ((self as usize) << UNIT_SHIFT)
    }
}

/// What the kernel does with the registers after a rule decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing moves.
    Keep,
    /// The hart's registers are written to the thread's state.
    Save,
    /// The thread's state is loaded into the hart's registers; `from` is the CPU that wrote it.
    Load {
        /// The CPU whose save (or first use) made the state's token.
        from: usize,
    },
    /// The unit was on for a thread without a state: off, and counted.
    Refuse,
}

const LANE_SHIFT: u32 = 48;

/// One hart's view of the unit: the state of the task current on it, whose state its
/// registers hold, and its counters.
pub struct HartUnit {
    /// This CPU's part of every token it makes — tokens of two harts never collide.
    lane: u64,
    /// The state of the task current on this hart (an address the kernel gives), or 0.
    current: AtomicUsize,
    /// The token of the state the registers hold; 0 = none.
    live: AtomicU64,
    seq: AtomicU64,
    saves: AtomicU64,
    loads: AtomicU64,
    kept: AtomicU64,
}

impl HartUnit {
    /// CPU `cpu`'s view; its registers hold no state.
    #[must_use]
    pub const fn new(cpu: usize) -> Self {
        Self {
            lane: (cpu as u64 + 1) << LANE_SHIFT,
            current: AtomicUsize::new(0),
            live: AtomicU64::new(0),
            seq: AtomicU64::new(0),
            saves: AtomicU64::new(0),
            loads: AtomicU64::new(0),
            kept: AtomicU64::new(0),
        }
    }

    /// A token no state carries yet.
    #[must_use]
    pub fn fresh(&self) -> u64 {
        self.lane | (self.seq.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// The task current on this hart changed: `state` is its state's address, or 0.
    pub fn runs(&self, state: usize) {
        self.current.store(state, Ordering::Release);
    }

    /// The state of the task current on this hart, or 0.
    #[must_use]
    pub fn current(&self) -> usize {
        self.current.load(Ordering::Acquire)
    }

    /// `state` is being freed: this hart no longer names it.
    pub fn forget(&self, state: usize) {
        let _ = self.current.compare_exchange(state, 0, Ordering::AcqRel, Ordering::Relaxed);
    }

    /// `(saves, loads, kept)`: states written, states loaded, returns that found the
    /// registers already right.
    #[must_use]
    pub fn counts(&self) -> (u64, u64, u64) {
        (
            self.saves.load(Ordering::Relaxed),
            self.loads.load(Ordering::Relaxed),
            self.kept.load(Ordering::Relaxed),
        )
    }

    /// A thread trapped from user mode with `unit`; `state` is its state's token. Returns the
    /// switch its frame keeps and what moves.
    pub fn trapped(&self, unit: Unit, state: Option<&AtomicU64>) -> (Unit, Step) {
        match (unit, state) {
            (Unit::Off, _) => (Unit::Off, Step::Keep),
            (_, None) => (Unit::Off, Step::Refuse),
            (Unit::Dirty, Some(token)) => {
                let now = self.fresh();
                token.store(now, Ordering::Relaxed);
                self.live.store(now, Ordering::Relaxed);
                self.saves.fetch_add(1, Ordering::Relaxed);
                (Unit::Clean, Step::Save)
            }
            (kept, Some(_)) => (kept, Step::Keep),
        }
    }

    /// A thread returns to user mode on this hart; `unit` is what its frame says, `state` its
    /// state's token. Returns the switch it runs with and what moves. The frame is not
    /// trusted: without a state the unit is off, with one it is on.
    pub fn resumes(&self, unit: Unit, state: Option<&AtomicU64>) -> (Unit, Step) {
        let Some(token) = state else { return (Unit::Off, Step::Keep) };
        let token = token.load(Ordering::Relaxed);
        if self.live.load(Ordering::Relaxed) == token {
            self.kept.fetch_add(1, Ordering::Relaxed);
            return (if unit == Unit::Off { Unit::Clean } else { unit }, Step::Keep);
        }
        self.live.store(token, Ordering::Relaxed);
        self.loads.fetch_add(1, Ordering::Relaxed);
        (Unit::Clean, Step::Load { from: ((token >> LANE_SHIFT) as usize).saturating_sub(1) })
    }
}

/// Whether `inst` (the trapped instruction's bits, a compressed one in the low half) belongs
/// to the floating-point unit: its loads and stores, its arithmetic, its control register.
#[must_use]
pub const fn is_unit_instruction(inst: u32) -> bool {
    if inst & 3 != 3 {
        // Compressed: `c.fld`, `c.fsd` and their stack-pointer forms (64-bit harts).
        let (quadrant, funct3) = (inst & 3, (inst >> 13) & 7);
        return matches!(quadrant, 0 | 2) && matches!(funct3, 1 | 5);
    }
    let funct3 = (inst >> 12) & 7;
    match inst & 0x7f {
        // Loads and stores: the widths 1–4 are the unit's, the others the vector unit's.
        0x07 | 0x27 => matches!(funct3, 1..=4),
        // Fused multiply-adds and every other operation.
        0x43 | 0x47 | 0x4b | 0x4f | 0x53 => true,
        // A control-register instruction naming `fflags`, `frm` or `fcsr`.
        0x73 => funct3 != 0 && funct3 != 4 && matches!(inst >> 20, 1..=3),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hart of the model: its view and one "register" standing for all of them.
    struct Hart {
        unit: HartUnit,
        regs: u64,
    }

    /// A thread of the model: its state (token, saved register), its frame's switch, and what
    /// it last wrote — what it must read whenever it runs.
    struct Thread {
        state: Option<(AtomicU64, u64)>,
        unit: Unit,
        wrote: u64,
    }

    impl Thread {
        const fn new() -> Self {
            Self { state: None, unit: Unit::Off, wrote: 0 }
        }

        /// The first instruction of the unit: a state of zeros, as `trap::fp::first_use`
        /// gives it.
        fn first_use(&mut self, hart: &Hart) {
            self.state = Some((AtomicU64::new(hart.unit.fresh()), 0));
            self.unit = Unit::Initial;
        }

        /// The trap entry, as `__fp_trap_enter` runs it.
        fn trap(&mut self, hart: &Hart) -> Step {
            let (unit, step) = hart.unit.trapped(self.unit, self.state.as_ref().map(|s| &s.0));
            if step == Step::Save {
                self.state.as_mut().expect("a save has a state").1 = hart.regs;
            }
            self.unit = unit;
            step
        }

        /// The return to user mode, as `__fp_user_return` runs it.
        fn resume(&mut self, hart: &mut Hart) -> Step {
            let (unit, step) = hart.unit.resumes(self.unit, self.state.as_ref().map(|s| &s.0));
            if matches!(step, Step::Load { .. }) {
                hart.regs = self.state.as_ref().expect("a load has a state").1;
            }
            self.unit = unit;
            step
        }

        /// The thread, running on `hart`, reads its register.
        fn reads(&self, hart: &Hart) -> u64 {
            assert_ne!(self.unit, Unit::Off, "a read with the unit off traps");
            hart.regs
        }

        /// The thread, running on `hart`, writes its register: the hardware marks the unit.
        fn writes(&mut self, hart: &mut Hart, value: u64) {
            assert_ne!(self.unit, Unit::Off, "a write with the unit off traps");
            hart.regs = value;
            self.unit = Unit::Dirty;
            self.wrote = value;
        }
    }

    fn hart(cpu: usize) -> Hart {
        Hart { unit: HartUnit::new(cpu), regs: 0xDEAD_0000 + cpu as u64 }
    }

    #[test]
    fn the_switch_round_trips_through_a_status_word() {
        let status = 0x8000_0002_0004_0122usize;
        for unit in [Unit::Off, Unit::Initial, Unit::Clean, Unit::Dirty] {
            let with = unit.set_in(status);
            assert_eq!(Unit::of(with), unit);
            assert_eq!(with & !UNIT_MASK, status & !UNIT_MASK, "no other bit moves");
        }
    }

    #[test]
    fn a_new_state_reads_zero_not_what_the_hart_held() {
        let mut h = hart(0);
        let mut t = Thread::new();
        t.first_use(&h);
        assert_eq!(t.resume(&mut h), Step::Load { from: 0 });
        assert_eq!(t.reads(&h), 0, "never the registers another thread left");
    }

    #[test]
    fn a_thread_that_only_reads_is_saved_once() {
        let mut h = hart(0);
        let mut t = Thread::new();
        t.first_use(&h);
        t.resume(&mut h);
        t.writes(&mut h, 7);
        assert_eq!(t.trap(&h), Step::Save);
        for _ in 0..5 {
            assert_eq!(t.resume(&mut h), Step::Keep);
            assert_eq!(t.reads(&h), 7);
            assert_eq!(t.trap(&h), Step::Keep, "nothing changed, nothing is written");
        }
        assert_eq!(h.unit.counts(), (1, 1, 5));
    }

    #[test]
    fn a_thread_alone_on_its_hart_is_never_reloaded() {
        let mut h = hart(0);
        let (mut t, mut other) = (Thread::new(), Thread::new());
        t.first_use(&h);
        t.resume(&mut h);
        for round in 1..=4u64 {
            t.writes(&mut h, round);
            t.trap(&h);
            // A thread without a state runs in between and cannot touch the registers.
            assert_eq!(other.resume(&mut h), Step::Keep);
            assert_eq!(other.unit, Unit::Off);
            other.trap(&h);
            assert_eq!(t.resume(&mut h), Step::Keep);
            assert_eq!(t.reads(&h), round);
        }
    }

    #[test]
    fn test_reject_unit_on_without_a_state() {
        let mut h = hart(0);
        let mut t = Thread::new();
        // A frame that says the unit is on (copied, forged or a kernel bug): off at the return.
        for forged in [Unit::Initial, Unit::Clean, Unit::Dirty] {
            t.unit = forged;
            assert_eq!(t.resume(&mut h), Step::Keep);
            assert_eq!(t.unit, Unit::Off);
        }
    }

    #[test]
    fn test_reject_changed_registers_without_a_state() {
        let h = hart(0);
        let mut t = Thread::new();
        for forged in [Unit::Initial, Unit::Clean, Unit::Dirty] {
            t.unit = forged;
            assert_eq!(t.trap(&h), Step::Refuse, "counted, and saved to nowhere");
            assert_eq!(t.unit, Unit::Off);
        }
    }

    #[test]
    fn test_reject_stale_registers_after_a_migration() {
        let (mut a, mut b) = (hart(0), hart(1));
        let mut t = Thread::new();
        t.first_use(&a);
        t.resume(&mut a);
        t.writes(&mut a, 1);
        t.trap(&a);
        // On the other hart: its registers are loaded, and the state names where it came from.
        assert_eq!(t.resume(&mut b), Step::Load { from: 0 });
        t.writes(&mut b, 2);
        t.trap(&b);
        // Back: hart 0 still holds the value 1 it saved — a stale register set.
        assert_eq!(a.regs, 1);
        assert_eq!(t.resume(&mut a), Step::Load { from: 1 });
        assert_eq!(t.reads(&a), 2);
    }

    #[test]
    fn a_read_only_visit_to_another_hart_costs_no_reload_at_home() {
        let (mut a, mut b) = (hart(0), hart(1));
        let mut t = Thread::new();
        t.first_use(&a);
        t.resume(&mut a);
        t.writes(&mut a, 9);
        t.trap(&a);
        assert_eq!(t.resume(&mut b), Step::Load { from: 0 });
        assert_eq!(t.trap(&b), Step::Keep);
        assert_eq!(t.resume(&mut a), Step::Keep, "hart 0 still holds exactly this state");
        assert_eq!(t.reads(&a), 9);
    }

    #[test]
    fn a_freed_state_is_forgotten_only_by_the_hart_that_names_it() {
        let (a, b) = (HartUnit::new(0), HartUnit::new(1));
        a.runs(0x1000);
        b.runs(0x2000);
        a.forget(0x2000);
        b.forget(0x2000);
        assert_eq!((a.current(), b.current()), (0x1000, 0));
    }

    #[test]
    fn test_reject_first_use_by_another_instruction() {
        // The unit's own: fld, fsd, fadd.d, fmadd.d, a read of fcsr, c.fld, c.fsdsp.
        for inst in [0x0005_3007u32, 0x00a5_3027, 0x02a5_7553, 0x6aa5_7543, 0x0030_2573] {
            assert!(is_unit_instruction(inst), "{inst:#010x}");
        }
        assert!(is_unit_instruction(0x2000) && is_unit_instruction(0xa002));
        // Not the unit's: an integer add, a vector load (width 0), a vector operation, a read
        // of the cycle counter, a cache-block instruction, c.ld, an all-zero word.
        for inst in [0x00b5_0533u32, 0x0205_0007, 0x02b5_0557, 0xc000_2573, 0x0015_200f] {
            assert!(!is_unit_instruction(inst), "{inst:#010x}");
        }
        assert!(!is_unit_instruction(0x6000) && !is_unit_instruction(0));
    }

    #[test]
    fn tokens_of_two_harts_never_collide() {
        let (a, b) = (HartUnit::new(0), HartUnit::new(1));
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1_000 {
            assert!(seen.insert(a.fresh()) && seen.insert(b.fresh()));
        }
        assert!(!seen.contains(&0), "0 is a hart that holds nothing");
    }

    /// Any schedule: every thread reads what it last wrote (or zero), whatever ran in between
    /// and wherever; a thread without a state never has the unit on.
    #[test]
    fn random_schedules_keep_every_threads_registers() {
        let mut rng = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move |below: u64| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng % below
        };
        let mut harts: Vec<Hart> = (0..4).map(hart).collect();
        let mut threads: Vec<Thread> = (0..9).map(|_| Thread::new()).collect();
        // Which thread is in user mode on each hart.
        let mut on: Vec<Option<usize>> = vec![None; harts.len()];
        let mut used = 0u64;
        for step in 0..100_000u64 {
            let h = next(harts.len() as u64) as usize;
            let t = next(threads.len() as u64) as usize;
            if on.contains(&Some(t)) && on[h] != Some(t) {
                continue; // a thread runs on one hart at a time
            }
            // The hart's thread traps; `t` returns to user mode on it.
            if let Some(running) = on[h].take() {
                threads[running].trap(&harts[h]);
            }
            let thread = &mut threads[t];
            thread.resume(&mut harts[h]);
            on[h] = Some(t);
            match next(4) {
                // Uses the unit (three threads never do).
                0 | 1 if t % 3 != 0 => {
                    if thread.state.is_none() {
                        thread.first_use(&harts[h]);
                        thread.resume(&mut harts[h]);
                    }
                    assert_eq!(thread.reads(&harts[h]), thread.wrote, "step {step}");
                    thread.writes(&mut harts[h], step + 1);
                    used += 1;
                }
                _ if thread.state.is_some() => {
                    assert_eq!(thread.reads(&harts[h]), thread.wrote, "step {step}");
                }
                _ => assert_eq!(thread.unit, Unit::Off, "step {step}"),
            }
        }
        assert!(used > 10_000, "the schedule exercised the unit ({used} writes)");
        assert!(threads.iter().step_by(3).all(|t| t.state.is_none() && t.unit == Unit::Off));
        let moved = harts.iter().map(|h| h.unit.counts().1).sum::<u64>();
        assert!(moved > 1_000, "states moved between harts and threads ({moved} loads)");
    }
}
