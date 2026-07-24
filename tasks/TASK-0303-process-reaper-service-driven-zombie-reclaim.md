# TASK-0303: Process reaper — service-driven zombie reclaim (execd reaper-of-record)

- Status: Done (boot-proven 2026-07-24)
- Owners: @kernel-team / @runtime
- RFC: `docs/rfcs/RFC-0081-process-reaper-nonblocking-reclaim.md`
- Related: RFC-0075 8f (process-image arena reclaim), RFC-0079 (IPC last-sender
  EOF → apps self-exit on window close), `kernel-early-blocking-fleet-collapse`

## Problem

Since RFC-0079, app-hosts self-exit when their window closes. But nothing
**reaps** them: `execd` only reaps on an explicit `OP_WAIT_PID` request, and no
client waits for a fire-and-forget app-host. The exited app-host stays a Zombie,
so its address space (heap-backed page tables) is never destroyed. After a
handful of launches the 8 MiB kernel heap fills → `PANIC ALLOC-FAIL` (the heap
was bumped 2→8 MiB purely as a bridge for this missing reaper). app-host is
single-threaded, so a reap gives AS refcount 1→0 and `destroy` succeeds cleanly.

## Goal / stop condition

`execd` is the **reaper-of-record**: it reaps its exited children promptly and
non-blockingly, so a spawned-and-closed app window's AS is reclaimed without any
client waiting. Address-space live-count stays flat across an open/close storm;
no `ALLOC-FAIL`. Exit codes are retained for `OP_WAIT_PID` clients (no
regression, no double crash-report).

## Design

- **Kernel** — `SYSCALL_WAIT_NOHANG` (52): reaps ONE ready zombie child of the
  caller and returns `(pid, status)`; returns `0` (nothing) instead of blocking
  when no child is ready. Reuses `reap_child(None)` (detach → destroy AS). Pure
  addition; existing `wait` (blocking) is untouched.
- **nexus-abi** — `wait_nohang() -> SysResult<Option<(Pid, i32)>>` (`None` = no
  zombie ready).
- **execd (os-lite)** — reaper-of-record:
  - `TrackedChild` gains `exit: Option<i32>` + `reported: bool`.
  - `handle_child_exit(pid, code)` — the crash-marker / minidump / nexus-log path
    (was inline in `OP_WAIT_PID`), run **exactly once** per child (idempotent).
  - `reap_ready_children()` — drains `wait_nohang()` until `None`, calling
    `handle_child_exit` for each. Called once per serve-loop iteration (covers
    every spawn / request), so a new launch first reclaims prior closures.
  - `OP_WAIT_PID` answers from the cached exit record when the sweep already
    reaped the child; otherwise blocks as before (no regression).
  - Honest marker: `execd: reaped pid=<n> code=<c>` (bounded, human-rate).

## Non-goals (this task)

- Kernel thread-teardown on process exit (terminating sibling threads that share
  an AS so its refcount reaches 0). That fixes the separate
  `destroy as failed InUse` seen ONLY for the kernel workpool selftest (a
  thread-spawning process), needs race-free cross-hart thread termination
  (SMP quiescence) → seeded as **TASK-0304**. app-host is single-threaded, so
  this task's reaper reclaims its AS without it.

## Proof

The new logic lives entirely in target-gated code (`reap_child`/AS refcount are
`#[cfg(target_os="none")]`, so not host-runnable — a synthetic pure-module test
would be theater). Honest proof set:

- Target unit (already present): `address_space::tests` —
  `destroy_rejects_address_space_with_owner` (InUse while an owner remains) and
  `destroy_reclaims_address_space_and_updates_stats` (freed once owners empty)
  pin the reap→free invariant a single-owner app-host reap relies on.
- QEMU: address-space live/peak telemetry stays flat across an app open/close
  storm; no `VMO-POOL exhausted` / `ALLOC-FAIL`; `execd: reaped` markers appear;
  `ci-os-smp1` green (existing markers unchanged).

## Checklist

- [x] `SYSCALL_WAIT_NOHANG` (52) const + handler + registration (kernel)
- [x] `wait_nohang` ABI wrapper (nexus-abi)
- [x] execd `TrackedChild` fields + `handle_child_exit` + `reap_ready_children`
- [x] execd serve-loop sweep + `OP_WAIT_PID` cache fast-path
- [x] Boot proof: `ci-os-smp1` green (9/9); `execd: reaped pid=44 code=0` fires
      at the greeter launch (reclaim-before-spawn); no `ALLOC-FAIL` / exhausted.
      (reap→free invariant covered by existing target `address_space::tests`.)
      The display-gated minidump / `OP_WAIT_PID` crash lane is behavior-preserving
      by construction — confirm in the next visible boot.
- [x] Module-size ratchet: pure crash serializers split to `execd/src/crash_fields.rs`
- [x] Docs sweep: RFC-0081, CHANGELOG, `lib.rs` heap-bridge comment updated
