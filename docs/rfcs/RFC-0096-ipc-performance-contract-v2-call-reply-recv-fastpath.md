<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0096: IPC performance contract v2 — `ipc_call` / `ipc_reply_recv` fastpath, inline tier, hard cap, budgets with numbers

- Status: **Implemented 2026-09-18** (TASK-0054C P0–P6). An exchange cost **208 µs** and five kernel entries when this was written; it costs **41–43 µs** and two, with one kernel heap allocation per message instead of two and none at all for the 72–93 % of messages that fit inline — and without a single scheduler change, because the one this RFC proposed was measured and withdrawn (§Amendment 2026-09-17).
- Owners: @kernel-team @runtime
- Created: 2026-09-15
- Last Updated: 2026-09-18
- Links:
  - Tasks: `tasks/TASK-0054C-ui-v1a-kernel-ipc-fastpath-control-plane-vmo-bulk.md` (execution + proof, P0–P6)
  - ADR: `docs/adr/0064-request-reply-one-trap-per-side-direct-handoff.md` (the one decision this contract rests on)
  - Extends: `docs/rfcs/RFC-0026-ipc-performance-optimization-contract-v1.md` (control/data-plane split as prose — this RFC makes it a kernel constant with an errno)
  - Amends: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (§"Copy-in/out now, zero/low-copy later" — its deferral criteria are met or made moot, see §"Relationship to RFC-0005"; §"Relationship to our existing IDL + filebuffer/VMO hybrid" — the control plane is packed-LE frames, the VMO travels as the moved cap, see §"Copies"; `MAX_FRAME_BYTES` "initially 512" → `IPC_PAYLOAD_MAX = 8192`)
  - Cites for the zero-copy line: `docs/adr/0038-display-wire-ssot-and-capnp-boundary.md` and `docs/adr/0051-declarative-wire-codec-nexus-wire.md` (Cap'n Proto measured and rejected for tiny frames), `docs/adr/0021-structured-data-formats-json-vs-capnp.md` (its "applies to IPC contracts" bullet is superseded for the OS wire by this RFC), `docs/rfcs/RFC-0072-*.md` / `docs/rfcs/RFC-0080-*.md` (the shipped VMO data plane)
  - Cites: `docs/rfcs/RFC-0079-ipc-last-sender-eof.md` (peer death ends a wait), `docs/rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md` §1 (routing v2 never parks on readiness) and §7 (waits without clocks), `docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md` (lock class), `docs/adr/0062-boot-stage-fence-and-readiness-barriers.md` (ordering is the fence's job)
  - Supersedes in part: `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` (nonce correlation on a shared inbox is no longer the shape of a request/reply — a reply inbox is sequential and private; the remaining shared-inbox case is decided in TASK-0054C P2)

## Status at a Glance

- **Phase 0 (this contract + ADR-0064)**: ✅ 2026-09-15 (TASK-0054C P0 — paper)
- **Phase 1 (first numbers: `KSELFTEST: ipc stats`, `SELFTEST: ipc bench`)**: ✅ 2026-09-15 (TASK-0054C P1 — smp1: `SELFTEST: ipc bench (rt=209us n=64)`, `KSELFTEST: ipc stats (sends=5700 heap_allocs=11400 copies=17665 copy_bytes=1690977 wake_ipis=0 handoff_hit=0 handoff_miss=5172)`; visible: rt=209us, sends=5696 heap_allocs=11392 copies=17653 wake_ipis=0 handoff_miss=5145 — 2 allocations and ~3 copies per message, 0.9 runqueue hops per message on one hart)
- **Phase 2 (ONE client API + ONE server loop in userspace; pairing / deadline / drain forms deleted; gate rule 4)**: ✅ 2026-09-16 (TASK-0054C P2-a ✅ clocks, P2-b ✅ pacing on timer pairs + rule 4, P2-c ✅ one send primitive + the awaited-reply invariant, P2-d ✅ one correlation model — `nexus_ipc::reqrep` deleted, P2-e ✅ a server answers exactly the senders that moved a reply cap — two armed wedges closed, P2-f ✅ statefsd takes the rule too, P2-g ✅ the SERVER hot path allocates nothing per request — opened on a user report, found inputd dying of heap exhaustion under real input load and took all 14 os-lite receive loops off the allocating API; it also corrects P2-f's "no exception left": 28 files fleet-wide still answer cap-less senders on their own shared response endpoint, measured cold in windowd's case)
- **Phase 3 (inline tier + hard cap + `E2BIG`; zero-allocation proof)**: ✅ 2026-09-17 (TASK-0054C P3a — the payload distribution measured (`KSELFTEST: ipc payload hist`), `IPC_SHORT_MAX` set to **32** by that measurement, and the redundant send-path clone deleted; P3b — the tier itself: `heap_allocs / sends` **1.000 → 0.276** (smp1) and **0.114** (visible), with `heap_allocs` equal to the histogram's > 32 B population EXACTLY in all 12 windows of the final gate; price **+10 592 B of steady kernel heap (+0.54 %)**; `E2BIG` boot-proven by `SELFTEST: ipc oversize rejected ok`)
- **Phase 4 (`ipc_call` / `ipc_reply_recv` + budget numbers)**: ✅ 2026-09-17 (TASK-0054C P4a syscall 58 — the reply comes home in registers a1..a4, so the planned copy-out hook at FIVE resume points was not needed; P4b syscall 59 — composition of the existing send + `recv_v2`, so P4a's completion hook fires from it unchanged; P4c-1 — one trap is **5.6–7.7 %** cheaper than two, same server and payload, `calls_in_regs` proves every round took the register path; P4c-2 — the direct handoff **withdrawn** on a measurement, the runqueue hop being 0.41 % of an exchange against the 5.6–7.7 % already banked)
- **Phase 5 (userspace seam on the new traps)**: ✅ 2026-09-18 (TASK-0054C P5a — `exchange::call_into` IS `ipc_call`, so the fleet moved without a call site changing, and the EOF gap that flip would have shipped (EAGAIN where the contract says EPIPE) was found and closed first; P5b — the two core services the P2-g gate's NAME LIST had missed, plus the allocating receive family deleted so the rule is structural; P5b-2 — samgrd's loop, three kernel entries to one, **192 µs → 42 µs**; P5c — nine server loops on ONE shape (`KernelServer::serve_next` + `PendingReply`), which the structure gate forced and which is better than the copy it replaced)
- **Phase 6 (closure)**: ✅ 2026-09-18 — the round trip is ASSERTED, not just printed: `KSELFTEST: ipc call budget ok (rt=<n>us budget=64)`, measured 41–43 µs across smp1, visible, reset and the OTA lanes.

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - the two request/reply syscalls `ipc_call` and `ipc_reply_recv`: argument shape, commit point, completion semantics, error model, rights;
  - the direct-handoff rule (when a wake is a switch, when it is an enqueue) and its counters;
  - the payload tiers: `IPC_SHORT_MAX = 32` (inline, zero kernel heap — measured, see the amendment in §Design) and `IPC_PAYLOAD_MAX = 8192` (`E2BIG` above), as public `nexus-abi` constants;
  - the IPC budgets (`IPC_CALL_RT_BUDGET_US`, `IPC_CALL_HANDOFF_MISS_BUDGET`, `IPC_CALL_ALLOCS = 0`), how they are measured (`smp1` + icount, after the bring-up reset window) and the marker vocabulary that proves them;
  - the userspace shape: ONE client exchange (`nexus_ipc::exchange`) and ONE server loop (`nexus_ipc::KernelServer`) on those traps, and the list of forms that no longer exist (pairing helpers, client deadlines, stale-reply drains).
- **This RFC does NOT own**:
  - the message model, CAP_MOVE rights/rollback, endpoint/waitset semantics (RFC-0005), last-sender EOF (RFC-0079), routing v2 / readiness / stage fence (RFC-0093, ADR-0062);
  - fire-and-forget sends, push subscriptions and waitset-driven receive — they keep `ipc_send_v1` / `ipc_recv_v1` / `ipc_recv_v2` / `waitset_wait` unchanged;
  - bulk transport (VMO arm/splice patterns — RFC-0026, RFC-0071/0073, TASK-0033);
  - the lock-free syscall class (ADR-0049 — the new traps are BKL syscalls);
  - a nonce/correlation layer above the kernel (RFC-0019's remaining consumer is decided in TASK-0054C P2);
  - distributed transport (dsoftbus).

### Relationship to tasks (single execution truth)

- `tasks/TASK-0054C-…md` defines the packages P0–P6, their blast radius, the lanes to re-prove and the stop conditions. Every phase above names its package.

### Relationship to RFC-0005 (amendment)

RFC-0005 §"Copy-in/out now, zero/low-copy later" defers "handle-attached messages" behind four criteria. Since then: one handle per message IS the shipped shape (`MsgHeader.flags & CAP_MOVE`, consumed on enqueue, rolled back on failure), `cap_close` and owner-exit release exist with rollback tests (`syscall/api/tests.rs`) and a router state-machine fuzz (`ipc/mod.rs`), which satisfies criteria 1 and 2; criterion 4 (softbus alignment) is unaffected because no remote handle enters the ABI. Criterion 3 asked for real-workload profiling: TASK-0054C P1 produces the first IPC numbers in the repo, in the `visible` lane at the ShellVisible fence, not only a synthetic ping-pong. This RFC therefore **replaces** that paragraph's deferral with the contract below; `ipc_call` composes RFC-0005's existing send + receive + CAP_MOVE semantics and adds no handle semantics. RFC-0005's "`MAX_FRAME_BYTES` (initially 512)" is corrected to `IPC_PAYLOAD_MAX = 8192`, the value enforced since TASK-0031.

## Context

Every request/reply in the fleet is two traps per side. The client sends the request with a moved SEND clone of its own reply endpoint, then blocks on that endpoint (`nexus_ipc::exchange`, the ONE exchange since TASK-0324 P7 — clockless, EOF-opted). The server receives, handles, sends the reply, receives again. The reply wakes the client through the generic wake (`pop_recv_waiter` → `tasks.wake` → per-CPU runqueue enqueue → cross-hart IPI when the homes differ). Every non-empty message allocates at least twice in the kernel (`Message.payload: Vec<u8>`). No IPC latency, hop or allocation number exists anywhere in the repo, and no test counts either.

The two-trap shape is also where the last clocks and the last dual structures in request/reply live: `~48` hand-rolled send-then-recv functions and 20 callers of `send_with_cap_move_wait` remain beside `exchange` (measured exactly at P2-c: 23 callers, all migrated and the helper deleted; 44 hand-built `CAP_MOVE` headers in 28 files are left for P2-d); app-host still carries a 250 ms deadline and an 8-frame stale-reply drain whose only trigger is that deadline; the wait-not-poll gate cannot see a raw non-zero `deadline_ns` argument. RFC-0093 §7 already says what a wait is — "the reply, or the peer's death" — this RFC gives that sentence an ABI.

## Goals

- ONE trap per side for request/reply, with the reply handed to the caller without a scheduler hop on the common path.
- Zero kernel heap for control messages (≤ `IPC_SHORT_MAX`) and a hard cap with its own errno so bulk cannot drift inline.
- Clocklessness by ABI shape: neither trap takes a deadline.
- Budgets as printed numbers under a deterministic profile, calibrated by measurement before they are asserted.
- Userspace: one client API, one server loop; every pairing / deadline / drain form deleted and gated.

## Non-Goals

- A new IPC primitive, message model or capability model.
- A lock-free path for the new traps; cross-address-space copies; scheduler policy changes beyond the handoff rule.
- Changing fire-and-forget, push-subscription or waitset-driven receive.
- Any change to `sender_service_id` stamping, CAP_MOVE rights/rollback or RFC-0079 EOF semantics.

## Constraints / invariants (hard requirements)

- **Determinism**: budgets are measured under `smp1` + icount after the bring-up reset window (`budgets::reset()` pattern) and printed with their numbers; a marker without numbers is not a proof.
- **No fake success**: `KSELFTEST: ipc call budget ok (…)` only when every asserted number is inside its budget; `SELFTEST: ipc fastpath … ok` only after a real exchange over the new trap.
- **Bounded resources**: inline stage `IPC_SHORT_MAX` bytes per task; a parked heap reply is one `Message` bounded by `IPC_PAYLOAD_MAX` and the endpoint byte budgets (RFC-0005); no queue scan on the hot path; the RFC-0079 scan runs only on the would-block branch as today.
- **Security floor**: identity kernel-stamped on both legs; rights checked as for the traps being composed; the reply cap moved by `ipc_call` must target the endpoint the caller waits on; the peer never writes into another address space.
- **Commit point**: a trap that has enqueued its request/reply is never re-executed.
- **No clock**: no deadline argument; `nsec()` never takes part in a request/reply.
- **Stubs policy**: none — there is no partial fastpath.

## Proposed design

### Contract / interface (normative)

**Constants (`nexus-abi`, public):** `IPC_SHORT_MAX = 32`, `IPC_PAYLOAD_MAX = 8192`. The kernel-private `MAX_FRAME_BYTES` is deleted; the three enforcement sites use `IPC_PAYLOAD_MAX`. A payload longer than `IPC_PAYLOAD_MAX` fails with `E2BIG` (new errno; `nexus_abi::IpcError::TooBig`) on every send-side trap (`ipc_send_v1`, `ipc_call`, `ipc_reply_recv`). Payloads ≤ `IPC_SHORT_MAX` are stored inline in the kernel message (`Payload::Inline`), longer ones on the heap (`Payload::Heap`); the wire ABI (`MsgHeader`, 16 bytes, `len` = inline payload length) is unchanged.

**Amendment 2026-09-16 (TASK-0054C P3a) — `IPC_SHORT_MAX` is 32, and this is why.** This RFC
wrote 64 before anything measured the payload distribution. The measurement (`KSELFTEST: ipc
payload hist`, eight buckets, one count per payload the kernel copies in) was read over **13
windows from two workload families** — the standard boot (smp1, headless, visible, reset,
ota-downgrade, ota-tamper: 5505 to 22 285 payloads) and the OTA bundle lanes (flip, bundle,
resume, delta, fallback: 7554 to 29 880).

In the standard boot every bucket ABOVE 32 bytes is constant to within a rounding error: `le64`
is **exactly 160 in all eight windows**, and the whole > 32 B population is 1527–1536. That is
the boot's fixed work. Everything that scales with the window is ≤ 32 B — 3978 → 20 755, i.e.
**72.3 % to 93.1 %** of all messages. The OTA bundle lanes shift the fixed part, not the shape:
`le64` becomes 526–531 and the 513–1024 B band grows to ~1750 messages (bundle streaming), so
≤ 32 B is 53.5–63.6 % there.

So a 32-byte tier covers every message that scales, in both families. Raising it to 64 does not
buy a percentage: it buys a FIXED **160 messages per standard boot** (0.7 to 2.9 points) or
**~530 per OTA bundle boot** (1.8 to 7.0 points), for twice the inline footprint in every
`Message` — a value moved on every send, push, pop and error return, held in `VecDeque`s across
384 endpoints. The 513–1024 B band that dominates the OTA lanes is the argument for the VMO bulk
path below, not for a bigger inline tier. An average could not have decided any of this: the mean
payload of those same windows runs from 30 to 88 bytes — it tracks how much idle traffic the
window spans, not what a message looks like. P3b may revisit the constant only with a measurement
of the same shape.

Two by-products of the same instrument, recorded because P3b and P4 build on them. The histogram
total equals `heap_allocs` EXACTLY in all 13 windows, which is what "one kernel heap allocation
per payload" means after P3a deleted the send-path clone (it was 2.000 per message from P1 until
this package). And both exceed `sends` by exactly 11 in each of the five OTA bundle lanes and by
0 everywhere else: those 11 are messages the kernel copied in and then failed to enqueue, so the
pair of counters reports the failed-send count for free.

**`SYSCALL_IPC_CALL_V1 = 58`** — `ipc_call(send_slot, hdr: &MsgHeader, req: &[u8], out: &mut [u8], sys_flags) -> Result<usize>`

- `hdr.flags` MUST carry `CAP_MOVE` and `hdr.src` MUST be a SEND cap to an endpoint `R` on which the calling task holds RECV; otherwise `EINVAL` (`test_reject_call_without_reply_cap`, `test_reject_call_reply_cap_foreign_endpoint`). `sys_flags ∈ {0, IPC_SYS_TRUNCATE}`; every other bit → `EINVAL`. There is no deadline argument.
- Validation order is RFC-0005 §`SYSCALL_IPC_SEND_V1` (decode / check / execute); `req.len() > IPC_PAYLOAD_MAX` → `E2BIG` before any side effect; `out` is bounds-checked (`ensure_user_slice`) at entry.
- **Phase 1 (re-entrant).** If the target endpoint's queue is full, the task blocks in `BlockReason::IpcSend` exactly as `ipc_send_v1` does today (moved cap rolled back, trap re-executed on wake).
- **Phase 2 (commit).** The request is enqueued with the moved cap and the kernel-stamped `sender_service_id`. In the same critical section the task is registered as a receive waiter on `R`, its completion state is recorded (`out` pointer + length, `sys_flags`, an `IPC_SHORT_MAX`-byte inline stage), `sepc` is advanced, and the task blocks in `BlockReason::IpcCall { reply_ep: R }`. From here the trap is never re-executed.
- **Phase 3 (completion by the peer).** A send to `R` that finds the task in `IpcCall`: payload ≤ `IPC_SHORT_MAX` is copied into the inline stage, longer payloads are parked as the message; `x[10]` is set to the payload length (or, when `out` is too short and `IPC_SYS_TRUNCATE` is absent, to `-ENOSPC` with the message left queued on `R`, as RFC-0005 requires for `ipc_recv_v1`); the moved cap (if any) is allocated into the caller's cap table and reported in the caller's `hdr.src` on return. The copy-out into `out` runs in the caller's own trap return under its own SATP (one epilogue hook). If `R`'s last foreign SEND cap disappears while the task waits (RFC-0079 scan), completion is `-EPIPE`.
- Return: the reply length; errors `EINVAL`, `EPERM`, `E2BIG`, `EAGAIN`/`ENOSPC` (RFC-0005 backpressure, before the commit point only), `EPIPE` (peer death), `ENOSPC` (reply longer than `out` without TRUNCATE).

**`SYSCALL_IPC_REPLY_RECV_V1 = 59`** — `ipc_reply_recv(reply_slot, reply_hdr: &MsgHeader, reply: &[u8], next: &mut IpcRecvV2Desc) -> Result<usize>`

- `reply_slot` needs `Rights::SEND` (the moved reply cap the server received); `next.endpoint` needs `Rights::RECV` and is the server's own endpoint; `next` is the RFC-0005 v2 descriptor (magic `NXI2`, version 1) so `sender_service_id` is delivered exactly as by `ipc_recv_v2`. `reply.len() > IPC_PAYLOAD_MAX` → `E2BIG`. No deadline argument; `IPC_SYS_EOF` is not accepted (a server owns its endpoint).
- **Phase 1 (re-entrant):** reply endpoint queue full → block in `IpcSend` as today, nothing committed. **Phase 2 (commit):** the reply is enqueued (or handed off — below), `sepc` advanced, the server registered as receive waiter on `next.endpoint` in `BlockReason::IpcRecv` with completion state for `next`. If a request is already queued, the trap completes immediately with it. **Phase 3:** the next request completes the server's trap through the same completion state; copy-out in the server's own return.
- Return: the request length; errors as for `ipc_send_v1` + `ipc_recv_v2`.

**Direct handoff (normative rule).** When a send (any of `ipc_send_v1`, `ipc_call`, `ipc_reply_recv`) completes a waiter that is in `IpcCall`, or in `IpcRecv` entered through `ipc_reply_recv`, and the waiter's affinity mask admits the current hart, the kernel switches to the waiter on this hart without enqueueing it (`handoff_hit`). Otherwise the wake is today's enqueue on the waiter's home CPU + `request_resched` (`handoff_miss`, and `reply_wake_ipi` when an IPI is sent). The sending task keeps its QoS placement; if it is itself blocking (as `ipc_reply_recv` does) the switch costs nothing extra; if it is not (a fire-and-forget `ipc_send_v1` answering a `call`) it is enqueued on its own home CPU and the waiter runs first. The rule is a scheduling shortcut, never a priority change: the waiter runs in its own QoS class.

**Amendment 2026-09-17 (TASK-0054C P4c-2) — the direct handoff is REJECTED, with numbers, and
this rule is withdrawn.** The paragraph above assumed the runqueue hop is a meaningful part of an
exchange. It is not. P4c-2 measured the runqueue half of a wake directly
(`wake_enq_ticks=<mean>/<max>` in the stats line): **0.40 µs mean, 0.9 µs peak**, so the two wakes
of an exchange cost **0.80 µs — 0.41 %** of the 195 µs a call takes. P4c-1 had already banked
**11–15 µs (5.6–7.7 %)** by merging the two traps into one. The handoff is **19× smaller than the
saving already in hand**, and it is not free: this kernel's scheduler is a per-CPU, per-QoS ring
with `enqueue` / `schedule_next` / `yield_current` / `purge` and NO "run this task next"
primitive. Adding one means either bypassing the QoS rings — which the withdrawn paragraph's own
last sentence forbids ("never a priority change") — or paying the queue cycle it was meant to
remove. So the rule is withdrawn rather than implemented, and the scheduler is left alone.

What survives from it: the counters. `handoff_hit` is REMOVED from the stats line instead of being
printed as a hardcoded 0 — a field that cannot be non-zero is decoration, not telemetry — and
`wake_enq_ticks` takes its place, so the premise stays measurable if the cost profile ever changes
(more harts, a different scheduler, cross-hart traffic where `wake_ipis` is not 0). Should that
happen, the decision is a re-measurement, not a rewrite of this paragraph from memory.

**Copies (normative — the zero-copy line).** A control message (≤ `IPC_SHORT_MAX`) costs the kernel zero heap allocations and exactly two payload copies: user → the waiter's completion stage on the sending side, stage → user in the waiter's own trap return. The kernel never stages a payload of that tier on the heap. Bulk never travels through the kernel; it follows the VMO contract the fleet already ships in three independent instances (vfsd splice RFC-0072/TASK-0295, bundlemgrd `OP_ARM_VMO`, windowd→gpud attach RFC-0059, execd's shared RO atlas RFC-0080): the consumer allocates and sizes the VMO; the VMO travels as the message's moved cap (never as a handle integer — RFC-0005 §"IDL + filebuffer/VMO hybrid"'s `vmoHandle` field is the paper version, CAP_MOVE the shipped one); the server is the only writer, payload first, header last (the magic is the release fence); the consumer maps read-only (`vm_map`) — `vmo_read` is a copy and is excluded from zero-copy claims (RFC-0047's honesty rule); oversize is `E2BIG`, never truncation. Cap'n Proto readers run in place over such mappings (`updated`'s OTA chain: `read_message_from_flat_slice_no_alloc` over the mapped VMO; `nexus-dsl-ir`'s `.nxir` reader) — capnp is no_std on riscv64 today and the payload-VMO header's 16 bytes are 8-byte aligned for it (`NXVR`, RFC-0097 — there were two of these headers when this was written). **No Cap'n Proto framing on the inline tier:** ADR-0038 and ADR-0051 measured it for tiny frames and rejected it (segment table + pointers + word alignment ≥ 24 B before any payload — three quarters of the 32-byte tier, no copy to avoid); control frames stay packed-LE `nexus-wire` declarations. Two bounds, two owners: `IPC_PAYLOAD_MAX = 8192` is the transport cap; `INLINE_IO_MAX = 4096` (`vfs-types::splice`) is the vfs surface policy. Every `nexus-ipc` receive buffer is sized by `IPC_PAYLOAD_MAX` (P2) — the triplicated 512-byte ceiling (`exchange.rs`, `os_kernel.rs`, `os_lite.rs`) once turned an oversize OTA frame into a "bad signature" report. The P1 counters (`copies`, `copy_bytes`, `heap_allocs`) are the baseline: P1 measured two allocations and three payload copies per non-empty message (user → heap, heap → heap clone, heap → user); P3a deleted the clone — the send path had kept it "in case the attempt fails", but a failed send already hands the whole `Message` back — so the baseline P3b starts from is **one allocation and two copies** (user → heap, heap → user), measured at exactly 1.000 allocations per message over three windows.

**Budgets (`core/trap/budgets.rs`, SSOT):** `IPC_CALL_RT_BUDGET_US` (`IPC_SHORT_MAX`-byte ping-pong round trip, `smp1` + icount), `IPC_CALL_HANDOFF_MISS_BUDGET` (misses per N exchanges in the kernel selftest, where both tasks are placed on the same hart), `IPC_CALL_ALLOCS = 0` (kernel heap allocations per ≤ `IPC_SHORT_MAX` exchange). Values are calibrated in TASK-0054C P1 on the two-trap path and entered here before P4 asserts them.

**Userspace shape (normative for `source/services`, `source/drivers`, `source/apps`, `source/init`, `userspace`):**

- Client request/reply = `nexus_ipc::exchange::call_into` (alloc-free; over `ipc_call` from P5) — plus `call_matching` only if TASK-0054C P2 records a structurally shared inbox that cannot be made private.
- Server loop = `nexus_ipc::KernelServer` (`recv` once, then `next = reply_recv(reply)`; over `ipc_reply_recv` from P5).
- Gone, and gated by `scripts/check-wait-not-poll.sh` rule 4 (a non-zero `deadline_ns` argument on `ipc_send_v1` / `ipc_recv_v1` / `ipc_recv_v2`, and the retired names): `send_with_cap_move`, `send_with_cap_move_wait`, `Connection` / `Transport`, `reqrep::recv_match*`, `budget::{send_until, recv_until, recv_matching_until, send_budgeted, recv_budgeted, deadline_after, Clock, OsClock}`, `exchange::call` (the allocating form), app-host `svc_call.rs`'s drain + `SVC_DEADLINE_NS`, the DSL `timeoutMs:` knob, `ReplyCap::reply_and_close_wait`.

### Phases / milestones (contract-level)

- **Phase 0**: this contract + ADR-0064 (TASK-0054C P0).
- **Phase 1**: numbers before assertions — `KSELFTEST: ipc stats (…)` over the steady-state window (headless, smp1, visible ladders); `SELFTEST: ipc bench (rt=<n>us n=64)` over the two-trap path (P1).
- **Phase 2**: ONE client API + ONE server loop, deletion list, gate rule 4 at zero (P2).
- **Phase 3**: inline tier + hard cap + `E2BIG`; host counting-allocator test at zero (P3).
- **Phase 4**: the two traps, completion state, handoff, `KSELFTEST: ipc call budget ok (rt=<n>us handoff_miss=<m> alloc=0)` (P4).
- **Phase 5**: `exchange` / `KernelServer` on the traps. The markers this line PLANNED (`ipc fastpath ping/reply/bulk-vmo`) were not built under those names; what was built instead proves more and is listed in §Deterministic markers — `SELFTEST: ipc call ok` and `ipc reply_recv ok` (each checking an echoed nonce, not merely that something came back), `ipc call bench` next to the old two-trap `ipc bench` so the two COMPARE, and `KSELFTEST: ipc call budget ok` asserting the result. The bulk-VMO path needed no new marker: it is unchanged by this RFC and already proven by the vfsd splice and bundlemgrd `OP_ARM_VMO` ladders.

## Security considerations

- **Threat model**: a caller that moves a reply cap for an endpoint it cannot receive on (self-inflicted hang, or a way to have a foreign task's waiter completed); a server that replies with an oversized frame; a peer that tries to make the kernel write into an address space that is not the current one; identity drift on the request delivered by `ipc_reply_recv`; a stale completion state surviving a task's death or exec.
- **Mitigations**: the reply cap is checked against the caller's RECV right on the same endpoint at the commit point; `E2BIG` before any side effect; the peer only stages into the waiter's completion state, the waiter copies out under its own SATP; `sender_service_id` is stamped by the kernel on the enqueue path shared with `ipc_send_v1`; completion state is cleared on `exit_current_and_release` and on `exec`, and a task in `IpcCall` is drained like any receive waiter (RFC-0079 wake path).
- **Negative tests (contract)**: `test_reject_call_without_reply_cap`, `test_reject_call_reply_cap_foreign_endpoint`, `test_reject_oversized_inline` (`E2BIG` on all three send-side traps), `test_reject_reply_recv_without_recv_right`, `test_reject_call_with_deadline_bits` (unknown `sys_flags` → `EINVAL`), `test_call_completes_on_peer_death`, `test_call_commit_is_never_reexecuted`.
- **Open risks**: the epilogue copy-out hook is a new place where the kernel touches user memory on the return path; it is bounded by the entry-time validation and by `IPC_PAYLOAD_MAX`, and it is exercised by the kernel selftest on every boot.

## Failure model (normative)

- Before the commit point every error leaves no trace (moved cap rolled back, nothing enqueued) — `EINVAL`, `EPERM`, `E2BIG`, `EAGAIN`, `ENOSPC`.
- After the commit point exactly three things end an `ipc_call`: a reply (length, or `-ENOSPC` with the message left queued when `out` is too short and TRUNCATE is absent), the peer's death (`-EPIPE`), or the caller's own death (state released). There is no fourth.
- `ipc_reply_recv` after its commit point ends only with the next request or the server's own death.
- No silent fallback: a `handoff_miss` is counted, never hidden; a reply longer than `IPC_SHORT_MAX` is a heap message, counted in `heap_allocs`.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && just lint-kernel && cargo +nightly-2025-01-15 test -p neuron --lib ipc
cd /home/jenning/open-nexus-OS && cargo +nightly-2025-01-15 test -p nexus-ipc
cd /home/jenning/open-nexus-OS && scripts/check-wait-not-poll.sh
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just test-os smp1
cd /home/jenning/open-nexus-OS && just test-os visible
cd /home/jenning/open-nexus-OS && just test-all
```

### Deterministic markers

- `KSELFTEST: ipc stats (sends=<n> heap_allocs=<a> copies=<c> copy_bytes=<b> wake_ipis=<i> handoff_hit=0 handoff_miss=<m>)` (P1; numbers only, no verdict; `handoff_hit` is 0 by construction until P4)
- `KSELFTEST: ipc payload hist (zero=<n> le32=<n> le64=<n> le128=<n> le256=<n> le512=<n> le1k=<n> gt1k=<n>)` (P3a; numbers only, no verdict; one count per accepted send, same window and same telemetry op as the stats line — it SIZES `IPC_SHORT_MAX`, see the amendment in §Design)
- `SELFTEST: ipc bench (rt=<n>us n=<N>)` (P1; numbers only — hops per message are read from the stats line's `handoff_miss / sends` ratio, the selftest has no kernel counter syscall)
- `KSELFTEST: ipc call budget ok (rt=<n>us handoff_miss=<m> alloc=0)` / `… FAIL (…)` (P4)
- `SELFTEST: ipc oversize rejected ok` / `… FAIL` (P3b; the hard cap answers `E2BIG` and nothing else — success fails the probe too)
- `kheap_used=<n>` inside the `ipc stats` line (P3b; kernel heap bytes in use at the same fence, so a change to `Message`'s size is a measured number)
- `SELFTEST: ipc fastpath ping ok (rt=<n>us)`, `SELFTEST: ipc fastpath reply ok`, `SELFTEST: ipc bulk-vmo path ok` (P5)

Registered in `source/apps/selftest-client/proof-manifest/markers/ipc_kernel.toml` (the two `end`-phase stats lines in `markers/end.toml`) and the `scripts/qemu-test.sh` headless + smp1 lists; gated on the stable prefix (numbers are evidence, the verdict word is the gate — `harness.toml` rule).

## Alternatives considered

- **Keep two traps and only add the inline tier + a hop-free wake** — rejected (ADR-0064): the gap, the deadline argument and the pairing helpers survive.
- **Complete the caller from the replier's context (cross-AS write)** — rejected: needs a primitive nothing else needs; staging + copy-out on the waiter's own return is one bounded `IPC_SHORT_MAX`-byte copy.
- **A user-visible "short message" ABI (separate header/syscall for ≤ `IPC_SHORT_MAX`)** — rejected: the tier is a kernel storage decision, the wire ABI stays one.
- **A nonce inside the kernel** — rejected: a reply endpoint is private and sequential; correlation is a symptom of shared inboxes, which P2 removes.

## Open questions

- P2 decides whether any reply inbox stays structurally shared (the statefsd audit-append case behind `call_matching`); if yes, the RFC records the one predicate form as the exception.
- **Answered by P2-d (2026-09-16).** The correlation MECHANISM is one predicate, not a data
  structure: `nexus_ipc::reqrep` (`NonceGen`, `ReplyBuffer`, `FrameStash`, `recv_match`) is
  deleted. Parking a foreign frame for a later exchange was never needed — no service in the
  fleet spawns a thread, so no client ever has two exchanges in flight — and `recv_match` did
  not even park a foreign PROTOCOL, it discarded it. The client forms are now: `call_into` on a
  private, sequential inbox; `call_matching` where the inbox is structurally shared;
  `send_call` where the answer is collected later; `send_with_cap` / `send_with_cap_nonblocking`
  where the moved cap is data; `send_nonblocking` / `send_request` / `recv_response` where no cap
  moves. RFC-0019's contract stands; its mechanism is superseded here.
- **Answered by P2-c (2026-09-16).** Sharing stays, and `call_matching` stays with it, but for a different reason than the draft assumed. The audit-append case is GONE at the source: a fire-and-forget send moves no reply cap any more, so no ack rots on anyone's inbox. What remains shared is structural and declared — one reply inbox per service, several services answering into it (`slots::<svc>::REPLY` with a service's legs hanging off `REPLY.recv`; the harness has six services on one inbox). On such an inbox the answer is the frame carrying this exchange's op (and nonce, where the protocol has one), so `call_matching` is the normative client form there and `call_into` is correct only on a private, sequential inbox. Two forms join it for sends that are not exchanges: `exchange::send_with_cap`, where the moved cap is DATA (a VMO, a push channel) and there is nothing to await, and `exchange::send_nonblocking`, which moves no cap at all.
- **The exception is closed (P2-f, 2026-09-16).** metricsd's retention writes used to move a reply cap whose status nobody read, because both exits were shut: statefsd answered a cap-less write on its shared response queue, and awaiting the status risks a wait cycle through statefsd's quota gate. The seed called for a one-way write op in the statefs wire. The better answer needed no wire change: **a server answers exactly the senders that moved a reply capability.** logd took that rule in P2-c, metricsd in P2-e, statefsd in P2-f — once its three cap-less clients moved a cap. A fire-and-forget write now moves no cap, so no ack exists, and no server blocks on a response queue nobody reads (the 0049B wedge class, removed at the source in three services rather than worked around in each client).
- P1 decides the budget numbers; until then the constants are named here without values.

---

## Implementation Checklist

- [x] **Phase 0**: contract + ADR-0064 — proof: this document indexed, ledger P0 Done (2026-09-15)
- [x] **Phase 1**: numbers — proof: `just test-os smp1` and `just test-os visible` show `KSELFTEST: ipc stats (…)` and `SELFTEST: ipc bench (…)` (2026-09-15: smp1: `SELFTEST: ipc bench (rt=209us n=64)`, `KSELFTEST: ipc stats (sends=5700 heap_allocs=11400 copies=17665 copy_bytes=1690977 wake_ipis=0 handoff_hit=0 handoff_miss=5172)`; visible: rt=209us, sends=5696 heap_allocs=11392 copies=17653 wake_ipis=0 handoff_miss=5145 — 2 allocations and ~3 copies per message, 0.9 runqueue hops per message on one hart)
- [x] **Phase 2**: one client API + one server loop — proof: `scripts/check-wait-not-poll.sh` at zero with rule 4; grep-gone list empty; `just test-all` (2026-09-16, P2-a…P2-f)
- [x] **Phase 3**: inline tier + `E2BIG` — proof: the counting `#[global_allocator]` in `just test-kernel` (0 allocations at `IPC_SHORT_MAX`, exactly 1 above; checked against a negative control), `SELFTEST: ipc oversize rejected ok` in QEMU, `just test-all` (2026-09-17, P3a + P3b: `heap_allocs / sends` 2.000 → 1.000 → 0.276, and `heap_allocs` no longer scales with the boot)
- [x] **Phase 4**: traps + budget — proof: `KSELFTEST: ipc call budget ok (rt=42us budget=64)`, `SELFTEST: ipc call ok`, `ipc reply_recv ok`, `ipc oversize rejected ok`; `just test-all` (2026-09-18). The HANDOFF is not in this line because it was withdrawn on a measurement — see §Amendment 2026-09-17.
- [x] **Phase 5**: seam flip — proof: the fleet on `ipc_call` (`calls_in_regs` 65 → 2143 per boot), nine server loops on `serve_next`, `just test-all` green at every step (2026-09-18).
- [ ] Task linked with stop conditions + proof commands (TASK-0054C).
- [ ] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
