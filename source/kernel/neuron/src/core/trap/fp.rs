// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a thread's floating-point registers on the target (RFC-0119): the state a task
//! owns, the two places every path passes — the trap entry and the return to user mode
//! (`arch/riscv/trap.S`, the idle loop's staged resume) — and the first use (`illegal.rs`).
//! The decisions are `crate::fp_rule`'s (pure, host-tested); this file moves the registers.
//! A thread without a state runs with the unit off and costs two tests per trap.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the rules on the host (`crate::fp_rule`); QEMU and board:
//!   `SELFTEST: fpsoak ok`, `KSELFTEST: fp state lazy ok`, `KSELFTEST: fp state moved ok`
//!   (lanes with more than one hart)
//! INVARIANTS: a hart's current state is the state of the task current on it
//!   (`TaskTable::set_current`, `first_use`) or 0, and is forgotten before a state is freed;
//!   only the hart running a task touches its state's registers; the kernel has no
//!   floating-point instruction outside `__fp_store` and `__fp_load`, and the unit is off
//!   while the kernel runs outside them (`hart_starts`, `trap.S`; checked at every return)
//! RFC: docs/rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md
//! ADR: docs/adr/0099-every-userspace-program-is-built-for-one-hard-float-target-vectors-are-asked-for-at-run-time.md

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use super::{KernelGuard, TrapFrame};
use crate::fp_rule::{is_unit_instruction, HartUnit, Step, Unit};
use crate::sync::hart_table::{HartTable, SizeError};

const STATUS_FROM_SUPERVISOR: usize = 1 << 8;
/// The lazy line is printed once this many saves happened without a refusal.
const SAVES_FOR_THE_LINE: u64 = 10_000;
/// `f0`–`f31` and the control register.
const REGISTERS: usize = 33;

/// A thread's saved registers.
#[repr(C)]
pub struct State {
    /// New after every write of `regs`; a hart holding this token holds these registers.
    token: AtomicU64,
    regs: UnsafeCell<[u64; REGISTERS]>,
}

// SAFETY: `regs` is read and written only by the hart its task is current on (the two hooks
// below), never by two harts at once — a task runs on one hart at a time.
unsafe impl Sync for State {}

impl Drop for State {
    fn drop(&mut self) {
        STATES.fetch_sub(1, Ordering::Relaxed);
        // No hart keeps naming a state that is gone (its task died while current there).
        let gone = core::ptr::from_ref::<Self>(self) as usize;
        HARTS.iter().for_each(|hart| hart.forget(gone));
    }
}

static HARTS: HartTable<HartUnit> = HartTable::new(HartUnit::new(0));
/// States alive — the threads that ever used the unit and still exist.
static STATES: AtomicUsize = AtomicUsize::new(0);
/// Whether every hart carries F and D (the device tree, read at boot).
static OFFERED: AtomicBool = AtomicBool::new(false);
static REFUSED: AtomicU64 = AtomicU64::new(0);
static LAZY_SAID: AtomicBool = AtomicBool::new(false);
static MOVED_SAID: AtomicBool = AtomicBool::new(false);
static FAIL_SAID: AtomicBool = AtomicBool::new(false);

