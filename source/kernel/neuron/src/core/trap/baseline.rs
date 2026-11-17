// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the SMP baseline's lines at the boot-end fence (TASK-0330 P0): where the kernel
//! lock's time went in the steady window, split by the subsystems its decomposition follows
//! (`lock_share`), and how long a runnable task waited for a hart, per class
//! (`ready_stats`). Printing only — the accounting and its rules are host-tested in those
//! modules. Read by the board cycle and the QEMU lanes; no gate judges them yet (the budgets
//! per lock class and the wake-to-run gate arrive with TASK-0330 P4 and TASK-0363).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `lock_share`, `ready_stats` (host); the lines on every QEMU lane's console
//! ADR: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md

use crate::lock_share::{self, Group};

const CLASS_NAMES: [&str; crate::ready_stats::CLASSES] = ["idle", "normal", "interactive", "burst"];

/// Logs the window's lock share and ready latency. Called under the lock by the telemetry
/// syscall: its own hold is not released yet and so is not in the sums it prints.
pub fn log_smp_baseline() {
    let tpu = super::budgets::ticks_per_us();
    let now = riscv::register::time::read() as u64;
    let window_ms = now.saturating_sub(lock_share::window_start()) / (tpu.max(1) * 1_000);
    let (us, takes) = lock_share::by_group(tpu);
    let g = |group: Group| us[group as usize];
    let n = |group: Group| takes[group as usize];
    log_info!(
        target: "smp",
        "KINIT: bkl held us (window_ms={} sched={} ipc={} mm={} task={} caps={} misc={} timer={} ipi={} ext={} fault={} idle={})",
        window_ms,
        g(Group::Sched),
        g(Group::Ipc),
        g(Group::Mm),
        g(Group::Task),
        g(Group::Caps),
        g(Group::Misc),
        g(Group::Timer),
        g(Group::Ipi),
        g(Group::External),
        g(Group::Fault),
        g(Group::Idle)
    );
    log_info!(
        target: "smp",
        "KINIT: bkl takes (sched={} ipc={} mm={} task={} caps={} misc={} timer={} ipi={} ext={} fault={} idle={})",
        n(Group::Sched),
        n(Group::Ipc),
        n(Group::Mm),
        n(Group::Task),
        n(Group::Caps),
        n(Group::Misc),
        n(Group::Timer),
        n(Group::Ipi),
        n(Group::External),
        n(Group::Fault),
        n(Group::Idle)
    );
    // TASK-0363 P2 S3: the idle loop's holds by what they are for, and the boot hart's delivery
    // by its two parts (a step that does not move its holder's share is reverted — ADR-0098).
    let held = |id| lock_share::holder(id, tpu);
    let (dispatch, steal, deliver) =
        (held(lock_share::IDLE), held(lock_share::IDLE_STEAL), held(lock_share::IDLE_DELIVER));
    log_info!(
        target: "smp",
        "KINIT: bkl idle split (dispatch={}us/{} steal={}us/{} deliver={}us/{} events_us={} walk_us={})",
        dispatch.0,
        dispatch.1,
        steal.0,
        steal.1,
        deliver.0,
        deliver.1,
        lock_share::deliver_us(lock_share::DeliverPart::Events, tpu),
        lock_share::deliver_us(lock_share::DeliverPart::DeadlineWalk, tpu)
    );
    // TASK-0363 P4: timer interrupts that woke an idle hart, those that found nothing due, and of
    // those the safety nets (the boot hart's heartbeat among them); the gate is the busiest
    // hart's empty wakes (<= 2 a second); `max_stray` the most on one hart that were not its net
    // (a tick into the idle loop, a deadline another hart served).
    // With them the window's steals: an idle hart that no longer ticks takes work when a wake
    // queues it behind a busy hart (`task::wake::kick_idle_thief`).
    static LAST_STEALS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    let w = crate::smp::take_idle_timer_window();
    let steals = crate::smp::work_steal_count();
    let last = LAST_STEALS.swap(steals, core::sync::atomic::Ordering::Relaxed);
    log_info!(
        target: "smp",
        "KINIT: idle timer wakes (window_ms={} total={} empty={} net={} stale={} dry={} max_empty={} at=cpu{} max_stray={} steals={})",
        window_ms,
        w.wakes,
        w.empty,
        w.net,
        w.stale,
        w.dry,
        w.max_empty,
        w.at,
        w.max_stray,
        steals.saturating_sub(last)
    );
    let top = lock_share::top(tpu);
    log_info!(
        target: "smp",
        "KINIT: bkl top sum id={}:{}us/{} id={}:{}us/{} id={}:{}us/{}",
        top[0].0,
        top[0].1,
        top[0].2,
        top[1].0,
        top[1].1,
        top[1].2,
        top[2].0,
        top[2].1,
        top[2].2
    );
    for (class, name) in CLASS_NAMES.iter().enumerate() {
        let r = crate::ready_stats::report(class, tpu);
        if r.count == 0 {
            continue;
        }
        log_info!(
            target: "smp",
            "KINIT: ready latency class={} n={} mean_us={} max_us={} le100us={} le1ms={} le10ms={} gt10ms={} late_idle={} late_lower={} late_peer={}",
            name,
            r.count,
            r.mean_us,
            r.max_us,
            r.buckets[0],
            r.buckets[1],
            r.buckets[2],
            r.buckets[3],
            r.late[0],
            r.late[1],
            r.late[2]
        );
    }
    // TASK-0363 P0: the three tasks that yield most (pid, the kernel's service id, yields).
    let (y, yields) = crate::yield_stats::top();
    log_info!(
        target: "smp",
        "KINIT: yield top total={} pid={}:{:#x}:{} pid={}:{:#x}:{} pid={}:{:#x}:{}",
        yields,
        y[0].pid,
        y[0].service_id,
        y[0].yields,
        y[1].pid,
        y[1].service_id,
        y[1].yields,
        y[2].pid,
        y[2].service_id,
        y[2].yields
    );
    let (ipi_max_us, ipi_mean_us, ipi_n) = super::budgets::wake_ipi_report();
    log_info!(
        target: "smp",
        "KINIT: wake ipi total max={}us mean={}us n={}",
        ipi_max_us,
        ipi_mean_us,
        ipi_n
    );
}
