// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Secondary-hart HSM bring-up for the machine the tree describes (RFC-0118) — the
//! entry stub and the record each CPU enters through, the secondaries' stacks (frame pool, a
//! guard page below each), sequential start with settle-wait + bounded retry, and the
//! self-diagnosing KGATE boot gate.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP proofs (KINIT: cpuN online, KGATE: smp ...); the hart↔CPU maps and
//!   the firmware's hart masks are host-tested in `crate::topology`
//! INVARIANTS: a hart is started by its id and enters through its CPU's record — nothing here
//!   is indexed by a hart id; a lost hart (firmware-side: HSM STARTED but never at our entry)
//!   degrades the boot loudly instead of killing it; every CPU's bring-up stage is recorded
//!   for the gate; the entry is a physical address on the first start and on every retry.
//! ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md
//! RFC: docs/rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use nexus_sched::CpuSet;

use crate::frames::FrameError;
use crate::sync::hart_table::{HartTable, SizeError};
use crate::types::{CpuId, HartId};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use sbi_rt as sbi;

use super::{cpu_online, hart_local_prepare};

const PAGE: usize = crate::frames::FRAME_SIZE as usize;

// Must match the boot-stack budget: secondaries run the FULL syscall path
// (incl. ELF-loading spawns) on this stack once they serve the runtime (A3).
// 32 KiB was measured too small for the deepest syscall (see kernel.ld note).
const STACK_BYTES: usize = 64 * 1024;
/// The frames one secondary takes from the pool: its stack and the guard page below it.
const STACK_FRAMES: usize = STACK_BYTES / PAGE + 1;

const SBI_ERR_INVALID_PARAM: usize = (-3isize) as usize;
const SBI_ERR_ALREADY_AVAILABLE: usize = (-6isize) as usize;
const SBI_ERR_ALREADY_STARTED: usize = (-7isize) as usize;

/// How long the boot hart waits for one started hart before it goes on to the next.
const SETTLE_NS: u64 = 500_000_000;

/// What a secondary enters through, one per logical CPU. The firmware hands the stub this
/// record's PHYSICAL address as the HSM opaque value: the stub marks its arrival in it with
/// paging still off, then takes its stack from it at the high alias. The first two fields are
/// an ABI with the stub (offsets injected below).
#[repr(C)]
pub(super) struct HartEntry {
    /// Set by the stub's FIRST instructions. Distinguishes the two failure modes behind
    /// `stage=0`: `asm=0` ⇒ the firmware reported STARTED but never transferred control to
    /// us; `asm=1, stage=0` ⇒ we were entered and died inside the stub (sp/gp/satp).
    /// An atomic, so the record is never placed read-only and the store is never dropped.
    seen: AtomicU64,
    /// The stack this CPU adopts (high alias); 0 until its stack is reserved.
    stack_top: AtomicUsize,
    /// The logical CPU this record belongs to.
    cpu: AtomicUsize,
    /// 0=never entered Rust, 1=entry, 2=hart-local prepared, 3=trap vector installed, 4=online.
    stage: AtomicUsize,
    /// The hart id the firmware entered with (evidence for the gate; `u64::MAX` = never).
    entered_as: AtomicU64,
    /// The physical address of the guard page below the stack; 0 = no stack reserved.
    guard: AtomicUsize,
}

impl HartEntry {
    const fn new() -> Self {
        Self {
            seen: AtomicU64::new(0),
            stack_top: AtomicUsize::new(0),
            cpu: AtomicUsize::new(0),
            stage: AtomicUsize::new(0),
            entered_as: AtomicU64::new(u64::MAX),
            guard: AtomicUsize::new(0),
        }
    }
}