core::arch::global_asm!(
    ".option push",
    ".option arch, +f, +d",
    // __fp_store(a0 = registers of a state): the hart's registers into the state.
    ".globl __fp_store",
    ".type __fp_store, @function",
    "__fp_store:",
    "li t0, 0x6000",
    "csrs sstatus, t0",
    "fsd f0, 0(a0)",
    "fsd f1, 8(a0)",
    "fsd f2, 16(a0)",
    "fsd f3, 24(a0)",
    "fsd f4, 32(a0)",
    "fsd f5, 40(a0)",
    "fsd f6, 48(a0)",
    "fsd f7, 56(a0)",
    "fsd f8, 64(a0)",
    "fsd f9, 72(a0)",
    "fsd f10, 80(a0)",
    "fsd f11, 88(a0)",
    "fsd f12, 96(a0)",
    "fsd f13, 104(a0)",
    "fsd f14, 112(a0)",
    "fsd f15, 120(a0)",
    "fsd f16, 128(a0)",
    "fsd f17, 136(a0)",
    "fsd f18, 144(a0)",
    "fsd f19, 152(a0)",
    "fsd f20, 160(a0)",
    "fsd f21, 168(a0)",
    "fsd f22, 176(a0)",
    "fsd f23, 184(a0)",
    "fsd f24, 192(a0)",
    "fsd f25, 200(a0)",
    "fsd f26, 208(a0)",
    "fsd f27, 216(a0)",
    "fsd f28, 224(a0)",
    "fsd f29, 232(a0)",
    "fsd f30, 240(a0)",
    "fsd f31, 248(a0)",
    "frcsr t0",
    "sd t0, 256(a0)",
    // Off again: the kernel never runs with the unit on outside these two routines.
    "li t0, 0x6000",
    "csrc sstatus, t0",
    "ret",
    // __fp_load(a0 = registers of a state): the state into the hart's registers.
    ".globl __fp_load",
    ".type __fp_load, @function",
    "__fp_load:",
    "li t0, 0x6000",
    "csrs sstatus, t0",
    "ld t0, 256(a0)",
    "fscsr t0",
    "fld f0, 0(a0)",
    "fld f1, 8(a0)",
    "fld f2, 16(a0)",
    "fld f3, 24(a0)",
    "fld f4, 32(a0)",
    "fld f5, 40(a0)",
    "fld f6, 48(a0)",
    "fld f7, 56(a0)",
    "fld f8, 64(a0)",
    "fld f9, 72(a0)",
    "fld f10, 80(a0)",
    "fld f11, 88(a0)",
    "fld f12, 96(a0)",
    "fld f13, 104(a0)",
    "fld f14, 112(a0)",
    "fld f15, 120(a0)",
    "fld f16, 128(a0)",
    "fld f17, 136(a0)",
    "fld f18, 144(a0)",
    "fld f19, 152(a0)",
    "fld f20, 160(a0)",
    "fld f21, 168(a0)",
    "fld f22, 176(a0)",
    "fld f23, 184(a0)",
    "fld f24, 192(a0)",
    "fld f25, 200(a0)",
    "fld f26, 208(a0)",
    "fld f27, 216(a0)",
    "fld f28, 224(a0)",
    "fld f29, 232(a0)",
    "fld f30, 240(a0)",
    "fld f31, 248(a0)",
    // Off until the status word of the frame is written back at the return to user mode.
    "li t0, 0x6000",
    "csrc sstatus, t0",
    "ret",
    ".option pop",
);

extern "C" {
    /// Turns the unit on in the live status word, then stores `f0`–`f31` and `fcsr`.
    fn __fp_store(registers: *mut u64);
    /// Turns the unit on in the live status word, then loads `fcsr` and `f0`–`f31`.
    fn __fp_load(registers: *const u64);
}

/// States what the machine's harts carry: the unit is offered when all of them have F and D.
pub fn offer(every_hart_has_f_and_d: bool) {
    OFFERED.store(every_hart_has_f_and_d, Ordering::Release);
}

/// Sizes the harts' views for a machine of `harts` CPUs.
pub fn size(harts: usize) -> Result<(), SizeError> {
    let mut cpu = 0;
    HARTS.size(harts, || {
        cpu += 1;
        HartUnit::new(cpu)
    })
}

fn this_hart() -> Option<(usize, &'static HartUnit)> {
    let cpu = crate::smp::cpu_current_id().as_index();
    HARTS.get(cpu).map(|hart| (cpu, hart))
}

/// The task current on this hart changed (`TaskTable::set_current`): `state` is its state.
pub fn now_runs(state: Option<&State>) {
    if let Some((_, hart)) = this_hart() {
        hart.runs(state.map_or(0, |state| core::ptr::from_ref(state) as usize));
    }
}

/// The state of the task current on this hart.
fn current(hart: &HartUnit) -> Option<&'static State> {
    // SAFETY: the address is 0 or the state of the task current on this hart — given by
    // `set_current` and `first_use` on this hart, and forgotten (`Drop`) before the state is
    // freed.
    unsafe { (hart.current() as *const State).as_ref() }
}

fn refused(cpu: usize, unit: Unit, what: &str) {
    REFUSED.fetch_add(1, Ordering::Relaxed);
    if !FAIL_SAID.swap(true, Ordering::AcqRel) {
        log_error!(target: "fp", "KSELFTEST: fp state FAIL (cpu={} unit={:?} {})", cpu, unit, what);
    }
}

/// A hart's kernel runs with the unit off: from its bring-up — whatever the firmware left in
/// the status word — and from every trap entry on (`trap.S`). The kernel is built without
/// floating-point instructions; with the unit off, one that got in traps instead of changing
/// the registers of whichever thread the hart holds.
pub fn hart_starts() {
    // SAFETY: this hart's status register; no thread's registers are on it yet.
    unsafe { riscv::register::sstatus::set_fs(riscv::register::sstatus::FS::Off) };
}

/// The unit as the kernel has it on this hart now (not a frame's).
fn kernels_unit() -> Unit {
    match riscv::register::sstatus::read().fs() {
        riscv::register::sstatus::FS::Off => Unit::Off,
        riscv::register::sstatus::FS::Initial => Unit::Initial,
        riscv::register::sstatus::FS::Clean => Unit::Clean,
        riscv::register::sstatus::FS::Dirty => Unit::Dirty,
    }
}

