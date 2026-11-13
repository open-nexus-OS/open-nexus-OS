// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a thread's floating-point and vector registers on the target (RFC-0119): the
//! states a task owns, the two places every path passes — the trap entry and the return to
//! user mode (`arch/riscv/trap.S`, the idle loop's staged resume) — the first use
//! (`illegal.rs`), and what the machine offers a program (the topology query `Offers`).
//! The decisions are `crate::fp_rule`'s (pure, host-tested), made once per unit; the
//! registers move in `fp_regs`. A thread without a state runs with both units off and costs
//! one test per trap.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the rules on the host (`crate::fp_rule`); QEMU and board:
//!   `SELFTEST: fpsoak ok`, `SELFTEST: vsoak ok`, `KSELFTEST: fp state lazy ok`,
//!   `KSELFTEST: vector state lazy ok`, and on lanes with more than one hart
//!   `KSELFTEST: fp state moved ok`, `KSELFTEST: vector state moved ok`
//! INVARIANTS: a hart's current state of a unit is the state of the task current on it
//!   (`TaskTable::set_current`, `first_use`) or 0, and is forgotten before a state is freed;
//!   only the hart running a task touches its states' registers; the kernel has no
//!   floating-point or vector instruction outside `fp_regs`, and both units are off while the
//!   kernel runs outside it (`hart_starts`, `trap.S`; checked at every return), and a hart's
//!   vector length, type and start are the reset ones while the kernel runs (`rests`); the vector
//!   unit is offered only when every hart's registers have one width the kernel keeps
//! RFC: docs/rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md
//! ADR: docs/adr/0099-every-userspace-program-is-built-for-one-hard-float-target-vectors-are-asked-for-at-run-time.md

use alloc::boxed::Box;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use super::{fp_regs, KernelGuard, TrapFrame};
use crate::fp_rule::{asks_for, keeps_vector_bytes, HartUnit, Kind, Step, Unit};
use crate::sync::hart_table::{HartTable, SizeError};

const STATUS_FROM_SUPERVISOR: usize = 1 << 8;
/// A unit's lazy line is printed once this many saves happened without a refusal.
const SAVES_FOR_THE_LINE: u64 = 10_000;
/// A new vector state's type: no setting yet, as the hardware has it after reset.
const VECTOR_TYPE_UNSET: u64 = 1 << 63;
/// The word of a vector state that holds its type.
const VECTOR_TYPE_WORD: usize = 3;

/// A thread's saved registers of one unit.
pub struct State {
    kind: Kind,
    /// New after every write of `words`; a hart holding this token holds these registers.
    token: AtomicU64,
    words: UnsafeCell<Box<[u64]>>,
}

// SAFETY: `words` is read and written only by the hart its task is current on (the two hooks
// below), never by two harts at once — a task runs on one hart at a time.
unsafe impl Sync for State {}

impl State {
    /// # Safety
    /// The caller is the hart this state's task is current on.
    unsafe fn store(&self) {
        // SAFETY: the words are this state's, sized for its unit (`first_use`); see above.
        unsafe {
            let words = (*self.words.get()).as_mut_ptr();
            match self.kind {
                Kind::Float => fp_regs::__fp_store(words),
                Kind::Vector => fp_regs::__vector_store(words),
            }
        }
    }

    /// # Safety
    /// As [`Self::store`].
    unsafe fn load(&self) {
        // SAFETY: as in `store`.
        unsafe {
            let words = (*self.words.get()).as_ptr();
            match self.kind {
                Kind::Float => fp_regs::__fp_load(words),
                Kind::Vector => fp_regs::__vector_load(words),
            }
        }
    }

    /// The thread continues on registers the hart still holds: what the kernel set to its
    /// own at the trap entry ([`rests`]) is the thread's again.
    ///
    /// # Safety
    /// As [`Self::store`], and the state's words are what the thread had at its trap.
    unsafe fn resume(&self) {
        if self.kind == Kind::Vector {
            // SAFETY: as in `store`; reads the state's first four words.
            unsafe { fp_regs::__vector_resume((*self.words.get()).as_ptr()) };
        }
    }
}