/// CPU 0's record is never entered through (the boot hart came in at `_start`); it is there
/// so the table is indexed by CPU like every other.
static ENTRIES: HartTable<HartEntry> = HartTable::new(HartEntry::new());

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
core::arch::global_asm!(
    r#"
    .section .text.__secondary_hart_start, "ax", @progbits
    .globl __secondary_hart_start
    .type  __secondary_hart_start, @function
    .align 4
__secondary_hart_start:
    /* SBI HSM contract: a0 = hart id, a1 = opaque. The opaque value is the PHYSICAL address
       of this CPU's entry record (paging is off here). */
    /* TASK-0306: record arrival BEFORE touching sp/gp. `stage=0` alone cannot
       distinguish "SBI never jumped here" from "jumped and died in this stub",
       and those have opposite fixes. This store uses only a1 + a scratch reg
       and no stack, so it is valid at the very first instruction. */
    li    t2, 1
    sd    t2, {seen}(a1)
    fence w, w
    /* RFC-0098 C4: this stub runs at its physical address with paging off,
       so every symbol above is `lla` (PC-relative); the GOT holds high
       addresses. Switch to the boot table the boot hart published, jump to
       the high alias, and only then touch sp/gp and Rust. */
    lla   t0, SECONDARY_BOOT_SATP
    ld    t0, 0(t0)
    csrw  satp, t0
    sfence.vma x0, x0
    lla   t0, 1f
    li    t1, {phys_offset}
    add   t0, t0, t1
    jr    t0
1:  li    t1, {phys_offset}
    add   a1, a1, t1
    ld    sp, {stack_top}(a1)
    .option push
    .option norelax
    la    gp, __global_pointer$
    .option pop
    tail  __secondary_hart_rust
    .size __secondary_hart_start, .-__secondary_hart_start
"#,
    phys_offset = const crate::phys::PHYS_OFFSET,
    seen = const core::mem::offset_of!(HartEntry, seen),
    stack_top = const core::mem::offset_of!(HartEntry, stack_top),
);

/// The boot table's `satp`, for the secondaries' switch: written by the boot
/// hart before any `hart_start`, read by the stub above PC-relatively.
#[no_mangle]
pub static SECONDARY_BOOT_SATP: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
extern "C" {
    fn __secondary_hart_start();
}

#[no_mangle]
extern "C" fn __secondary_hart_rust(hart: usize, entry: &'static HartEntry) -> ! {
    entry.entered_as.store(hart as u64, Ordering::Release);
    let cpu = CpuId::from_raw(entry.cpu.load(Ordering::Acquire) as u16);
    crate::cpu_main::kmain_secondary(cpu, entry.stack_top.load(Ordering::Acquire))
}

/// Sizes the entry records for a machine of `harts` CPUs.
pub(super) fn size(harts: usize) -> Result<(), SizeError> {
    ENTRIES.size(harts, HartEntry::new)
}

/// Reserves each secondary's stack: `STACK_BYTES` from the frame pool with one more frame
/// below it that the kernel's address space leaves unmapped ([`stack_guards`]) — an overflow
/// faults instead of running into whatever the pool handed out next. Once, by the boot hart,
/// before the kernel's address space is built.
pub(super) fn reserve_stacks(harts: usize) -> Result<(), FrameError> {
    let order = STACK_FRAMES.next_power_of_two().ilog2() as u8;
    for cpu in 1..harts {
        let Some(entry) = ENTRIES.get(cpu) else { continue };
        if entry.stack_top.load(Ordering::Acquire) != 0 {
            continue;
        }
        let block = crate::mm::frame_pool::alloc(order)?;
        // The frames above the stack go back to the pool; the kept ones are never freed.
        let _kept = crate::mm::frame_pool::trim(block, STACK_FRAMES);
        let guard = block.base as usize;
        entry.cpu.store(cpu, Ordering::Release);
        entry.guard.store(guard, Ordering::Release);
        entry
            .stack_top
            .store(crate::phys::phys_to_virt(guard + STACK_FRAMES * PAGE), Ordering::Release);
    }
    Ok(())
}

