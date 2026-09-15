<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0064: Request/reply is one trap per side with a direct handoff — the reply completes the caller's syscall, and no clock takes part

- Status: Proposed (P0 seed 2026-09-15; Accepted when TASK-0054C P4 lands with its `test_reject_*` and the budget marker)
- Date: 2026-09-15
- Links:
  - Tasks: `tasks/TASK-0054C-ui-v1a-kernel-ipc-fastpath-control-plane-vmo-bulk.md` (execution + proof, P0–P6)
  - RFCs: `docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md` (the contract this decision feeds), `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (message model, CAP_MOVE — unchanged), `docs/rfcs/RFC-0079-ipc-last-sender-eof.md` (peer death ends a wait), `docs/rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md` §7 (waits without clocks)
  - Related ADRs: `docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md` (the lock class the new traps live in), `docs/adr/0062-boot-stage-fence-and-readiness-barriers.md` (ordering comes from the fence, never from parking on readiness), `docs/adr/0057-service-restart-capability-re-resolve.md` (a live-but-silent peer is a supervision truth)

## Context

Every request/reply exchange in the fleet is two traps on the client (`ipc_send_v1` with the
moved reply cap, then `ipc_recv_v1` on the caller's own reply inbox — `userspace/nexus-ipc/
src/exchange.rs`, the ONE exchange since TASK-0324 P7) and two on the server (`ipc_recv_v2`,
then `ipc_send_v1` of the reply). A reply wakes the caller through the generic path
(`pop_recv_waiter` → `tasks.wake` → per-CPU runqueue enqueue → cross-hart IPI when the
homes differ), so the caller runs only when its home hart's scheduler reaches it. Every
non-empty message costs at least two kernel heap allocations (`Message.payload: Vec<u8>`);
there is no inline tier. No IPC latency number exists anywhere in the repo.

Two kernel facts decide how a combined trap can exist at all:

- `Error::Reschedule` re-executes the syscall instruction when the task runs again
  (`syscall/mod.rs`, `core/trap/handler.rs`). Send and receive are re-entrant by rolling the
  moved cap back before they block. A trap that has already enqueued its request is not
  re-entrant: re-executing it would send the request twice.
- The kernel has no cross-address-space copy. Every user copy runs under the current task's
  SATP, so the peer that produces the reply cannot write into the caller's buffer.

The fork in the road: keep two traps per side and optimize around them (hop-free wake,
inline payload only), or make request/reply ONE trap per side and let the peer complete the
waiter's syscall. The first keeps `exchange` as it is but leaves the trap count, the
"reply lands in the inbox while nobody waits yet" window and the client-side deadline
argument in place. The second is the canonical microkernel form and closes the desync class
(the app-host stale-reply drain, TASK-0054C ground truth) by construction.

## Decision

We will make request/reply ONE trap per side, with the peer completing the waiter's syscall,
and without any clock argument.

- **Two syscalls, one mechanism.** `ipc_call` (request + wait for the reply) and
  `ipc_reply_recv` (reply + wait for the next request). Each has a **commit point** — the
  request (resp. the reply) is enqueued — after which the trap is never re-executed: the
  kernel advances `sepc`, records the waiter's completion state on the task
  (`task/completion.rs`: output pointer + length, a 64-byte inline stage, or a parked
  message) and blocks it as a registered receive waiter of the endpoint it waits on. Before
  the commit point (target queue full) the trap blocks exactly like `ipc_send_v1` today,
  with nothing committed and the moved cap rolled back.
- **The peer completes the syscall.** The send that answers a waiter in call state writes
  the result (`x[10]`) and stages the payload into the waiter's completion state. The
  copy-out into the user buffer happens in the waiter's own trap return, under its own
  address space — one hook in the trap epilogue, never a cross-AS write. The RFC-0079 EOF
  scan completes a waiter the same way with `EPIPE` when its last peer dies.
- **Direct handoff.** When the answering send finds the waiter in call state and the
  waiter's affinity admits the current hart, the hart switches to the waiter directly — no
  runqueue enqueue, no IPI. Otherwise the wake is the ordinary enqueue + resched request.
  The two outcomes are counted (`handoff_hit` / `handoff_miss`) and the miss count is a
  budget.
- **No clock.** Neither syscall takes a deadline. A `call` ends by the reply or by the death
  of the last peer (RFC-0079); a live-but-silent peer is a supervision truth (ADR-0057),
  never a client timer (RFC-0093 §7). `ipc_reply_recv` does not opt into EOF: a server owns
  its endpoint, and "all clients gone" is not an error for it.
- **Inline tier and hard cap.** Payloads ≤ `IPC_SHORT_MAX = 64` bytes travel inline in the
  kernel message with zero heap allocation; payloads above `IPC_PAYLOAD_MAX = 8192` are
  rejected with `E2BIG`. Both constants are public in `nexus-abi`; bulk stays VMO
  (RFC-0026).
- **Authority unchanged.** The reply cap moved by `ipc_call` must be a SEND cap to the
  endpoint the caller waits on, and the caller must hold RECV on it; `ipc_reply_recv`
  needs the same rights `ipc_send_v1` + `ipc_recv_v2` need today. `sender_service_id` is
  kernel-stamped on the request `ipc_reply_recv` delivers exactly as on `ipc_recv_v2`.
  CAP_MOVE rights, rollback and RFC-0079 semantics are not touched.
- **Out of scope for this decision:** a lock-free class for the new traps (they are BKL
  syscalls under ADR-0049 and are measured by `record_ecall_hold` like every other), a
  cross-address-space copy primitive, changes to fire-and-forget, push-subscription or
  waitset-driven receive, and any nonce/correlation layer above the kernel.

## Consequences

- **Positive**: one trap per side; the reply cannot arrive "between" a send and a recv
  because there is no such gap — the stale-reply / desync class is impossible by
  construction, so the app-host drain, every client deadline and the DSL `timeoutMs:` knob
  go; short control messages allocate nothing in the kernel; the reply reaches the caller
  without a scheduler hop on the common path; clocklessness (RFC-0093 §7) is enforced by
  the ABI shape instead of a scanner; the budget is a printed number under `smp1` + icount.
- **Negative / accepted cost**: a new kernel concept — a syscall completed by another task —
  with its own completion state, an epilogue hook and one more `BlockReason` arm; every
  request/reply consumer moves onto `exchange` / `KernelServer` first (TASK-0054C P2, ~48
  hand-rolled sites), so the kernel seam flips in one file per side afterwards; server
  loops change shape from `recv → handle → send` to `next = reply_recv(reply)`; two of the
  six remaining syscall numbers (58, 59) are spent.
- **Follow-ups**: RFC-0096 (contract + budgets), TASK-0054C P1 (the first IPC numbers in
  the repo), P2–P6; the VMO splice poll in `updated/apply_os.rs` is a bulk-path concern and
  belongs to TASK-0033.

## Alternatives considered

- **Keep two traps, add only the inline tier and a hop-free wake.** Rejected: the
  send/recv gap, the client deadline argument and the pairing helpers survive, so the
  desync class and the clock stay expressible; the scanner would remain the only guard.
- **Complete the caller by writing into its user buffer from the replier's context.**
  Rejected: needs a cross-address-space copy primitive the kernel does not have and that
  nothing else needs; staging in the waiter's completion state and copying out on its own
  return costs one bounded 64-byte copy and no new primitive.
- **Make `ipc_call` re-entrant by de-duplicating a re-sent request (sequence tag).**
  Rejected: turns "never re-execute a committed trap" into a per-endpoint dedup table with
  its own bounds and failure modes; the commit point + completion state is the smaller
  invariant.
- **Give `ipc_call` a deadline "for safety".** Rejected by RFC-0093 §7 and TASK-0324 P7's
  evidence: every liveness bound in the fleet existed only because a dead peer could not
  wake a waiter, and that cause is gone.
