// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: External-interrupt (PLIC) → userspace-endpoint routing. Lets a
//! userspace driver block on a device IRQ instead of polling: the kernel claims
//! the IRQ, delivers a notification to the bound endpoint, wakes the waiter, and
//! leaves the source masked at the PLIC until the driver services the device and
//! calls `irq_complete`. A line is taken on the hart its driver runs on (RFC-0118,
//! TASK-0330 P8): it is enabled in one context — its binder's home CPU's — and follows
//! that task when it is pinned elsewhere or migrates; the rules are `crate::irq_route`'s
//! (host-proven), this module keeps one route per line and carries its steps out.
//! OWNERS: @kernel-hal-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the route rules on the host (`crate::irq_route`); the path by every boot's
//!   drivers and `KSELFTEST: irq off boot hart ok` on a lane with more than one hart
//! INVARIANTS: no allocation beyond a small notification Vec; a claimed IRQ stays
//!   masked until completed (level-triggered storm-safe); delivery runs only with
//!   exclusive borrows (U-mode S_EXT trap, a syscall, or the idle loop under the
//!   BKL) — an S-mode-interrupted hart claims into its per-hart stash instead and
//!   its next `dispatch_external` delivers (`stash_undelivered`); a claim and its record,
//!   a route's change and its controller writes each happen under `LINES` (taken with
//!   interrupts off, nothing is taken inside it).

extern crate alloc;

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::hal::plic::{self, IrqId, MAX_IRQ};
use crate::ipc::{self, EndpointId};
use crate::irq_route::{self, Cpu, Route, Step, Steps};
use crate::sched::Scheduler;
use crate::sync::spin_irq::SpinIrqLock;
use crate::task;
use crate::types::CpuId;

/// IPC op byte for an IRQ-fired notification (payload: 4-byte LE source id).
pub const OP_IRQ_FIRED: u8 = 0x70;

const TABLE_LEN: usize = (MAX_IRQ as usize) + 1;

/// `irq source id -> bound endpoint id` (0 = unbound). Index 0 is unused (the
/// PLIC "no interrupt" sentinel). Atomics so a bind from a syscall is visible to
/// the S_EXT trap handler without a lock.
static IRQ_ENDPOINT: [AtomicU32; TABLE_LEN] = {
    const UNBOUND: AtomicU32 = AtomicU32::new(0);
    [UNBOUND; TABLE_LEN]
};

/// Where every line is at the controller, and whose home it follows.
struct Lines {
    route: [Route; TABLE_LEN],
    /// The task that bound the line, until the line's endpoint dies (`forget_endpoint`).
    owner: [Option<u32>; TABLE_LEN],
}

/// One lock for the routes and for every enable-word write they cause: the controller's
/// enable words are read-modify-write and shared by the lines of a context. A leaf lock —
/// taken in the external-interrupt trap as well, hence with interrupts off.
static LINES: SpinIrqLock<Lines> =
    SpinIrqLock::new(Lines { route: [Route::UNBOUND; TABLE_LEN], owner: [None; TABLE_LEN] });

/// Which tasks ever bound a line, by `pid mod 256` — a filter in front of `rehome`'s scan,
/// so a task that owns no line costs a migration one load (a false hit costs one scan).
static BINDERS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

fn binder_bit(owner: task::Pid) -> (usize, u64) {
    let slot = owner.as_raw() as usize % 256;
    (slot / 64, 1u64 << (slot % 64))
}

fn cpu_of(cpu: Cpu) -> CpuId {
    CpuId::from_raw(u16::from(cpu))
}

fn here() -> Cpu {
    Cpu::try_from(crate::smp::cpu_current_id().as_index()).unwrap_or(Cpu::MAX)
}

/// The CPU that takes a line whose driver is homed on `home` (`irq_route::target` on this
/// machine: online, and the tree names a supervisor context for it).
fn target_for(home: CpuId) -> Option<Cpu> {
    irq_route::target(
        Cpu::try_from(home.as_index()).unwrap_or(Cpu::MAX),
        crate::smp::cpu_online().bits(),
        |cpu| crate::hal::platform::plic_s_context(usize::from(cpu)).is_some(),
    )
}

