// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the standing detector of a thread's floating-point registers (RFC-0119,
//!   ADR-0048): two threads of this process each hold their own pattern in `f0`–`f31` and
//!   the control register and hand the hart to each other 10 000 times. A hand-over is a
//!   block on a fence and a wake by the other thread — a wake places a thread on the CPU it
//!   is pinned to, so both hop together over the CPUs this task may run on, and every
//!   round's registers come back from the kernel's copy. Four more rounds both threads
//!   compute at once for several ticks: preempted in the middle of use, no syscall between
//!   the write and the check. Every register is checked after every round. A flipped bit is
//!   the kernel's save, load or first-use path: the two routines below are the only
//!   floating-point instructions of this program (it is built without them), so nothing
//!   else touches the registers.
//!   The kernel's side of the proof: `KSELFTEST: fp state lazy ok` (the saves it did, and
//!   that the unit was never on for a thread without a state) and, with more than one hart,
//!   `KSELFTEST: fp state moved ok`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) and the board — `SELFTEST: fpsoak ok`.
//! RFC: docs/rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use nexus_abi::sched::{self, CpuSet};
use nexus_abi::{fence_signal, fence_wait, Cap};

use crate::markers::{emit_bytes, emit_hex_u64, emit_line, emit_u64};

core::arch::global_asm!(
    ".option push",
    ".option arch, +f, +d",
    // __fpsoak_seed(a0 = base, a1 = control): f<n> = the bits base + n, fcsr = control.
    ".globl __fpsoak_seed",
    ".type __fpsoak_seed, @function",
    "__fpsoak_seed:",
    "fscsr a1",
    ".irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31",
    r"fmv.d.x f\n, a0",
    "addi a0, a0, 1",
    ".endr",
    "ret",
    // __fpsoak_check(a0 = base, a1 = control) -> a0: bit n = f<n> differs, bit 32 = fcsr.
    ".globl __fpsoak_check",
    ".type __fpsoak_check, @function",
    "__fpsoak_check:",
    "li t2, 0",
    "frcsr t0",
    "beq t0, a1, 1f",
    "li t2, 1",
    "slli t2, t2, 32",
    "1:",
    ".irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31",
    r"fmv.x.d t0, f\n",
    "beq t0, a0, 2f",
    "li t1, 1",
    r"slli t1, t1, \n",
    "or t2, t2, t1",
    "2:",
    "addi a0, a0, 1",
    ".endr",
    "mv a0, t2",
    "ret",
    ".option pop",
);

extern "C" {
    fn __fpsoak_seed(base: u64, control: u64);
    fn __fpsoak_check(base: u64, control: u64) -> u64;
}

/// A thread's pattern: the bits of its registers and its control register (a rounding mode
/// other than the default, and flags) — the two threads share no value.
#[derive(Clone, Copy)]
struct Pattern {
    base: u64,
    control: u64,
}

const MINE: Pattern = Pattern { base: 0x5A5A_F0F0_0000_0100, control: 0x3F };
const PARTNERS: Pattern = Pattern { base: 0xC3C3_0F0F_0000_0200, control: 0x4A };

impl Pattern {
    fn seed(self) {
        // SAFETY: writes the floating-point registers only; the Rust code of this program
        // never uses them (soft-float build).
        unsafe { __fpsoak_seed(self.base, self.control) }
    }

    /// The registers that differ from the pattern (bit n = `f<n>`, bit 32 = the control
    /// register).
    fn differs(self) -> u64 {
        // SAFETY: reads the floating-point registers only.
        unsafe { __fpsoak_check(self.base, self.control) }
    }
}

const ROUNDS: u64 = 10_000;
/// The rounds after the hand-overs in which both threads compute at once.
const BUSY_ROUNDS: u64 = 4;
/// Long enough for several scheduler ticks on the board and on the emulator.
const BUSY_STEPS: u32 = 8_000_000;
/// Both threads move to the next CPU of the set every so many rounds.
const HOP_EVERY: u64 = 100;
/// A round the partner does not answer within this time is a stalled soak (the FAIL
/// witness — a live partner answers in microseconds).
const STALL_NS: u64 = 10_000_000_000;
const NO_CPU: u32 = u32::MAX;
const NO_FENCE: u32 = u32::MAX;

/// The CPU both threads pin themselves to (`NO_CPU`: where they are).
static TARGET: AtomicU32 = AtomicU32::new(NO_CPU);
/// The partner's capabilities: the fence the main thread advances to hand a round over, and
/// the one the partner advances to hand it back.
static PARTNER_GO: AtomicU32 = AtomicU32::new(NO_FENCE);
static PARTNER_DONE: AtomicU32 = AtomicU32::new(NO_FENCE);
static PARTNER_ROUNDS: AtomicU64 = AtomicU64::new(0);
static PARTNER_DIFFERS: AtomicU64 = AtomicU64::new(0);
static mut PARTNER_STACK: [u8; 16 * 1024] = [0; 16 * 1024];

fn pin(cpu: u32) -> bool {
    CpuSet::single(cpu as usize).is_some_and(|one| sched::set_affinity(one).is_ok())
}

fn compute() {
    for step in 0..BUSY_STEPS {
        core::hint::black_box(step);
    }
}

