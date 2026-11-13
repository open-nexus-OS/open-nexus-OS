// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the standing detector of a thread's floating-point and vector registers
//!   (RFC-0119, ADR-0048), one soak per register set the machine offers. This program asks
//!   the kernel what every CPU carries (`nexus_abi::sched::offers`) and runs the soak of
//!   each unit named there; the vector soak's buffers have the width the answer states.
//!   A soak: two threads of this process each hold their own pattern in the whole register
//!   set and hand the hart to each other 10 000 times. A hand-over is a block on a fence and
//!   a wake by the other thread — a wake places a thread on the CPU it is pinned to, so both
//!   hop together over the CPUs this task may run on, and every round's registers come back
//!   from the kernel's copy, across two syscalls. Four more rounds both threads compute at
//!   once for several ticks: preempted in the middle of use, no syscall between the write
//!   and the check. Every register is checked after every round. A flipped bit is the
//!   kernel's save, load or first-use path: the routines of `fpsoak` and `vsoak` are the only
//!   instructions of this program that touch these registers.
//!   The answer itself is proven by acting on it: `SELFTEST: hw probe ok (` only after the
//!   soak of every unit it names passed and this thread's own vector registers have the
//!   width it states.
//!   The kernel's side: `KSELFTEST: fp state lazy ok`, `KSELFTEST: vector state lazy ok`
//!   (the saves it did, and that no unit was ever on for a thread without a state) and, with
//!   more than one hart, `… state moved ok`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) and the board — `SELFTEST: fpsoak ok`,
//!   `SELFTEST: vsoak ok`, `SELFTEST: hw probe ok (`.
//! RFC: docs/rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use nexus_abi::sched::{self, CpuSet, Extension};
use nexus_abi::{fence_signal, fence_wait, Cap};

use super::{fpsoak, vsoak};
use crate::markers::{self, emit_bytes, emit_hex_u64, emit_line, emit_u64};

/// Which of a soak's two threads: each holds its own pattern.
#[derive(Clone, Copy)]
pub(super) enum Who {
    Main = 0,
    Partner = 1,
}

/// A register set a thread owns.
#[derive(Clone, Copy)]
enum Unit {
    Float = 0,
    Vector = 1,
}

/// A soak's name in its line and its four verdicts.
struct Lines {
    name: &'static [u8],
    ok: &'static str,
    registers: &'static str,
    partner: &'static str,
    spawn: &'static str,
}

impl Unit {
    /// Writes `who`'s pattern into the whole register set.
    fn seed(self, who: Who) {
        match self {
            Self::Float => fpsoak::seed(who),
            Self::Vector => vsoak::seed(who),
        }
    }

    /// The registers that differ from `who`'s pattern (one bit per register).
    fn differs(self, who: Who) -> u64 {
        match self {
            Self::Float => fpsoak::differs(who),
            Self::Vector => vsoak::differs(who),
        }
    }

    /// What the soak's caller holds in registers of the unit that a call preserves — the
    /// pattern overwrites them. The vector unit has none (the convention preserves no
    /// vector register and no vector setting).
    fn keep(self) -> Option<fpsoak::Kept> {
        matches!(self, Self::Float).then(fpsoak::keep)
    }