/// The kernel runs under one configuration of a unit, not under the one the trapped thread
/// set: a hart's vector length, type and start index are as after reset from every trap
/// entry of a thread that has the unit — its own are in its state by then (`trapped`).
/// The float unit has no such configuration.
///
/// # Safety
/// For [`Kind::Vector`], this hart's node states the vector extension.
unsafe fn rests(kind: Kind) {
    if kind == Kind::Vector {
        // SAFETY: the caller's.
        unsafe { fp_regs::__vector_rest() };
    }
}

impl Drop for State {
    fn drop(&mut self) {
        STATES[self.kind as usize].fetch_sub(1, Ordering::Relaxed);
        // No hart keeps naming a state that is gone (its task died while current there).
        let gone = core::ptr::from_ref::<Self>(self) as usize;
        HARTS[self.kind as usize].iter().for_each(|hart| hart.forget(gone));
    }
}

/// The states a task owns: one per unit it ever used. Dropped with the task.
#[derive(Default)]
pub struct Held([Option<Box<State>>; 2]);

impl Held {
    fn of(&self, kind: Kind) -> Option<&State> {
        self.0[kind as usize].as_deref()
    }
}

static HARTS: [HartTable<HartUnit>; 2] =
    [HartTable::new(HartUnit::new(0)), HartTable::new(HartUnit::new(0))];
/// States alive per unit — the threads that ever used it and still exist.
static STATES: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
/// Whether a thread is given a state of the unit at its first use.
static OFFERED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// The width of every hart's vector registers in bytes; 0 while the unit is not offered.
static VECTOR_BYTES: AtomicUsize = AtomicUsize::new(0);
/// The extensions every hart's node states (bit = `nexus_sched::Extension`).
static STATED: AtomicUsize = AtomicUsize::new(0);
static REFUSED: AtomicU64 = AtomicU64::new(0);
static LAZY_SAID: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static MOVED_SAID: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static FAIL_SAID: AtomicBool = AtomicBool::new(false);

/// What every hart's node states (the device tree, read on the boot hart before any other
/// hart starts and before any thread runs). The float unit is offered when every hart
/// carries F and D; the vector unit when every hart carries V as well and the registers'
/// width is one the kernel keeps — the boot hart's here, each other hart's at its start.
pub fn offer(stated: impl Fn(nexus_sched::Extension) -> bool) {
    use nexus_sched::Extension;
    let bits = Extension::ALL.iter().fold(0, |bits, &e| bits | (usize::from(stated(e)) << e as u8));
    STATED.store(bits, Ordering::Release);
    let float = stated(Extension::Float);
    let bytes = if float && stated(Extension::Vector) {
        // SAFETY: this hart's node states the vector extension.
        unsafe {
            rests(Kind::Vector);
            fp_regs::__vector_bytes()
        }
    } else {
        0
    };
    let vector = keeps_vector_bytes(bytes);
    VECTOR_BYTES.store(if vector { bytes } else { 0 }, Ordering::Release);
    OFFERED[Kind::Float as usize].store(float, Ordering::Release);
    OFFERED[Kind::Vector as usize].store(vector, Ordering::Release);
    log_info!(
        target: "fp",
        "KINIT: units offered (float={} vector={} vector_bytes={})",
        u8::from(float),
        u8::from(vector),
        bytes
    );
}

/// What every CPU offers a program — the answer to the topology query `Offers`.
#[must_use]
pub fn offers() -> nexus_sched::Offers {
    let stated = STATED.load(Ordering::Acquire);
    let vector = OFFERED[Kind::Vector as usize].load(Ordering::Acquire);
    nexus_sched::Offers::of_machine(
        |e| stated & (1 << e as u8) != 0,
        OFFERED[Kind::Float as usize].load(Ordering::Acquire),
        vector.then(|| VECTOR_BYTES.load(Ordering::Acquire)),
    )
}

/// Sizes the harts' views for a machine of `harts` CPUs.
pub fn size(harts: usize) -> Result<(), SizeError> {
    HARTS.iter().try_for_each(|table| {
        let mut cpu = 0;
        table.size(harts, || {
            cpu += 1;
            HartUnit::new(cpu)
        })
    })
}

