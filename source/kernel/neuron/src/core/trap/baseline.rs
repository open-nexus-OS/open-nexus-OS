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

const CLASS_NAMES: [&str; crate::ready_stats::LANES] =
    ["idle", "normal", "interactive", "burst", "reserved"];

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
    // TASK-0330 P2 (RFC-0102): what each ranked lock class cost in the window — a class that
    // left the kernel lock shows here what it holds and waits on its own.
    for class in crate::sync::lock_rank::Class::ALL {
        let window = crate::sync::class_stats::take_window(class);
        if window.takes != 0 {
            log_info!(
                target: "smp",
                "KINIT: lock class {} (takes={} waited={} max_hold_us={} max_wait_us={})",
                class.name(),
                window.takes,
                window.waited,
                window.max_hold / tpu.max(1),
                window.max_wait / tpu.max(1)
            );
        }
    }
    // TASK-0330 P3: single frames the harts took from their magazines against the refills that
    // asked the pool, and what a dry pool took back.
    let (harts, held, served, refills, flushed) = crate::mm::frame_pool::magazine_stats();
    log_info!(
        target: "mm",
        "KINIT: mm magazines (harts={} held={} served={} refills={} flushed={})",
        harts,
        held,
        served,
        refills,
        flushed
    );
    // And the user page faults by the path they took: filled with the thread's own space held
    // and nothing else, or handed to the kernel lock; the longest fill without it.
    let (free, locked, longest) = super::fault_entry::take_window();
    log_info!(
        target: "mm",
        "KINIT: mm faults (free={} locked={} longest_free_us={})",
        free,
        locked,
        longest / tpu.max(1)
    );
    // What the three calls that run before the kernel lock (a mapping, an unmap, a device's
    // runs) still took of it: a page of the answer not ready, an order waiting, the judge of an
    // object nobody holds any more.
    let (map, unmap, runs) = (
        lock_share::holder(crate::syscall::SYSCALL_VM_MAP, tpu),
        lock_share::holder(crate::syscall::SYSCALL_VM_UNMAP, tpu),
        lock_share::holder(crate::syscall::SYSCALL_VMO_RUNS, tpu),
    );
    log_info!(
        target: "mm",
        "KINIT: bkl mm calls (map={}us/{} unmap={}us/{} runs={}us/{})",
        map.0,
        map.1,
        unmap.0,
        unmap.1,
        runs.0,
        runs.1
    );
    // And the shootdowns: of every hart, for one space, and how many harts the latter reached
    // (a machine of n harts reaches n - 1 with each of the former).
    let (all, targeted, reached) = crate::smp::tlb::counts();
    log_info!(
        target: "mm",
        "KINIT: tlb shootdowns (all={} space={} reached={})",
        all,
        targeted,
        reached
    );
    // TASK-0363 P2 S3: the idle loop's holds by what they are for, and the boot hart's delivery
    // by its two parts (a step that does not move its holder's share is reverted — ADR-0098).
    let held = |id| lock_share::holder(id, tpu);
    let (dispatch, due, deliver) =
        (held(lock_share::IDLE), held(lock_share::IDLE_DUE), held(lock_share::IDLE_DELIVER));
    log_info!(
        target: "smp",
        "KINIT: bkl idle split (dispatch={}us/{} due={}us/{} deliver={}us/{} events_us={} walk_us={})",
        dispatch.0,
        dispatch.1,
        due.0,
        due.1,
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
    // finds it queued behind a busy hart (since P5 a wake places such a task outright).
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
    // TASK-0363 P5: each hart's busy share (permille of the window), and the wakes placed on a
    // free hart instead of behind a peer at home. P3: `metered=` the harts' mean share the meter
    // charged to tasks — the rest of `mean` is the idle loop awake (its walks, steals, deliveries).
    static LAST_PLACED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    let (busy_ns, min, at_min, max, at_max, mean) =
        crate::smp::take_busy_window(crate::hal::platform::ticks_to_ns(now));
    let placed = crate::smp::wake_placement_count();
    let last = LAST_PLACED.swap(placed, core::sync::atomic::Ordering::Relaxed);
    log_info!(
        target: "smp",
        "KINIT: hart busy pm (window_ms={} min={} at=cpu{} max={} at=cpu{} mean={} metered={} placed={})",
        busy_ns / 1_000_000,
        min,
        at_min,
        max,
        at_max,
        mean,
        crate::sched::meter::take_metered_pm(busy_ns),
        placed.saturating_sub(last)
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
            "KINIT: ready latency class={} n={} mean_us={} max_us={} le100us={} le200us={} le1ms={} le10ms={} gt10ms={} late_idle={} late_lower={} late_peer={}",
            name,
            r.count,
            r.mean_us,
            r.max_us,
            r.buckets[0],
            r.le200us,
            r.buckets[1],
            r.buckets[2],
            r.buckets[3],
            r.late[0],
            r.late[1],
            r.late[2]
        );
    }
    // TASK-0363 P3: the time rights — the most any hart has reserved, and since the last line the
    // budgets that ran out under their holder (it went on fair) and the periods a runnable holder
    // did not get its budget in (what a guarantee is judged by: zero — claimed by no marker
    // before the kernel lock is gone, RFC-0120 G1). The holders' waits: `class=reserved` above.
    // `lent`: the calls that lent their caller's right to the server they woke (ADR-0102).
    static LAST_COUNTS: [core::sync::atomic::AtomicUsize; 3] =
        [const { core::sync::atomic::AtomicUsize::new(0) }; 3];
    let (overruns, misses) = crate::sched::reserve::counts();
    let (reserved_ppm, at) = crate::sched::reserve::load();
    let since = |slot: usize, now: usize| {
        now.saturating_sub(LAST_COUNTS[slot].swap(now, core::sync::atomic::Ordering::Relaxed))
    };
    log_info!(
        target: "smp",
        "KINIT: time rights (window_ms={} reserved_max_pm={} at=cpu{} overruns={} misses={} lent={})",
        window_ms,
        reserved_ppm / 1_000,
        at,
        since(0, overruns),
        since(1, misses),
        since(2, crate::sched::loan::count())
    );
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