/// Tells the controller what a route decided. Called with `LINES` held.
fn carry_out(irq: IrqId, steps: Steps) {
    for step in steps.iter() {
        match step {
            Step::Enable(cpu) => plic::enable_source(irq, cpu_of(cpu)),
            Step::Disable(cpu) => plic::disable_source(irq, cpu_of(cpu)),
            Step::Complete(cpu) => plic::complete(irq, cpu_of(cpu)),
        }
    }
}

/// Binds `irq` to `endpoint` for the task `owner` and enables the line in the context of the
/// CPU that takes it for `home` (the binder's home CPU). Idempotent; a later bind re-points
/// the line. `None`: no online CPU has a controller context — the line cannot fire and
/// nothing was bound.
#[must_use]
pub fn bind(irq: IrqId, endpoint: EndpointId, owner: task::Pid, home: CpuId) -> Option<CpuId> {
    let cpu = target_for(home)?;
    let id = irq.raw() as usize;
    let (word, bit) = binder_bit(owner);
    BINDERS[word].fetch_or(bit, Ordering::AcqRel);
    IRQ_ENDPOINT[id].store(endpoint, Ordering::Release);
    let mut lines = LINES.lock();
    lines.owner[id] = Some(owner.as_raw());
    let steps = lines.route[id].want(Some(cpu));
    carry_out(irq, steps);
    Some(cpu_of(cpu))
}

/// The task `owner` is homed on `home` now (a pin or a migration): every line it bound
/// follows — at once, or at its completion when a claim is outstanding.
pub fn rehome(owner: task::Pid, home: CpuId) {
    let (word, bit) = binder_bit(owner);
    if BINDERS[word].load(Ordering::Acquire) & bit == 0 {
        return;
    }
    let cpu = target_for(home);
    let key = Some(owner.as_raw());
    let last = (crate::hal::platform::plic_ndev() as usize).min(MAX_IRQ as usize);
    let mut lines = LINES.lock();
    for id in 1..=last {
        if lines.owner[id] != key {
            continue;
        }
        if let Some(irq) = IrqId::new(id as u32) {
            let steps = lines.route[id].want(cpu);
            carry_out(irq, steps);
        }
    }
}

/// Drops every binding that targets `endpoint` — the endpoint died (TASK-0333 6a), and a
/// line firing into its slot's next tenant would be a message nobody sent. The line is
/// released at the controller: a claim its dead driver can no longer complete is completed,
/// the line disabled — the driver's successor binds a line that can fire. Returns the lines.
pub fn forget_endpoint(endpoint: EndpointId) -> usize {
    let mut dropped = 0;
    for (id, slot) in IRQ_ENDPOINT.iter().enumerate().skip(1) {
        if slot.compare_exchange(endpoint, 0, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            dropped += 1;
            let Some(irq) = IrqId::new(id as u32) else {
                continue;
            };
            let mut lines = LINES.lock();
            lines.owner[id] = None;
            let steps = lines.route[id].release();
            carry_out(irq, steps);
        }
    }
    dropped
}

/// Returns the endpoint bound to `irq`, if any.
#[must_use]
pub fn binding(irq: IrqId) -> Option<EndpointId> {
    match IRQ_ENDPOINT[irq.raw() as usize].load(Ordering::Acquire) {
        0 => None,
        ep => Some(ep),
    }
}

/// Completes `irq` at the PLIC so it can fire again. A driver calls this after it
/// has cleared the device's interrupt condition (drained the virtqueue). The
/// completion goes to the context that claimed the line — the syscall may run on any
/// hart — and a move that waited for it follows.
pub fn complete(irq: IrqId) {
    let mut lines = LINES.lock();
    let steps = lines.route[irq.raw() as usize].complete();
    carry_out(irq, steps);
}

/// Claims this hart's next pending line and records where — one step under `LINES`, so a
/// move decided on another hart either sees the claim or is carried out before it.
fn claim_here(cpu: Cpu) -> Option<IrqId> {
    let mut lines = LINES.lock();
    let irq = plic::claim()?;
    // The console's line is the kernel's (RFC-0107): never routed, completed by its handler.
    if crate::hal::platform::console_irq() != Some(irq) {
        lines.route[irq.raw() as usize].claimed(cpu);
    }
    Some(irq)
}