    fn lines(self) -> Lines {
        match self {
            Self::Float => Lines {
                name: b"fpsoak",
                ok: markers::M_SELFTEST_FPSOAK_OK,
                registers: markers::M_SELFTEST_FPSOAK_FAIL_REGISTERS,
                partner: markers::M_SELFTEST_FPSOAK_FAIL_PARTNER,
                spawn: markers::M_SELFTEST_FPSOAK_FAIL_SPAWN,
            },
            Self::Vector => Lines {
                name: b"vsoak",
                ok: markers::M_SELFTEST_VSOAK_OK,
                registers: markers::M_SELFTEST_VSOAK_FAIL_REGISTERS,
                partner: markers::M_SELFTEST_VSOAK_FAIL_PARTNER,
                spawn: markers::M_SELFTEST_VSOAK_FAIL_SPAWN,
            },
        }
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

/// The partner thread; `unit` is the register set it soaks (`Unit as usize`).
extern "C" fn partner(unit: usize) {
    let unit = if unit == Unit::Vector as usize { Unit::Vector } else { Unit::Float };
    let go = PARTNER_GO.load(Ordering::Acquire);
    let done = PARTNER_DONE.load(Ordering::Acquire);
    let (mut pinned, mut differs, mut round) = (NO_CPU, 0u64, 0u64);
    unit.seed(Who::Partner);
    // Parked until the main thread hands a round over; past the last round (or with the
    // fence gone) the thread ends.
    while round < ROUNDS + BUSY_ROUNDS && fence_wait(go, round + 1, 0).is_ok() {
        round += 1;
        differs |= unit.differs(Who::Partner);
        // Written again: the unit is changed, so the next trap saves it.
        unit.seed(Who::Partner);
        if round > ROUNDS {
            compute();
            differs |= unit.differs(Who::Partner);
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

/// Spawns the partner holding its two fences. `None`: no thread, or no hand-over of the
/// capabilities — the partner, if it exists, has ended and is reaped.
fn spawn_partner(unit: Unit, go: Cap, done: Cap) -> Option<nexus_abi::Pid> {
    // SAFETY: the stack is used by the one thread spawned here; the soaks are sequential
    // (spawn → soak → join).
    let stack = unsafe { &mut *core::ptr::addr_of_mut!(PARTNER_STACK) };
    let pid = nexus_abi::thread::spawn_thread_suspended(partner, unit as usize, stack).ok()?;
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

/// One unit's soak; `true` = every register of both threads came back in every round.
fn soak(unit: Unit) -> bool {
    let lines = unit.lines();
    TARGET.store(NO_CPU, Ordering::Release);
    PARTNER_ROUNDS.store(0, Ordering::Release);
    PARTNER_DIFFERS.store(0, Ordering::Release);
    let home = sched::get_affinity().ok();
    let set = home.map_or(0, CpuSet::bits);
    let (Ok(go), Ok(done)) = (nexus_abi::fence_create(), nexus_abi::fence_create()) else {
        emit_line(lines.spawn);
        return false;
    };
    let Some(pid) = spawn_partner(unit, go, done) else {
        let _ = (nexus_abi::cap_close(go), nexus_abi::cap_close(done));
        emit_line(lines.spawn);
        return false;
    };

    let (mut differs, mut rounds, mut hops, mut stalled) = (0u64, 0u64, 0u32, false);
    let mut cpu = set.trailing_zeros().min(63);
    let kept = unit.keep();
    unit.seed(Who::Main);
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
            differs |= unit.differs(Who::Main);
            unit.seed(Who::Main);
        }
        stalled |= fence_wait(done, round, answer_by).is_err();
        differs |= unit.differs(Who::Main);
        unit.seed(Who::Main);
        rounds = round;
    }
    if let Some(kept) = &kept {
        fpsoak::give_back(kept);
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

    emit_bytes(lines.name);
    emit_bytes(b": rounds=");
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
        emit_line(lines.registers);
    } else if stalled || !complete || !reaped {
        emit_line(lines.partner);
    } else {
        emit_line(lines.ok);
        return true;
    }
    false
}

/// The soak of every unit the machine offers, and the proof of the answer that named them.
pub(crate) fn unit_soaks_proof() {
    let Ok(offers) = sched::offers() else {
        emit_bytes(markers::M_SELFTEST_HW_PROBE_FAIL.as_bytes());
        emit_line("no answer)");
        return;
    };
    let float = offers.has(Extension::Float);
    let vector_bytes = offers.vector_bytes();
    let mut held = true;
    if float {
        held &= soak(Unit::Float);
    } else {
        emit_line("fpsoak: the machine offers no floating-point unit");
    }
    // The width this thread reads from its own registers (0: none read — not offered, or
    // wider than a register the kernel keeps).
    let mut own_bytes = 0;
    match vector_bytes {
        Some(stated) => {
            own_bytes = vsoak::holds(stated);
            held &= own_bytes == stated && soak(Unit::Vector);
        }
        None => emit_line("vsoak: the machine offers no vector unit"),
    }
    if !float && vector_bytes.is_none() {
        // Nothing was named, so nothing was acted on: no verdict about the answer.
        emit_line("hw probe: the machine offers neither unit");
        return;
    }
    let verdict =
        if held { markers::M_SELFTEST_HW_PROBE_OK } else { markers::M_SELFTEST_HW_PROBE_FAIL };
    emit_bytes(verdict.as_bytes());
    emit_bytes(b"float=");
    emit_u64(u64::from(float));
    emit_bytes(b" vector=");
    emit_u64(u64::from(vector_bytes.is_some()));
    emit_bytes(b" vector_bytes=");
    emit_u64(vector_bytes.unwrap_or(0) as u64);
    emit_bytes(b" own_bytes=");
    emit_u64(own_bytes as u64);
    emit_bytes(b" word=0x");
    emit_hex_u64(offers.raw() as u64);
    emit_line(")");
}
