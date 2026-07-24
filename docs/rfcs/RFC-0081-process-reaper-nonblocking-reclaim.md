# RFC-0081: Process reaper — non-blocking service-driven zombie reclaim

- Status: In Progress (boot-proven 2026-07-24; execution TASK-0303 Done)
- Owners: @kernel-team / @runtime
- Created: 2026-07-23
- Last Updated: 2026-07-23
- Links:
  - Tasks: `tasks/TASK-0303-process-reaper-service-driven-zombie-reclaim.md`
    (execution + proof), `tasks/TASK-0304-thread-teardown-shared-as-refcount-lifecycle.md`
    (deferred thread-teardown)
  - Related RFCs: `docs/rfcs/RFC-0079-ipc-last-sender-eof.md` (apps self-exit on
    window close — the event this RFC reclaims), RFC-0075 8f (process-image
    arena reclaim on exit)

## Status at a Glance

- **Kernel `SYSCALL_WAIT_NOHANG` (52)**: ✅ (2026-07-24)
- **`nexus-abi::wait_nohang`**: ✅
- **execd reaper-of-record**: ✅ — boot-proven: `execd: reaped pid=44 code=0`
  fires at the greeter launch (reclaim-before-spawn); `ci-os-smp1` green; no
  `ALLOC-FAIL`.
- **Thread-teardown (InUse)**: deferred → TASK-0304.

## Problem

RFC-0079 lets a closed-window app-host self-exit; RFC-0075 8f returns its image
pages to the arena on exit. But the exited task remains a **Zombie** until a
parent `wait`s it — and `execd` only `wait`s on an explicit `OP_WAIT_PID`
request. No client waits for a fire-and-forget app-host, so its zombie (and its
heap-backed page-table address space) is never reaped. A handful of launches
exhausts the 8 MiB kernel heap → `PANIC ALLOC-FAIL`. The heap was grown 2→8 MiB
purely as a bridge for this missing reaper.

app-host is single-threaded: reaping it drives its AS `owners` set to empty
(`refcount` 1→0), so `destroy` succeeds and the page tables are freed.

## Goals

- `execd` reaps its exited children **promptly and non-blockingly**, so a
  spawned-and-closed window's AS is reclaimed with no client waiting.
- AS live-count stays flat across an open/close storm; no `ALLOC-FAIL`.
- Exit codes are preserved for `OP_WAIT_PID` clients; crash reporting fires
  exactly once (no regression, no double-report).

## Non-goals

- Terminating sibling **threads** that share an AS on process exit → TASK-0304.
  Its **Part 1 landed alongside this RFC** (2026-07-24): `reap_child` now
  destroys a shared AS only when the reaped task was its last owner, so reaping a
  thread whose parent is still alive no longer logs a spurious
  `destroy as failed InUse` (the AS is correctly reclaimed when the last owner is
  reaped — it was never leaked). Part 2 — active teardown of *parked daemon*
  worker threads when their owning service exits (needs cross-hart quiesce) —
  stays deferred; no shipping service needs it yet.
- Making `OP_WAIT_PID` for a still-running child non-blocking (it blocks execd's
  serve loop today; unchanged here — noted as a follow-up).
- Kernel-side auto-reap (adopt-and-reap in the kernel): rejected because execd
  owns the crash-vs-clean decision and the exit-code cache; the kernel must not
  discard exit status a service still needs.

## Design

### Kernel — `SYSCALL_WAIT_NOHANG` (52)

A non-blocking sibling of `SYSCALL_WAIT` (12). Reaps ONE ready zombie child of
the caller (`reap_child(None)` → detach + destroy AS), returning the pid in `a0`
and the exit status in `a1` (mirrors `wait`). When no child is ready to reap
(`WouldBlock`) or the caller has no children (`NoChildren`), it returns `0` in
`a0` (and `0` status) **instead of blocking**. Real errors propagate. A reaped
child is always pid ≥ 1 (PID 0 is the kernel), so `0` is an unambiguous
"nothing reaped" sentinel. Pure addition — `wait` (blocking) is untouched.

### nexus-abi

```rust
pub fn wait_nohang() -> SysResult<Option<(Pid, i32)>>  // None = no zombie ready
```

### execd — reaper-of-record

- `TrackedChild` gains `exit: Option<i32>` + `reported: bool`.
- `handle_child_exit(pid, code)` — the crash-marker / minidump / nexus-log path,
  factored out of `OP_WAIT_PID` and made idempotent (`reported` guard) so it
  runs exactly once whether triggered by the sweep or by a client wait.
- `reap_ready_children()` — drains `wait_nohang()` until `None`, calling
  `handle_child_exit` per reaped child. Invoked once per serve-loop iteration,
  so every spawn / request first reclaims prior closures — the arena and heap
  never accumulate across launches.
- `OP_WAIT_PID` returns the cached exit record when the sweep already reaped the
  child; otherwise it blocks on `wait(pid)` as before.

## Security / invariants

- No new authority: `wait_nohang` reaps only the caller's own children (same
  parent check as `wait`). A service cannot reap another service's tasks.
- Crash reporting stays exactly-once (idempotent `handle_child_exit`); no
  attacker-influenced path (children are execd-spawned, exit codes kernel-sourced).
- Bounded: `reap_ready_children` drains at most the bounded child count per
  iteration; the reap marker is human-rate.

## Proof

The reaper's logic is entirely target-gated (`reap_child` + AS refcount are
`#[cfg(target_os="none")]`), so the honest proof is target/QEMU, not a synthetic
host test:

- Target unit (already present): `address_space::tests` —
  `destroy_rejects_address_space_with_owner` and
  `destroy_reclaims_address_space_and_updates_stats` pin the reap→free invariant
  (destroy rejects while an owner remains, succeeds once the owners set empties).
- QEMU: AS live/peak telemetry flat across an app open/close storm; no
  `ALLOC-FAIL`; `execd: reaped` markers present; `ci-os-smp1` green.

## Alternatives considered

- **Blocking `wait` in a reaper thread** (the host-std `std_server.rs` model):
  os-lite has no threads, so a blocking wait would stall the single serve loop.
- **Kernel adopt-and-reap**: discards exit status execd needs for crash
  reporting; rejected (see Non-goals).