/// The guard pages below the secondaries' stacks as physical `(start, end)` ranges: the holes
/// the kernel's direct map leaves (`mm::kernel_layout`).
pub fn stack_guards() -> impl Iterator<Item = (usize, usize)> {
    ENTRIES
        .iter()
        .map(|entry| entry.guard.load(Ordering::Acquire))
        .filter(|&guard| guard != 0)
        .map(|guard| (guard, guard + PAGE))
}

/// The CPU whose secondary stack holds `sp` (the identity fallback when `tp` is not a
/// hart-local block).
pub(super) fn cpu_of_stack(sp: usize) -> Option<CpuId> {
    ENTRIES.iter().enumerate().skip(1).find_map(|(cpu, entry)| {
        let top = entry.stack_top.load(Ordering::Acquire);
        (top != 0 && sp > top - STACK_BYTES && sp <= top).then(|| CpuId::from_raw(cpu as u16))
    })
}

/// Records how far `cpu` got in its bring-up (see [`HartEntry::stage`]).
pub fn note_bringup_stage(cpu: CpuId, stage: usize) {
    if let Some(entry) = ENTRIES.get(cpu.as_index()) {
        entry.stage.store(stage, Ordering::Release);
    }
}

/// Boot gate (self-diagnosing evidence): one line per expected secondary with the
/// full bring-up picture — its hart, online bit, arrival in the stub, reached stage, HSM
/// state. Emitted unconditionally on boots that expect more than one CPU so ANY failure
/// localizes itself without re-instrumented reruns.
pub fn emit_bringup_gate(expected: CpuSet) {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let machine = crate::topology::machine();
        if let Some(machine) = machine.filter(|m| m.runnable() > m.count()) {
            log_error!(
                target: "smp",
                "KGATE: smp harts beyond the cpu set (named={} run={})",
                machine.runnable(),
                machine.count()
            );
        }
        if expected.count() <= 1 {
            return;
        }
        let online = cpu_online();
        for cpu in expected.iter().filter(|&cpu| cpu != 0) {
            let (Some(entry), Some(hart)) = (ENTRIES.get(cpu), crate::topology::hart_of(cpu))
            else {
                continue;
            };
            let hart = HartId::from_raw(hart);
            let hsm = sbi::hart_get_status(hart.as_sbi()).value;
            let seen = entry.seen.load(Ordering::Acquire);
            let stage = entry.stage.load(Ordering::Acquire);
            let entered_as = entry.entered_as.load(Ordering::Acquire);
            if entered_as != u64::MAX && entered_as != hart.as_raw() {
                log_error!(
                    target: "smp",
                    "KGATE: smp cpu{} entered by hart {} (the tree names hart {})",
                    cpu,
                    entered_as,
                    hart
                );
            }
            log_info!(
                target: "smp",
                "KGATE: smp cpu{} hart={} online={} asm={} stage={} hsm={}",
                cpu,
                hart,
                online.contains(cpu) as usize,
                seen,
                stage,
                hsm
            );
        }
        if expected.without(online).is_empty() {
            log_info!(target: "smp", "KGATE: smp bringup ok mask=0x{:x}", online);
        } else {
            log_error!(
                target: "smp",
                "KGATE: smp bringup DEGRADED expected=0x{:x} got=0x{:x}",
                expected,
                online
            );
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = expected;
}

/// Asks the firmware to start `cpu`'s hart at the stub's PHYSICAL address (the firmware
/// starts a hart with paging off — on the first try and on every retry), with the CPU's entry
/// record as the opaque value. `None`: the CPU has no hart, record or stack.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn start_hart(cpu: usize) -> Option<sbi::SbiRet> {
    let hart = HartId::from_raw(crate::topology::hart_of(cpu)?);
    let entry = ENTRIES.get(cpu)?;
    let stack_top = entry.stack_top.load(Ordering::Acquire);
    if stack_top == 0 {
        return None;
    }
    // Prepare the hart-local block BEFORE the hart can start executing:
    // hart_start is asynchronous, and the secondary's trap install reads this block.
    hart_local_prepare(CpuId::from_raw(cpu as u16), stack_top);
    SECONDARY_BOOT_SATP.store(crate::phys::boot_satp(), Ordering::Release);
    let stub = crate::phys::virt_to_phys(__secondary_hart_start as usize);
    let record = crate::phys::virt_to_phys(entry as *const HartEntry as usize);
    Some(sbi::hart_start(hart.as_sbi(), stub, record))
}