/// A hart's kernel runs with both units off: from its bring-up — whatever the firmware left
/// in the status word — and from every trap entry on (`trap.S`). The kernel is built without
/// their instructions; with the units off, one that got in traps instead of changing the
/// registers of whichever thread the hart holds.
/// A hart whose vector registers have another width than the boot hart's ends the offer: a
/// thread could not move between them. Harts start before the first thread runs.
pub fn hart_starts() {
    // SAFETY: this hart's status register; no thread's registers are on it yet.
    unsafe { fp_regs::__units_off() };
    if OFFERED[Kind::Vector as usize].load(Ordering::Acquire) {
        // SAFETY: offered means every hart's node states the vector extension.
        let bytes = unsafe {
            rests(Kind::Vector);
            fp_regs::__vector_bytes()
        };
        let agreed = VECTOR_BYTES.load(Ordering::Acquire);
        if bytes != agreed {
            OFFERED[Kind::Vector as usize].store(false, Ordering::Release);
            VECTOR_BYTES.store(0, Ordering::Release);
            log_error!(
                target: "fp",
                "KINIT: vector unit not offered (cpu={} vector_bytes={} boot hart {})",
                crate::smp::cpu_current_id().as_index(),
                bytes,
                agreed
            );
        }
    }
}

/// The task current on this hart changed (`TaskTable::set_current`): `held` are its states.
pub fn now_runs(held: Option<&Held>) {
    let cpu = crate::smp::cpu_current_id().as_index();
    for kind in Kind::ALL {
        if let Some(hart) = HARTS[kind as usize].get(cpu) {
            let state = held.and_then(|held| held.of(kind));
            hart.runs(state.map_or(0, |state| core::ptr::from_ref(state) as usize));
        }
    }
}

/// The state of `hart`'s unit of the task current on it.
fn current(hart: &HartUnit) -> Option<&'static State> {
    // SAFETY: the address is 0 or the state of the task current on this hart — given by
    // `set_current` and `first_use` on this hart, and forgotten (`Drop`) before the state is
    // freed.
    unsafe { (hart.current() as *const State).as_ref() }
}

fn refused(cpu: usize, kind: Kind, unit: Unit, what: &str) {
    REFUSED.fetch_add(1, Ordering::Relaxed);
    if !FAIL_SAID.swap(true, Ordering::AcqRel) {
        log_error!(
            target: "fp",
            "KSELFTEST: {} state FAIL (cpu={} unit={:?} {})",
            kind.name(),
            cpu,
            unit,
            what
        );
    }
}

/// A unit's lazy line: enough saves happened, and no unit was ever on without a state or in
/// the kernel.
fn say_lazy(kind: Kind, hart: &HartUnit) {
    let said = &LAZY_SAID[kind as usize];
    if hart.counts().0 % 256 != 0 || said.load(Ordering::Relaxed) {
        return;
    }
    let (mut saves, mut loads, mut kept) = (0, 0, 0);
    for (s, l, k) in HARTS[kind as usize].iter().map(HartUnit::counts) {
        (saves, loads, kept) = (saves + s, loads + l, kept + k);
    }
    if saves < SAVES_FOR_THE_LINE || REFUSED.load(Ordering::Relaxed) != 0 {
        return;
    }
    if !said.swap(true, Ordering::AcqRel) {
        log_info!(
            target: "fp",
            "KSELFTEST: {} state lazy ok (threads={} saves={} loads={} kept={} refused=0)",
            kind.name(),
            STATES[kind as usize].load(Ordering::Relaxed),
            saves,
            loads,
            kept
        );
    }
}

/// Trap entry (`trap.S`, before the handler): a thread that changed a unit's registers has
/// them written to its state — here, before anything can run it on another hart.
#[no_mangle]
extern "C" fn __fp_trap_enter(frame: &mut TrapFrame) {
    if frame.sstatus & Kind::SWITCHES == 0 || frame.sstatus & STATUS_FROM_SUPERVISOR != 0 {
        return;
    }
    let cpu = crate::smp::cpu_current_id().as_index();
    for kind in Kind::ALL {
        let unit = Unit::of(kind, frame.sstatus);
        let Some(hart) = HARTS[kind as usize].get(cpu).filter(|_| unit != Unit::Off) else {
            continue;
        };
        let state = current(hart);
        let (unit_now, step) = hart.trapped(unit, state.map(|s| &s.token));
        match (step, state) {
            (Step::Save, Some(state)) => {
                // SAFETY: the state is this hart's current task's, touched by no other hart
                // while the task is here.
                unsafe { state.store() };
                say_lazy(kind, hart);
            }
            (Step::Refuse, _) => refused(cpu, kind, unit, "without a state"),
            _ => {}
        }
        // SAFETY: the unit was on in user mode, so this hart has it.
        unsafe { rests(kind) };
        frame.sstatus = unit_now.set_in(kind, frame.sstatus);
    }
}