extern "C" fn partner(_arg: usize) {
    let go = PARTNER_GO.load(Ordering::Acquire);
    let done = PARTNER_DONE.load(Ordering::Acquire);
    let (mut pinned, mut differs, mut round) = (NO_CPU, 0u64, 0u64);
    PARTNERS.seed();
    // Parked until the main thread hands a round over; past the last round (or with the
    // fence gone) the thread ends.
    while round < ROUNDS + BUSY_ROUNDS && fence_wait(go, round + 1, 0).is_ok() {
        round += 1;
        differs |= PARTNERS.differs();
        // Written again: the unit is changed, so the next trap saves it.
        PARTNERS.seed();
        if round > ROUNDS {
            compute();
            differs |= PARTNERS.differs();
        }
        // A pin places the thread at its next wake — the wake of the next round.
        let target = TARGET.load(Ordering::Acquire);
        if target != pinned {
            let _ = pin(target);
            pinned = target;
        }
        PARTNER_DIFFERS.store(differs, Ordering::Release);
        PARTNER_ROUNDS.store(round, Ordering::Release);
        if fence_signal(done, round).is_err() {
            return;
        }
    }
}

/// The next CPU of `set` after `cpu` (wrapping).
fn next_cpu(set: u64, cpu: u32) -> u32 {
    let above = set & !((2u64 << cpu.min(63)).wrapping_sub(1));
    if cpu < 63 && above != 0 {
        above.trailing_zeros()
    } else {
        set.trailing_zeros()
    }
}

/// Spawns the partner holding its two fences. `None`: no fence, no thread, or no hand-over
/// of the capabilities — the partner, if it exists, has ended and is reaped.
fn spawn_partner(go: Cap, done: Cap) -> Option<nexus_abi::Pid> {
    // SAFETY: the stack is used by the one thread spawned here; the proof is sequential
    // (spawn → soak → join).
    let stack = unsafe { &mut *core::ptr::addr_of_mut!(PARTNER_STACK) };
    let pid = nexus_abi::thread::spawn_thread_suspended(partner, 0, stack).ok()?;
    let rights = nexus_abi::Rights::MANAGE;
    let slots = nexus_abi::cap_transfer(pid, go, rights)
        .and_then(|go| nexus_abi::cap_transfer(pid, done, rights).map(|done| (go, done)));
    // Without its fences the partner's first wait is refused and it ends at once.
    let (partner_go, partner_done) = slots.unwrap_or((NO_FENCE, NO_FENCE));
    PARTNER_GO.store(partner_go, Ordering::Release);
    PARTNER_DONE.store(partner_done, Ordering::Release);
    if nexus_abi::task_resume(pid).is_err() {
        return None;
    }
    if slots.is_err() {
        let _ = nexus_abi::wait(pid as i32);
        return None;
    }
    Some(pid)
}

pub(crate) fn fpsoak_proof() {
    TARGET.store(NO_CPU, Ordering::Release);
    PARTNER_ROUNDS.store(0, Ordering::Release);
    PARTNER_DIFFERS.store(0, Ordering::Release);
    let home = sched::get_affinity().ok();
    let set = home.map_or(0, CpuSet::bits);
    let (Ok(go), Ok(done)) = (nexus_abi::fence_create(), nexus_abi::fence_create()) else {
        emit_line(crate::markers::M_SELFTEST_FPSOAK_FAIL_SPAWN);
        return;
    };
    let Some(pid) = spawn_partner(go, done) else {
        let _ = (nexus_abi::cap_close(go), nexus_abi::cap_close(done));
        emit_line(crate::markers::M_SELFTEST_FPSOAK_FAIL_SPAWN);
        return;
    };

    let (mut differs, mut rounds, mut hops, mut stalled) = (0u64, 0u64, 0u32, false);
    let mut cpu = set.trailing_zeros().min(63);
    MINE.seed();
    while rounds < ROUNDS + BUSY_ROUNDS && differs == 0 && !stalled {
        let round = rounds + 1;
        if round <= ROUNDS && rounds % HOP_EVERY == 0 && set.count_ones() > 1 {
            cpu = next_cpu(set, cpu);
            TARGET.store(cpu, Ordering::Release);
            hops += u32::from(pin(cpu));
        }
        let answer_by = nexus_abi::nsec().unwrap_or(0).saturating_add(STALL_NS);
        // The partner wakes with this round. Up to the last hand-over the hart goes to it
        // while this thread is parked; after that both compute at once.
        stalled = fence_signal(go, round).is_err();
        if round > ROUNDS {
            compute();
            differs |= MINE.differs();
            MINE.seed();
        }
        stalled |= fence_wait(done, round, answer_by).is_err();
        differs |= MINE.differs();
        MINE.seed();
        rounds = round;
    }
    // Past its last round the partner ends by itself; on a cut-short soak every round is
    // handed over at once and it runs them out.
    let _ = fence_signal(go, u64::MAX);
    if let Some(home) = home {
        let _ = sched::set_affinity(home);
    }
    let reaped = !stalled && matches!(nexus_abi::wait(pid as i32), Ok((_, 0)));
    let _ = (nexus_abi::cap_close(go), nexus_abi::cap_close(done));
    let partner_rounds = PARTNER_ROUNDS.load(Ordering::Acquire);
    let partner_differs = PARTNER_DIFFERS.load(Ordering::Acquire);

    emit_bytes(b"fpsoak: rounds=");
    emit_u64(rounds);
    emit_bytes(b" partner=");
    emit_u64(partner_rounds);
    emit_bytes(b" hops=");
    emit_u64(u64::from(hops));
    emit_bytes(b" differs=0x");
    emit_hex_u64(differs);
    emit_bytes(b" partner_differs=0x");
    emit_hex_u64(partner_differs);
    emit_line("");
    let complete = rounds == ROUNDS + BUSY_ROUNDS && partner_rounds == rounds;
    if differs != 0 || partner_differs != 0 {
        emit_line(crate::markers::M_SELFTEST_FPSOAK_FAIL_REGISTERS);
    } else if stalled || !complete || !reaped {
        emit_line(crate::markers::M_SELFTEST_FPSOAK_FAIL_PARTNER);
    } else {
        emit_line(crate::markers::M_SELFTEST_FPSOAK_OK);
    }
}