/// Starts the machine's secondary harts via SBI HSM and returns the CPUs expected online:
/// every CPU of the machine — a hart the tree names and the firmware refuses is a degraded
/// boot the gate reports, not a smaller expectation. Which harts there are is the tree's: a
/// cpu node it disables (or whose hart has no MMU) is not a logical CPU and is never started
/// — a deliberate single-hart boot on a board reads that way.
pub fn start_secondary_harts() -> CpuSet {
    let expected = crate::topology::runs();

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        if let Some(machine) = crate::topology::machine().filter(|m| m.named() > m.runnable()) {
            log_info!(
                target: "smp",
                "KINIT: {} of the tree's {} harts are not started (disabled or without an mmu)",
                machine.named() - machine.runnable(),
                machine.named()
            );
        }
        for cpu in 1..crate::topology::hart_count() {
            let hart = crate::topology::hart_of(cpu).unwrap_or(u64::MAX);
            let Some(ret) = start_hart(cpu) else {
                log_error!(target: "smp", "KINIT: cpu{} has no entry record — not started", cpu);
                continue;
            };
            // A firmware that refuses one hart of the tree says nothing about the next: each
            // failure is named and the loop goes on.
            match ret.error {
                0 | SBI_ERR_ALREADY_AVAILABLE | SBI_ERR_ALREADY_STARTED => {
                    // SEQUENTIAL bring-up: wait for this hart to come online
                    // before starting the next. Batched hart_start requests
                    // were observed to lose a hart nondeterministically (HSM
                    // reports STARTED but the hart never reaches the kernel
                    // entry; restart returns ALREADY_AVAILABLE).
                    let one = CpuSet::EMPTY.with(cpu);
                    if !wait_for_online(one, SETTLE_NS) {
                        log_error!(
                            target: "smp",
                            "KINIT: cpu{} hart {} did not come online (sequential bring-up)",
                            cpu,
                            hart
                        );
                    }
                }
                SBI_ERR_INVALID_PARAM => {
                    log_error!(
                        target: "smp",
                        "KINIT: cpu{} hart {} unknown to the firmware — not started",
                        cpu,
                        hart
                    );
                }
                error => {
                    log_error!(
                        target: "smp",
                        "KINIT: cpu{} hart {} start failed err=0x{:x}",
                        cpu,
                        hart,
                        error
                    );
                }
            }
        }
    }

    expected
}

/// Bounded bring-up retry: re-issues `hart_start` for every expected CPU
/// that has not come online (HSM start requests issued in quick succession
/// were observed to get lost nondeterministically — a hart never reached the
/// kernel entry despite a success return). Logs the SBI HSM status of each
/// missing hart for the evidence trail.
pub fn retry_missing_harts(expected: CpuSet) {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        for cpu in expected.without(cpu_online()).iter() {
            let (Some(entry), Some(hart)) = (ENTRIES.get(cpu), crate::topology::hart_of(cpu))
            else {
                continue;
            };
            let status = sbi::hart_get_status(HartId::from_raw(hart).as_sbi());
            log_error!(
                target: "smp",
                "KINIT: cpu{} hart {} missing (asm={} stage={} hsm_err=0x{:x} hsm_state={}) — retrying start",
                cpu,
                hart,
                entry.seen.load(Ordering::Acquire),
                entry.stage.load(Ordering::Acquire),
                status.error,
                status.value
            );
            if let Some(ret) = start_hart(cpu) {
                log_info!(target: "smp", "KINIT: cpu{} hart {} restart err=0x{:x}", cpu, hart, ret.error);
            }
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = expected;
}

/// Poll interval while waiting for a hart: 1 ms of the platform's timebase
/// (was the literal `10_000` ticks — 1 ms only at QEMU's 10 MHz). Short enough
/// that bring-up latency is unaffected, long enough that the emulator gets a
/// real slice to run the hart we are waiting for.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn online_poll_ticks() -> u64 {
    crate::hal::platform::ns_to_ticks(1_000_000)
}