/// A claimed line nobody is bound to: completed, then disabled — it cannot storm, and a
/// later bind finds a line that can fire.
fn quarantine(irq: IrqId, cpu: Cpu) {
    let id = irq.raw() as usize;
    let mut lines = LINES.lock();
    lines.owner[id] = None;
    let steps = lines.route[id].quarantine(cpu);
    carry_out(irq, steps);
}

fn irq_payload(irq: IrqId) -> [u8; 4] {
    irq.raw().to_le_bytes()
}

/// Per-hart stash of sources claimed by an S-mode-interrupted hart (its
/// scheduler idle loop, where this very hart may hold the BKL, so delivery
/// cannot run inside the trap). A claimed source stays MASKED at the PLIC —
/// no level storm — and this hart's next `dispatch_external` (idle loop /
/// timer backstop / U-mode S_EXT) delivers it. Bitmask over the source ids
/// (`MAX_IRQ` = 1023, the PLIC's ceiling, is sixteen words).
///
/// Why not `drain_undelivered` once the runtime runs: completing a bound level
/// source without delivery re-asserts it immediately, so a hart that is
/// legitimately idle in S-mode (every blocking syscall now parks the hart
/// instead of spinning in U-mode — `park_hart_or_self_wake`) took the S_EXT
/// trap back-to-back and never reached its idle loop's `dispatch_external`:
/// the block-plane IRQ was never delivered (`init: volume spawn FAIL
/// reason=query`, 2026-09-08).
const STASH_WORDS: usize = (TABLE_LEN + 63) / 64;
type Stash = [AtomicU64; STASH_WORDS];

const fn empty_stash() -> Stash {
    [const { AtomicU64::new(0) }; STASH_WORDS]
}

/// One stash per CPU; the boot hart's is the static's own (a trap may stash before any
/// allocator runs), the others come with the machine (`smp::size_for_machine`).
static STASHED: crate::sync::hart_table::HartTable<Stash> =
    crate::sync::hart_table::HartTable::new(empty_stash());

/// Sizes the stashes for a machine of `harts` CPUs.
pub fn size(harts: usize) -> Result<(), crate::sync::hart_table::SizeError> {
    STASHED.size(harts, empty_stash)
}

/// Whether `cpu` holds lines it claimed and has not delivered — its idle loop delivers them
/// before it sleeps.
#[must_use]
pub fn has_stash(cpu: CpuId) -> bool {
    STASHED
        .get(cpu.as_index())
        .is_some_and(|stash| stash.iter().any(|word| word.load(Ordering::Acquire) != 0))
}

/// Claims every pending source for this hart's context WITHOUT completing it
/// and records it in the hart's stash (bound sources) or quarantines it
/// (unbound: completed + masked). Used on the S-mode external trap once the
/// runtime runs; the idle loop's `dispatch_external` delivers the stash. The hart is
/// told not to sleep on it: the claim took the line's pending bit, so nothing else would
/// wake a hart that reaches its `wfi` right after this trap.
pub fn stash_undelivered() {
    let me = crate::smp::cpu_current_id();
    let Some(stash) = STASHED.get(me.as_index()) else {
        drain_undelivered();
        return;
    };
    let cpu = here();
    let mut stashed = false;
    for _ in 0..TABLE_LEN {
        let Some(irq) = claim_here(cpu) else {
            break;
        };
        if crate::hal::platform::console_irq_fired(irq) {
            continue;
        }
        let id = irq.raw() as usize;
        if binding(irq).is_some() {
            stash[id / 64].fetch_or(1u64 << (id % 64), Ordering::AcqRel);
            stashed = true;
        } else {
            quarantine(irq, cpu);
        }
    }
    if stashed {
        crate::smp::set_wake_hint(me);
    }
}