/// Return to user mode (`trap.S` after the handler, the idle loop's staged resume): the
/// hart's registers become the thread's unless they are, and the frame's switches are set
/// from whether the task has a state — never from what the frame said.
#[no_mangle]
pub(crate) extern "C" fn __fp_user_return(frame: &mut TrapFrame) {
    let cpu = crate::smp::cpu_current_id().as_index();
    // The handler ran since the trap entry turned the units off: they are still off.
    // SAFETY: reads this hart's status register.
    let kernels = unsafe { fp_regs::__units_status() };
    if kernels & Kind::SWITCHES != 0 {
        for kind in Kind::ALL.into_iter().filter(|&kind| Unit::of(kind, kernels) != Unit::Off) {
            refused(cpu, kind, Unit::of(kind, kernels), "on in the kernel");
        }
        // SAFETY: this hart's status register.
        unsafe { fp_regs::__units_off() };
    }
    if frame.sstatus & STATUS_FROM_SUPERVISOR != 0 {
        return;
    }
    for kind in Kind::ALL {
        let Some(hart) = HARTS[kind as usize].get(cpu) else {
            frame.sstatus = Unit::Off.set_in(kind, frame.sstatus);
            continue;
        };
        let state = current(hart);
        let (unit, step) = hart.resumes(Unit::of(kind, frame.sstatus), state.map(|s| &s.token));
        match (step, state) {
            (Step::Load { from }, Some(state)) => {
                // SAFETY: as in `__fp_trap_enter`; the status word the return path writes
                // afterwards is the frame's.
                unsafe { state.load() };
                if from != cpu && !MOVED_SAID[kind as usize].swap(true, Ordering::AcqRel) {
                    log_info!(
                        target: "fp",
                        "KSELFTEST: {} state moved ok (from=cpu{} to=cpu{})",
                        kind.name(),
                        from,
                        cpu
                    );
                }
            }
            // SAFETY: as above; no save happened since the thread's trap, so the state's
            // words are what it had there.
            (_, Some(state)) => unsafe { state.resume() },
            _ => {}
        }
        frame.sstatus = unit.set_in(kind, frame.sstatus);
    }
}

/// A user-mode illegal instruction (`stval` = its bits when the hart reports them, else 0):
/// `true` = the thread was given a state of zeros of the unit the instruction needs, and the
/// instruction runs again.
pub(super) fn first_use(frame: &mut TrapFrame, stval: usize) -> bool {
    let offered = [
        OFFERED[Kind::Float as usize].load(Ordering::Acquire),
        OFFERED[Kind::Vector as usize].load(Ordering::Acquire),
    ];
    let Some(kind) = asks_for(stval as u32, frame.sstatus, offered) else { return false };
    let cpu = crate::smp::cpu_current_id().as_index();
    let Some(hart) = HARTS[kind as usize].get(cpu) else { return false };
    // U-mode trap → this hart holds no kernel lock (as `cache_right::try_grant`).
    let Ok(mut kernel) = KernelGuard::acquire_as(crate::lock_share::FAULT) else { return false };
    let (_, tasks, ..) = kernel.parts();
    let pid = tasks.current_pid();
    let task = tasks.current_task_mut();
    if task.fp.0[kind as usize].is_some() {
        return false;
    }
    let words = kind.state_words(VECTOR_BYTES.load(Ordering::Acquire));
    if !crate::kheap::admits(core::mem::size_of::<State>() + words * 8) {
        log_error!(
            target: "fp",
            "KINIT: {} state refused (pid={} no memory)",
            kind.name(),
            pid.as_raw()
        );
        return false;
    }
    let mut zeros = alloc::vec![0u64; words].into_boxed_slice();
    if kind == Kind::Vector {
        zeros[VECTOR_TYPE_WORD] = VECTOR_TYPE_UNSET;
    }
    let state = Box::new(State {
        kind,
        token: AtomicU64::new(hart.fresh()),
        words: UnsafeCell::new(zeros),
    });
    STATES[kind as usize].fetch_add(1, Ordering::Relaxed);
    hart.runs(core::ptr::from_ref::<State>(&state) as usize);
    task.fp.0[kind as usize] = Some(state);
    frame.sstatus = Unit::Initial.set_in(kind, frame.sstatus);
    true
}
