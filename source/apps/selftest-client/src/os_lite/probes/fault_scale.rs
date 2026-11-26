// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: how the kernel's page fault path scales over the CPUs (TASK-0330 P3's gate). One
//!   thread touches fresh pages of a lazy mapping; then one thread per CPU does at once, each
//!   its own pages of the same mapping — one address space, so what the threads share is the
//!   kernel's fault path: the space's lock, the object's table, the frame pool. Prints both
//!   times and the factor between the rates. A bench line: the board cycle judges it (>= 6x
//!   with 8 threads); a multi-hart emulation shares host cores, so no QEMU lane does.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `SELFTEST: fault scale (threads=…)` on every lane with two or more CPUs
//!   (the line; its value is the board's)
//! ADR: docs/adr/0048-userspace-runtime-integrity-detectors.md

use core::sync::atomic::{AtomicUsize, Ordering};

use nexus_abi::{page_flags, yield_};

use crate::markers::{emit_bytes, emit_line, emit_u64};

const PAGE: usize = 4096;
/// Fresh pages one thread touches in a round.
const PAGES: usize = 1024;
/// The most threads a round runs (the stacks below are for the others, this thread is one).
const MAX_THREADS: usize = 8;
const STACK: usize = 16 * 1024;
/// How long this thread waits for the others, a round.
const WAIT_NS: u64 = 5_000_000_000;

static mut STACKS: [[u8; STACK]; MAX_THREADS - 1] = [[0; STACK]; MAX_THREADS - 1];
/// The mapping's base; share `i` is its pages `i * PAGES ..`.
static BASE: AtomicUsize = AtomicUsize::new(0);
static READY: AtomicUsize = AtomicUsize::new(0);
static GO: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicUsize = AtomicUsize::new(0);

/// One write into each page of share `share`: each a first touch, each a page fault.
fn touch(share: usize) {
    let base = BASE.load(Ordering::Acquire) + share * PAGES * PAGE;
    for page in 0..PAGES {
        // SAFETY: inside the mapping `run` made; a share is one thread's alone.
        unsafe { core::ptr::write_volatile((base + page * PAGE) as *mut u8, 1) };
    }
}

/// Another thread of the round: on its CPU and spinning until all start together.
extern "C" fn worker(share: usize) {
    READY.fetch_add(1, Ordering::Release);
    while GO.load(Ordering::Acquire) == 0 {
        core::hint::spin_loop();
    }
    touch(share);
    DONE.fetch_add(1, Ordering::Release);
}

fn now() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// Waits until `counter` reached `target`; `spin`: without giving the CPU away.
fn reached(counter: &AtomicUsize, target: usize, spin: bool) -> bool {
    let deadline = now().saturating_add(WAIT_NS);
    while counter.load(Ordering::Acquire) < target {
        if now() > deadline {
            return false;
        }
        if spin {
            core::hint::spin_loop();
        } else {
            let _ = yield_();
        }
    }
    true
}

fn reap(pid: nexus_abi::Pid) {
    for _ in 0..4096 {
        if nexus_abi::wait(pid as i32).is_ok() {
            return;
        }
        let _ = yield_();
    }
}

/// Both rounds; `None`: no mapping, no thread, or a thread that did not finish.
fn measure(threads: usize) -> Option<(u64, u64)> {
    let len = (threads + 1) * PAGES * PAGE;
    let vmo = nexus_abi::vmo_create_lazy(len).ok()?;
    let flags = page_flags::VALID | page_flags::READ | page_flags::WRITE | page_flags::USER;
    let Ok(va) = nexus_abi::vm_map(vmo, 0, len, flags) else {
        let _ = nexus_abi::cap_close(vmo);
        return None;
    };
    BASE.store(va, Ordering::Release);
    for counter in [&READY, &GO, &DONE] {
        counter.store(0, Ordering::Release);
    }

    // One thread, share 0.
    let t0 = now();
    touch(0);
    let one_ns = now().saturating_sub(t0);

    // One thread per CPU: this one and `threads - 1` others, shares 1..=threads.
    let mut pids = [0 as nexus_abi::Pid; MAX_THREADS - 1];
    let mut spawned = 0;
    for (i, pid) in pids.iter_mut().enumerate().take(threads - 1) {
        // SAFETY: stack `i` is used by the one thread spawned here, reaped before `run` ends.
        let stack = unsafe { &mut (*core::ptr::addr_of_mut!(STACKS))[i] };
        match nexus_abi::thread::spawn_thread(worker, i + 2, stack) {
            Ok(spawned_pid) => {
                *pid = spawned_pid;
                spawned += 1;
            }
            Err(_) => break,
        }
    }
    let ready = spawned == threads - 1 && reached(&READY, spawned, false);
    let t0 = now();
    GO.store(1, Ordering::Release);
    touch(1);
    let done = reached(&DONE, spawned, true);
    let all_ns = now().saturating_sub(t0);

    for pid in &pids[..spawned] {
        reap(*pid);
    }
    let _ = nexus_abi::vm_unmap(va, len);
    let _ = nexus_abi::cap_close(vmo);
    (ready && done).then_some((one_ns, all_ns))
}

/// Runs the bench on a machine of two or more CPUs and prints its line.
pub(crate) fn run() {
    let cpus = nexus_abi::sched::online().map_or(1, |set| set.count());
    let threads = cpus.min(MAX_THREADS);
    if threads < 2 {
        return;
    }
    let Some((one_ns, all_ns)) = measure(threads) else {
        emit_line(crate::markers::M_SELFTEST_FAULT_SCALE_FAIL);
        return;
    };
    // The rates' factor in tenths: `threads` times the pages in `all_ns` against one share's
    // in `one_ns`.
    let x10 = (threads as u64 * one_ns * 10) / all_ns.max(1);
    emit_bytes(crate::markers::M_SELFTEST_FAULT_SCALE_THREADS.as_bytes());
    emit_u64(threads as u64);
    emit_bytes(b" pages=");
    emit_u64(PAGES as u64);
    emit_bytes(b" one_us=");
    emit_u64(one_ns / 1_000);
    emit_bytes(b" all_us=");
    emit_u64(all_ns / 1_000);
    emit_bytes(b" x10=");
    emit_u64(x10);
    emit_line(")");
}