/// Takes this hart's stash (claimed, undelivered, still masked sources).
fn take_stashed(cpu: usize) -> [u64; STASH_WORDS] {
    let mut out = [0u64; STASH_WORDS];
    if let Some(stash) = STASHED.get(cpu) {
        for (slot, word) in out.iter_mut().zip(stash) {
            *slot = word.swap(0, Ordering::AcqRel);
        }
    }
    out
}

/// Claims and immediately completes every pending source without delivery, used
/// before the runtime is installed so a stray assertion cannot storm. Does NOT
/// mask bound sources (delivery resumes on the next U-mode trap). Bounded by the
/// source count.
pub fn drain_undelivered() {
    let cpu = here();
    for _ in 0..TABLE_LEN {
        let Some(irq) = claim_here(cpu) else {
            break;
        };
        // Unbound sources are masked so they cannot re-assert into a storm; bound
        // sources are left enabled (just completed) so a later U-mode trap can
        // deliver them.
        if binding(irq).is_none() {
            quarantine(irq, cpu);
        } else {
            complete(irq);
        }
    }
}

/// Drains all pending external interrupts for our context, delivering each to its
/// bound endpoint (and waking a blocked driver). Unbound sources are quarantined
/// (completed + masked) so a stray device cannot storm the CPU.
///
/// Caller contract: invoked with exclusive borrows — the S_EXT/timer trap with
/// a USER-mode interrupted context, or the scheduler idle loop under the BKL —
/// so `router`/`tasks`/`scheduler` are the unique live borrows. Delivers this
/// hart's stash (sources already claimed by an S-mode trap) first.
pub fn dispatch_external(
    router: &mut ipc::Router,
    tasks: &mut task::TaskTable,
    scheduler: &mut Scheduler,
) {
    let cpu = here();
    let stashed = take_stashed(usize::from(cpu));
    for (w, word) in stashed.iter().enumerate() {
        let mut bits = *word;
        while bits != 0 {
            let bit = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let Some(irq) = IrqId::new((w * 64 + bit) as u32) else {
                continue;
            };
            match binding(irq) {
                Some(ep) => deliver(irq, ep, router, tasks, scheduler),
                // Unbound since the claim: quarantine like a fresh claim.
                None => quarantine(irq, cpu),
            }
        }
    }
    // Bound: at most MAX_IRQ claims (each claimed source is masked until
    // completed, so the loop cannot spin on a re-asserting level IRQ).
    for _ in 0..TABLE_LEN {
        let Some(irq) = claim_here(cpu) else {
            break;
        };
        if crate::hal::platform::console_irq_fired(irq) {
            continue;
        }
        match binding(irq) {
            Some(ep) => deliver(irq, ep, router, tasks, scheduler),
            // No driver: complete + mask so it cannot re-fire indefinitely.
            None => quarantine(irq, cpu),
        }
    }
}

/// The first line a CPU other than the boot hart delivered is named once (the proof that
/// interrupts are served where their drivers run).
static OFF_BOOT_NAMED: AtomicBool = AtomicBool::new(false);

/// Delivers one claimed source to its bound endpoint and wakes the waiter.
/// The source stays masked (no complete) until the driver services the device
/// and calls `irq_complete` — prevents a level storm.
fn deliver(
    irq: IrqId,
    ep: EndpointId,
    router: &mut ipc::Router,
    tasks: &mut task::TaskTable,
    scheduler: &mut Scheduler,
) {
    let payload = irq_payload(irq);
    let header =
        ipc::header::MessageHeader::new(0, ep, OP_IRQ_FIRED as u16, 0, payload.len() as u32);
    let msg = ipc::Message::new(header, ipc::Payload::from_slice(&payload), None);
    if router.send(ep, msg).is_ok() {
        if let Ok(Some(waiter)) = router.pop_recv_waiter(ep) {
            let _ = tasks.wake(task::Pid::from_raw(waiter), scheduler);
        }
        let cpu = crate::smp::cpu_current_id();
        if !cpu.is_boot() && !OFF_BOOT_NAMED.swap(true, Ordering::AcqRel) {
            log_info!(
                target: "smp",
                "KSELFTEST: irq off boot hart ok (cpu={} irq={})",
                cpu.as_index(),
                irq.raw()
            );
        }
    }
}