/// The lazy line: enough saves happened, and the unit was never on without a state.
fn say_lazy(hart: &HartUnit) {
    if hart.counts().0 % 256 != 0 || LAZY_SAID.load(Ordering::Relaxed) {
        return;
    }
    let (mut saves, mut loads, mut kept) = (0, 0, 0);
    for (s, l, k) in HARTS.iter().map(HartUnit::counts) {
        (saves, loads, kept) = (saves + s, loads + l, kept + k);
    }
    if saves < SAVES_FOR_THE_LINE || REFUSED.load(Ordering::Relaxed) != 0 {
        return;
    }
    if !LAZY_SAID.swap(true, Ordering::AcqRel) {
        log_info!(
            target: "fp",
            "KSELFTEST: fp state lazy ok (threads={} saves={} loads={} kept={} refused=0)",
            STATES.load(Ordering::Relaxed),
            saves,
            loads,
            kept
        );
    }
}

/// Trap entry (`trap.S`, before the handler): a thread that changed its registers has them
/// written to its state — here, before anything can run it on another hart.
#[no_mangle]
extern "C" fn __fp_trap_enter(frame: &mut TrapFrame) {
    let unit = Unit::of(frame.sstatus);
    if unit == Unit::Off || frame.sstatus & STATUS_FROM_SUPERVISOR != 0 {
        return;
    }
    let Some((cpu, hart)) = this_hart() else { return };
    let state = current(hart);
    let (unit_now, step) = hart.trapped(unit, state.map(|s| &s.token));
    match (step, state) {
        (Step::Save, Some(state)) => {
            // SAFETY: the state is this hart's current task's, touched by no other hart while
            // the task is here; the routine turns the unit on before it stores.
            unsafe { __fp_store(state.regs.get().cast()) };
            say_lazy(hart);
        }
        (Step::Refuse, _) => refused(cpu, unit, "without a state"),
        _ => {}
    }
    frame.sstatus = unit_now.set_in(frame.sstatus);
}

/// Return to user mode (`trap.S` after the handler, the idle loop's staged resume): the
/// hart's registers become the thread's unless they are, and the frame's switch is set from
/// whether the task has a state — never from what the frame said.
#[no_mangle]
pub(crate) extern "C" fn __fp_user_return(frame: &mut TrapFrame) {
    // The handler ran since the trap entry turned the unit off: it is still off.
    let kernels = kernels_unit();
    if kernels != Unit::Off {
        refused(crate::smp::cpu_current_id().as_index(), kernels, "on in the kernel");
        hart_starts();
    }
    if frame.sstatus & STATUS_FROM_SUPERVISOR != 0 {
        return;
    }
    let Some((cpu, hart)) = this_hart() else {
        frame.sstatus = Unit::Off.set_in(frame.sstatus);
        return;
    };
    let state = current(hart);
    let (unit, step) = hart.resumes(Unit::of(frame.sstatus), state.map(|s| &s.token));
    if let (Step::Load { from }, Some(state)) = (step, state) {
        // SAFETY: as in `__fp_trap_enter`; the routine turns the unit on before it loads, and
        // the status word the return path writes afterwards is the frame's.
        unsafe { __fp_load(state.regs.get().cast()) };
        if from != cpu && !MOVED_SAID.swap(true, Ordering::AcqRel) {
            log_info!(
                target: "fp",
                "KSELFTEST: fp state moved ok (from=cpu{} to=cpu{})",
                from,
                cpu
            );
        }
    }
    frame.sstatus = unit.set_in(frame.sstatus);
}

/// A user-mode illegal instruction (`stval` = its bits when the hart reports them, else 0):
/// `true` = the thread was given a state of zeros and the instruction runs again.
pub(super) fn first_use(frame: &mut TrapFrame, stval: usize) -> bool {
    if !OFFERED.load(Ordering::Acquire)
        || Unit::of(frame.sstatus) != Unit::Off
        || (stval != 0 && !is_unit_instruction(stval as u32))
    {
        return false;
    }
    let Some((_, hart)) = this_hart() else { return false };
    // U-mode trap → this hart holds no kernel lock (as `cache_right::try_grant`).
    let Ok(mut kernel) = KernelGuard::acquire_as(crate::lock_share::FAULT) else { return false };
    let (_, tasks, ..) = kernel.parts();
    let pid = tasks.current_pid();
    let task = tasks.current_task_mut();
    if task.fp.is_some() {
        return false;
    }
    if !crate::kheap::admits(core::mem::size_of::<State>()) {
        log_error!(target: "fp", "KINIT: fp state refused (pid={} no memory)", pid.as_raw());
        return false;
    }
    let state = alloc::boxed::Box::new(State {
        token: AtomicU64::new(hart.fresh()),
        regs: UnsafeCell::new([0; REGISTERS]),
    });
    STATES.fetch_add(1, Ordering::Relaxed);
    hart.runs(core::ptr::from_ref::<State>(&state) as usize);
    task.fp = Some(state);
    frame.sstatus = Unit::Initial.set_in(frame.sstatus);
    true
}