/// Parks this hart for ~[`online_poll_ticks`] instead of spinning.
///
/// TASK-0306. The old loop called `spin_loop()`, which under TCG burns the
/// boot hart's entire scheduler quantum — starving the very vCPU it is
/// waiting for. That is why a *different* hart went missing on each boot
/// (`KGATE: smp bringup DEGRADED expected=0xf got=0x7`, then `got=0xb`), and
/// why it got worse under host load. `kmain_secondary` already parks in WFI
/// for exactly this reason ("under icount/TCG a spinning vCPU steals whole
/// scheduler quanta"); the bring-up wait never got the same treatment.
///
/// Deadlock-free by construction: the timer is armed HERE, so a wake always
/// arrives even though no scheduler timer exists yet at this point in kmain
/// (`start_secondary_harts` runs ~180 lines before `KINIT: timer sbi`).
/// `sstatus.SIE` stays clear throughout, so the timer only ever *wakes* WFI —
/// it is never taken as a trap into a half-built scheduler. A pending-but-
/// masked interrupt still releases WFI (the same RISC-V property the
/// secondary park loop relies on).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn park_until_poll() {
    let next = (riscv::register::time::read() as u64).saturating_add(online_poll_ticks());
    crate::hal::platform::arm_timer_ticks(next);
    // SAFETY: enables the timer bit in `sie` and executes WFI with
    // `sstatus.SIE` clear (the caller cleared it for the whole wait). The
    // pending timer releases WFI without being taken as a trap.
    unsafe {
        riscv::register::sie::set_stimer();
        core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
    }
}

/// Waits (bounded by TIME, not iterations) until every CPU of `expected` is
/// online. Iteration budgets are meaningless across icount/MTTCG — 2M
/// spin_loops are microseconds under MTTCG, which flakily timed out hart 3
/// on SMP=4 bring-up.
pub fn wait_for_online(expected: CpuSet, budget_ns: u64) -> bool {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        // The budget in the platform's ticks (was `budget_ns / 100`: QEMU's 10 MHz only —
        // on a 24 MHz board that cut every wait to 42 %).
        let deadline = (riscv::register::time::read() as u64)
            .saturating_add(crate::hal::platform::ns_to_ticks(budget_ns));
        // Keep traps masked for the whole wait: we want the timer as a WFI
        // wake source only, never as a trap into a scheduler that does not
        // exist yet. Restored before returning.
        let sie_was_set = riscv::register::sstatus::read().sie();
        // SAFETY: masking S-mode interrupts around a bounded wait; restored
        // on every return path below.
        unsafe {
            riscv::register::sstatus::clear_sie();
        }
        let result = loop {
            if expected.without(cpu_online()).is_empty() {
                break true;
            }
            if (riscv::register::time::read() as u64) >= deadline {
                break false;
            }
            // The bring-up phase can be long without any other progress
            // signal; keep the liveness watchdog informed so a slow hart
            // start cannot be mistaken for a wedged kernel (the documented
            // "watchdog: no progress right after cpu0 sched loop" class).
            crate::liveness::bump();
            park_until_poll();
        };
        // Disarm: leave no stale compare behind for the real scheduler timer.
        crate::hal::platform::arm_timer_ticks(u64::MAX);
        if sie_was_set {
            // SAFETY: restoring the caller's interrupt-enable state.
            unsafe {
                riscv::register::sstatus::set_sie();
            }
        }
        result
    }
    #[allow(unreachable_code)]
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = budget_ns;
        expected.without(cpu_online()).is_empty()
    }
}
