---
title: TASK-0054C Kernel IPC performance contract + `call` / `reply_recv` fastpath (one trap per side, direct handoff, inline ≤ 32 B, VMO-first bulk)
status: In Progress (P0 paper 2026-09-15; end-state rewrite 2026-09-15 supersedes 2026-09-09; kernel + libs approval zones — approval per package)
owner: @kernel-team @runtime
created: 2026-03-29
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Kernel IPC contract: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - IPC runtime architecture: docs/adr/0003-ipc-runtime-architecture.md
  - VMO plumbing baseline: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - UI perf floor baseline: tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md
  - Present/input consumer baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Testing contract: scripts/qemu-test.sh
  - IPC performance contract v1 (extended by RFC-0096): docs/rfcs/RFC-0026-ipc-performance-optimization-contract-v1.md
  - Last-sender EOF: docs/rfcs/RFC-0079-ipc-last-sender-eof.md
  - Waits without clocks (§7), routing v2 (§1): docs/rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md
  - Lock classes / cpu0 right-of-way: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md
  - Stage fence: docs/adr/0062-boot-stage-fence-and-readiness-barriers.md
  - The ONE request/reply exchange today: userspace/nexus-ipc/src/exchange.rs
  - Contract seeds (P0): docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md, docs/adr/0064-request-reply-one-trap-per-side-direct-handoff.md
---

## End-state rewrite 2026-09-15 (binding; supersedes the 2026-09-09 rewrite where they differ)

**Review verdict 2026-09-15 (three-lens architecture review, user decisions recorded):**
the idea behind this task — small control-plane messages are bounded, allocation-free and
predictable; request/reply is the hot path and costs ONE trap per side with a direct
handoff; bulk travels as VMO; every budget is a printed number — is the canonical
microkernel form and stays. The 2026-09-09 text is superseded because its premise
(pairing helpers everywhere), its numbers (ADR-0063, marker paths), its dependency line
(parking on `@ready`) and its package order are stale, and because it did not know two
kernel facts that decide the design: `Error::Reschedule` re-executes a syscall, and the
kernel has no cross-address-space copy. User decisions: (1) the end state covers BOTH sides
— `ipc_call` and `ipc_reply_recv`, handoff in both directions; (2) measurement (P1)
calibrates the budget constants only; the fastpath is end-state structure and is built, and
RFC-0096 amends RFC-0005's deferral paragraph explicitly.

- **Scope** — in: kernel `ipc/{payload,stats}.rs`, `syscall/api/ipc_call.rs`,
  `task/completion.rs` plus minimal match arms in `ipc_msg.rs`/`handler.rs`/`task/mod.rs`/
  `budgets.rs`; `nexus-abi` wrappers + public payload constants; `nexus-ipc` `exchange`
  (client) and `KernelServer` (server) + the deletion list in D5; every consumer only
  through those two APIs. Out: a lock-free class for `call`, a cross-AS copy primitive,
  raising `MAX_SYSCALL` (58/59 fit), the fire-and-forget / push-subscription / waitset
  primitives, the VMO splice poll in `updated/apply_os.rs` (bulk path → TASK-0033), the
  network family.
- **Invariant** — a `call` ends only by the reply or the peer's death; a committed syscall
  is never re-executed; a reply completes only the waiter registered on the endpoint of
  the moved reply cap; identity is kernel-stamped on both legs; ≤ `IPC_SHORT_MAX` ⇒ 0 allocations,
  > 8 KiB ⇒ `E2BIG`. Proven by `test_reject_call_without_reply_cap`,
  `test_reject_call_reply_cap_foreign_endpoint`, `test_reject_oversized_inline`,
  `test_reject_reply_recv_without_recv_right`, `test_call_completes_on_peer_death`,
  `test_call_commit_is_never_reexecuted`.
- **Contract** — RFC-0096 (extends RFC-0026, amends RFC-0005, cites RFC-0079 and RFC-0093
  §7), ADR-0064. SSOT rows: syscall table 58/59 + its `nexus-abi` mirror, payload constants
  in `nexus-abi`, budgets in `core/trap/budgets.rs`, markers in
  `source/apps/selftest-client/proof-manifest/markers/ipc_kernel.toml`, `exchange` /
  `KernelServer`.

### Ground truth 2026-09-15

Userspace
- `userspace/nexus-ipc/src/exchange.rs` is the ONE request/reply exchange since TASK-0324
  P7 (`call` / `call_into` / `call_matching`: clockless, EOF-opted wait on the caller's own
  reply inbox, two traps). Adoption is ~20 %: 20 callers of
  `KernelClient::send_with_cap_move_wait` (`os_kernel.rs:138`) and ~48 hand-rolled
  send-then-recv functions remain (app-host, imed, updated, ingressd, abilitymgr, metricsd,
  keystored, packagefsd, bundlemgrd, execd, `userspace/{statefs,storage,nexus-metrics,
  nexus-vfs}`, `nexus-log`, ten init bootstrap helpers, ~20 selftest-client sites).
- Dead or nearly dead: `connection.rs` (`Connection`/`Transport`, no consumer),
  `reqrep::recv_match` (0 callers) / `recv_match_until` (1), the deadline forms and
  `Clock`/`OsClock`/`deadline_after` in `budget.rs` (reachable only through
  `recv_match_until`), the `os_lite.rs` mailbox (compiled only for `os-lite` without
  `kernel-ipc`; init enables only `os-lite` — reachability to be verified in P2).
- `source/services/app-host/src/svc_call.rs` (pre-P7, last touched 2026-07-31): an 8-frame
  NONBLOCK stale-reply drain (`:44-66`) next to a `nsec()` deadline (`:70-88`, 250 ms via
  `SVC_DEADLINE_NS`, `effect_host.rs:53`); the DSL knob `timeoutMs:` (`effect_host.rs:
  613-623`) is the only consumer of `budget_ns`. The drain is obsolete: the child's reply
  inbox is private and minted per launch (`execd/os_lite.rs:1287-1321`), app-host is
  single-threaded, and only CAP_MOVE replies can land there — the deadline is the sole
  producer of a stale frame.
- Other clock-bound request/reply: `imed/os_lite.rs:401` (`nsec()+200 ms` on
  `ipc_recv_v2`), `app-host/effect_host.rs:764` (`send_fire_and_forget` deadline), execd
  `run_recv_wake_probe` (`os_lite.rs:1066`, a deliberate probe budget).
- **Gate blind spot:** `scripts/check-wait-not-poll.sh` rule 3 matches helper names only,
  never a raw non-zero `deadline_ns` argument to `ipc_send_v1`/`ipc_recv_v1`/`ipc_recv_v2`;
  rule 2 needs NONBLOCK + `yield_()` + bound, and the drain has no `yield_()`.
- `call_matching` carries 14 crates through `nexus_ipc::policyd` (`policyd.rs:210`)
  because the reply inbox is shared with fire-and-forget traffic (statefsd audit appends
  leave unread logd acks queued). Design smell: a reply inbox should see only awaited
  replies.
- The server side is centralized: 37 files go through `KernelServer` / `ReplyCap`
  (`recv_request_with_meta_into`, `reply_and_close(_wait)`), 8 hand-rolled `ipc_recv_v2`
  loops remain (selftest policyd + settings_watch, imed, ingressd, inputd/wait, netstackd
  facade, policyd, windowd region). `nexus-service-entry` has no server loop.
- `tests/sdk_surface` asserts `docs/dev/sdk/crates.toml` only — no symbol surface.

Kernel (`source/kernel/neuron/src/`)
- Syscalls: `MAX_SYSCALL = 64`, next free number **58** (4 and 27 are retired and never
  reused); `register()` asserts on duplicates; numbers are mirrored as local constants in
  `nexus-abi/src/syscall/ipc.rs`.
- `ipc/mod.rs:66-97` `Message { header, payload: Vec<u8>, moved_cap, capmove_expected_ep,
  sender_service_id }`; a non-empty send costs ≥ 2 heap allocations (`ipc_msg.rs:204,260`).
  No inline path exists.
- The 8 KiB cap exists three times (`ipc_msg.rs:60`, `ipc_recv_v2.rs:64`, `endpoint.rs:44`)
  and fails with `EINVAL`, not `E2BIG`; RFC-0005 still documents "initially 512".
- No handoff: a reply is `ipc_send_v1` → `pop_recv_waiter` → `tasks.wake` → per-CPU
  runqueue enqueue + a cross-hart IPI when homes differ (`task/mod.rs:1029-1088`).
  `BlockReason` (`task/mod.rs:50-77`) = IpcRecv / IpcSend / WaitChild / Waitset / Fence,
  each with a `deadline_ns` and four mandatory match sites (`task/mod.rs:1008`,
  `api/mod.rs:264,327`, `runtime.rs:507`).
- **`Error::Reschedule` re-executes the syscall** (`syscall/mod.rs:214`, `handler.rs:83`);
  send/recv are re-entrant by rolling the moved cap back (`ipc_msg.rs:311,338`). A `call`
  that has sent its request is not. The only precedent for "write the result into a saved
  frame" is `core/trap/phased.rs:83-90` (`sepc + 4`, `x[10]`), never from a wake path.
- No cross-address-space copy primitive; every user copy runs under the current task's
  SATP.
- Lock classes: no `LockClass` type, three `matches!` arms (`handler.rs:549,647`,
  `api/mod.rs:450` — lock-free = `nsec` only). IPC is BKL; `record_ecall_hold`
  (`handler.rs:661`) measures every BKL syscall into `KSELFTEST: bkl budget ok`.
- EOF (RFC-0079, P7): `eof_pending` (`endpoint.rs:36`), the one scan
  `api/eof_scan.rs:18-57` (owner excluded, in-flight moved SEND caps count as a peer), wake
  through `drain_recv_waiters`. A `call`-blocked task MUST be a recv waiter of its reply
  endpoint. `ipc_recv_v2` rejects `IPC_SYS_EOF` (`ipc_recv_v2.rs:73`); `ipc_recv_v1` accepts.
- Routing v2 parks in init (`route_park.rs`) on unresolvable targets only, never on
  `@ready` (RFC-0093 §1 amendment 2026-09-09); the stage fence orders (ADR-0062).
- LOC: `ipc/mod.rs` 718/718 (baselined, may not grow), `ipc_msg.rs` 579, `task/mod.rs`
  1292, `handler.rs` 1224, `sched/mod.rs` 759 (all baselined) — new code lands in new files.
- Tests: `ipc/mod.rs:495-717` (incl. a 2000-step router state-machine fuzz),
  `api/tests.rs` (1653 LOC). **No** op-count / alloc-count test, **no** IPC latency number
  anywhere in the repo.

Contracts / proof
- RFC-0005 §"Copy-in/out now, zero/low-copy later" defers handle-attached messages behind
  four criteria. CAP_MOVE (one handle per message) IS landed with `cap_close`, rollback
  tests and the router fuzz — criteria 1 + 2 hold; `ipc_call` composes existing semantics
  and adds no handle semantics. RFC-0096 amends that paragraph and the 512 → 8192 number.
- RFC-0026 "IPC performance optimization v1" (Complete) owns the control/data-plane split
  as prose; nothing enforces it. RFC-0096 is its v2 and extends it by reference.
- RFC-0093 §7 (waits without clocks): `ipc_call` / `ipc_reply_recv` carry NO deadline
  argument — clocklessness by ABI.
- Proof pattern to copy: `core/trap/budgets.rs` (constants, reset window after bring-up) +
  `syscall/api/sched_telemetry.rs:119-135` (numbers inside the marker, prefix gate via
  `count_lines` in `scripts/qemu-test.sh`). Marker manifest:
  `source/apps/selftest-client/proof-manifest/markers/ipc_kernel.toml` (+
  `profiles/harness.toml`). `tools/nx/chains/markers.txt` is the hop-ladder registry, not
  a budget registry. The "TASK-0318 pattern" exists only as a Draft ledger.
- Numbers: RFC-0096 is free (0094 content transfer, 0095 screencap, 0097 payload VMO are
  reserved by sibling ledgers). **ADR-0063 is taken** (proof-lane envelope) → **ADR-0064**.
  Syscalls 58 / 59.

Zero-copy ground truth 2026-09-15 (user direction: zero copy everywhere, above all in IPC;
what Cap'n Proto has done for it so far)
- **Cap'n Proto is NOT the OS IPC control plane and never was.** Every service gates capnp
  behind `std` / `idl-capnp`; the OS image builds `--no-default-features --features os-lite`,
  and `docs/standards/BUILD_STANDARDS.md` treats capnp leaking into `os-lite` as a build bug.
  On the wire in the running OS: hand-rolled packed-LE frames only (`nexus-wire`, ADR-0051;
  `nexus-display-proto`, ADR-0038; vfs `splice.rs`; `blockproto`). ADR-0038 and ADR-0051
  measured and rejected capnp for tiny frames: segment table + pointers + 8-byte words make
  the encoding larger than the fields (≥ 24 B floor before any payload — three quarters of the
  measured 32-byte tier). RFC-0005 §"Relationship to our existing IDL + filebuffer/VMO hybrid" ("Control
  plane: Cap'n Proto frames … `vmoHandle :UInt32`") and ADR-0021's "applies to IPC contracts"
  are the paper version; CAP_MOVE of the VMO cap is the shipped version. RFC-0096 states the
  non-adoption for the inline tier explicitly (cites ADR-0038/0051) and extends its RFC-0005
  amendment to that paragraph; ADR-0021 gets a supersession note.
- **Cap'n Proto IS the zero-copy data-plane reader, no_std on riscv64, boot-proven.** The
  canonical chain is the OTA path: vfsd `OP_READ_VMO` (payload first, `NXVR` header last =
  release fence) → `updated` `vm_map(vmo, RO)` (`apply_os.rs:152`) → `mapmem::ro_slice` →
  `capnp::serialize::read_message_from_flat_slice_no_alloc` over the mapped pages
  (`userspace/updates/src/component_set.rs:328-335`; capnp `alloc` + `unaligned`). Second
  instance: `nexus-dsl-ir` reads `.nxir` in place (`SingleSegment`, `AlignedBytes`, bounded
  `ReaderOptions` — `userspace/dsl/ir/src/read.rs`). `NXPL`'s 16-byte header is 8-byte
  aligned *for capnp* (`nexus-wire/src/bundlemgrd.rs:159-160`).
- **The de-facto bulk contract (three independent instances agree):** (1) the consumer
  allocates and sizes the VMO; (2) the VMO travels as the message's moved cap, never as a
  handle integer; (3) the server is the only writer, payload first, header last; (4) the
  consumer maps read-only; (5) oversize is `E2BIG`, never truncation. Shipped by vfsd
  splice (RFC-0072/TASK-0295), bundlemgrd `OP_ARM_VMO` + reply-cap wait (no header poll),
  windowd→gpud framebuffer attach (RFC-0059), execd's shared RO atlas (RFC-0080,
  `vmo_share_readonly`). Only `vm_map` is a true zero-copy read; `vmo_read`/`vmo_write` copy.
- **Copies that remain (bulk path, parked to TASK-0033 / RFC-0097, not this task):**
  `vfsd/splice_os.rs:139-147` `pkg:/` = three copies; `nexus-vfs/src/lib.rs:584-638`
  `read_vmo` polls the header and `vmo_read`s the whole payload into a `Vec` instead of
  mapping; `app-host/probe/boot.rs:32-53` `vmo_read`s a capnp `.nxir` into `AlignedBytes`
  although the reader is zero-parse; `updated/apply_os.rs:122-138` polls the splice header
  (`SPLICE_POLL_MAX`) although the `OP_ARM_VMO` + reply-cap pattern already solved it;
  ADR-0042 app surfaces blit one copy per damaged rect by decision. RFC-0047's honesty rule
  applies: a non-zero-copy path is named and excluded from zero-copy claims.
- **Bounds that must become ONE:** `nexus-ipc` carries a 512-byte inline ceiling three
  times (`exchange.rs:42`, `os_kernel.rs:203/286/314`, `os_lite.rs:51`) against the
  kernel's 8 KiB cap — `os_kernel.rs:307-313` records that this mismatch once turned a
  real OTA failure into a "bad signature" report. P2 puts every receive buffer on
  `IPC_PAYLOAD_MAX`. `INLINE_IO_MAX = 4096` (vfs surface policy, RFC-0071/0072) and
  `IPC_PAYLOAD_MAX = 8192` (transport) are two numbers with two owners — RFC-0096 says so.
- **Live pulls toward capnp on the OS wire, for the user to decide, not this task:**
  TASK-0317 (vfs v2 write ops on `vfs.capnp`), RFC-0066 P4 (IDL-typed proxies), queryd
  (host-loopback only until its capnp glue is no_std), TASK-0163 (IDL freeze, Draft).

### Goal (end system)

A written IPC performance contract (RFC-0096) and the kernel fastpath sized to it:
`ipc_call` — request + wait for the reply in ONE trap, the reply handed off to the caller
without a runqueue hop — and `ipc_reply_recv` — reply + wait for the next request in ONE
trap, the request handed off to the server; payload ≤ 32 B inline without kernel heap (the size
P3a measured); an
8 KiB hard cap with `E2BIG`; budgets measured as numbers and gated. Userspace has ONE
request/reply API (`exchange`) and ONE server loop (`KernelServer`), both on the new
syscalls; every pairing / deadline / drain form is deleted and a gate makes it impossible
to build again.

### Non-goals

A new IPC primitive or ABI redesign beyond the two syscalls; lock-free router experiments;
MM changes; any change to `sender_service_id`, CAP_MOVE rights / rollback, or RFC-0079 EOF
semantics; a cross-address-space copy primitive (copy-out happens on resume in the
waiter's own address space).

### Invariants

- RFC-0005 semantics preserved; identity stamped by the kernel on every path, including
  the request delivered through `ipc_reply_recv`.
- `IPC_SHORT_MAX = 32` (inline, zero allocations — measured in P3a, see the result under P3a
  below) and `IPC_PAYLOAD_MAX = 8192` (`E2BIG`
  above) are public constants in `nexus-abi`; the kernel-private `MAX_FRAME_BYTES` is
  deleted.
- `ipc_call` / `ipc_reply_recv` have NO deadline argument (RFC-0093 §7). Exactly two things
  end a `call`: the reply, or the death of the last peer (EOF → `EPIPE`).
- A committed syscall is never re-executed: the commit point is "request / reply enqueued";
  from there the peer (or the EOF scan) completes the syscall through the task's call state.
- Hot path bounded: no queue scan, no logging, no heap for ≤ `IPC_SHORT_MAX`.
- Budgets measured under `smp1` + icount, printed with their numbers, asserted — never a
  bare `ok`.
- Gates at zero: wait-not-poll (new rule 4: a raw non-zero `deadline_ns` on an IPC
  syscall), slot SSOT, init-sync, structure (baseline only shrinks), FAIL markers.

### Decisions

- **D1 Contract.** RFC-0096 "IPC performance contract v2: `call` / `reply_recv` fastpath"
  extends RFC-0026, amends RFC-0005 (deferral paragraph satisfied and replaced, 512 → 8192)
  and cites RFC-0079 / RFC-0093 §7. ADR-0064 "Request/reply is one trap per side with a
  direct handoff; the reply completes the caller's syscall". Budget constants in
  `core/trap/budgets.rs`: `IPC_CALL_RT_BUDGET_US`, `IPC_CALL_HANDOFF_MISS_BUDGET`,
  `IPC_CALL_ALLOCS = 0` — values from P1.
- **D2 Two syscalls, one mechanism.** `SYSCALL_IPC_CALL_V1 = 58` (send slot, header with the
  CAP_MOVE reply cap, request, out buffer + length, `sys_flags ∈ {TRUNCATE}`) and
  `SYSCALL_IPC_REPLY_RECV_V1 = 59` (reply slot, reply header + frame, an `IpcRecvV2Desc` for
  the next request incl. `sender_service_id`). Both: phase 1 re-entrant (target queue full →
  `BlockReason::IpcSend` as today, nothing committed); phase 2 commit (enqueued, `sepc + 4`,
  task in `BlockReason::IpcCall { reply_ep }` resp. `IpcRecv`, registered as recv waiter,
  call state `{ out_ptr, out_len, staged }` in the new `task/completion.rs`); phase 3
  completion by the peer: an inline reply into the waiter's `IPC_SHORT_MAX`-byte stage buffer, a heap
  reply as a parked `Message`, `x[10]` written; copy-out into the user buffer in the
  waiter's trap return under its own SATP (one hook in the `handler.rs` epilogue). The EOF
  scan completes with `EPIPE`. Lock class BKL; `record_ecall_hold` measures automatically.
  The reply cap moved by `ipc_call` must be a SEND cap to the endpoint the caller waits on
  and the caller must hold RECV on it (`test_reject_call_reply_cap_foreign_endpoint`).
  `ipc_reply_recv` does NOT opt into EOF (a server owns its endpoint; "all clients gone" is
  not a server error).
- **D3 Handoff rule (fixed in the RFC).** When a send finds the receiver blocked in
  `IpcCall` (or in `IpcRecv` entered through `ipc_reply_recv`) and the receiver's affinity
  admits the current hart, the hart switches to the receiver directly — no enqueue, no
  IPI; otherwise enqueue + `request_resched` as today. Counters `handoff_hit`,
  `handoff_miss`, `reply_wake_ipi` in `ipc/stats.rs`.
- **D4 Payload.** `ipc/payload.rs`: `Payload::{ Inline { len, [u8; 64] }, Heap(Vec<u8>) }`,
  `Message.payload: Payload`, ABI unchanged. Over the cap → `E2BIG` (new errno in
  `nexus-abi` → `IpcError::TooBig`).
- **D5 Userspace: ONE API per side, the old forms deleted.** Client: `exchange::call_into`
  (+ `call_matching` only if P2 proves a structurally shared inbox; the goal is "every
  reply inbox sees only awaited replies" — statefsd audit appends without a reply cap, then
  `call_matching` is deleted). Server: `KernelServer::reply_recv`. Deleted:
  `send_with_cap_move(_wait)`, `Connection` / `Transport`, `reqrep::recv_match*`, the
  deadline forms + `Clock` / `OsClock` / `deadline_after` in `budget.rs`, `exchange::call`
  (the `Vec` form), the `os_lite.rs` mailbox (if unreachable), app-host's `svc_call.rs`
  drain + `SVC_DEADLINE_NS` + the DSL `timeoutMs:` knob (an app-visible removal, named in
  CHANGELOG + DSL docs), imed's deadline, execd's probe moved onto the timer-notify pair.
  `raw::recv_blocking` without EOF goes.
- **D7 Zero-copy rule (RFC-0096 §"Copies").** A control message (≤ `IPC_SHORT_MAX`) costs
  the kernel zero heap and exactly the two register-sized copies a cross-address-space
  transfer needs (user → the waiter's stage, stage → user on its own return); the kernel
  never stages a payload on the heap for that tier. Bulk never travels through the kernel:
  it follows the shipped VMO contract above (consumer-allocated VMO as the moved cap, payload
  first / header last, consumer `vm_map`s read-only, capnp readers run in place, oversize
  `E2BIG`). No Cap'n Proto framing on the inline tier (ADR-0038/0051). The P1 counters
  (`copies`, `copy_bytes`, `heap_allocs`) are the baseline: P1 measured two allocations and three
  payload copies per non-empty message (user → heap, heap → heap clone, heap → user); P3a deleted
  the clone, so P3b starts from ONE allocation and two copies (measured at exactly 1.000
  allocations per message over three windows).
- **D6 Bench gate.** Host: a counting `#[global_allocator]` in the kernel host tests
  (0 allocations for ≤ `IPC_SHORT_MAX` send / recv / call) and router op counters in the state machine,
  both in the `just test-all` kernel stage. QEMU: `KSELFTEST: ipc call budget ok (rt=<n>us
  handoff_miss=<m> alloc=0)` after the reset window (pattern `sched_telemetry.rs`),
  registered in `markers/ipc_kernel.toml` + `scripts/qemu-test.sh` headless / smp1. Real
  load: `KSELFTEST: ipc stats (...)` at the ShellVisible fence in the `visible` lane.

### Packages (one commit each; approval zones named per package)

- **P0 Paper** — this ledger section, the IMPLEMENTATION-ORDER row (ADR-0064,
  `call` + `reply_recv`), RFC-0096 seed (index row; fix the RFC-0093 index text "replies
  parked until the target's `@ready`"), ADR-0064 seed (index row), architecture-review
  verdict above, approval question for kernel + libs + abi. Zones: `docs/rfcs`. Blast: paper.
- **P1 Measure** — `ipc/stats.rs` (sends, heap allocs, wake IPIs, handoff = 0) +
  `KSELFTEST: ipc stats (...)` (pattern `sched_telemetry.rs`); selftest-client `SELFTEST:
  ipc bench (rt=<n>us hops=<h>)` ping-pong over today's two-trap `exchange` (no assert);
  the stats line at the ShellVisible fence. Zones: kernel (small). Lanes: headless, smp1,
  visible. Result → the D1 budget constants.
- **P2 Userspace ONE API** — three sub-packages (inventory corrected 2026-09-15 at P2 start:
  the raw-`deadline_ns` scan finds **31** sites, not 3; `reqrep::ReplyBuffer`/`FrameStash`/
  `recv_match` are ALIVE in dsoftbusd, keystored, execd and init — a shared-inbox
  correlation model, not dead code):
  - **P2-a Clocks out of request/reply, dead forms deleted.** `Wait::Timeout` deleted from
    the `Wait` enum (clocklessness by type), `budget.rs` reduced to the route ask + the raw
    blocking send (`Clock`/`OsClock`/`HostClock`, `deadline_after`, `remaining`,
    `send_until`, `recv_until`, `recv_matching_until`, `*_budgeted`, `raw::recv_blocking`
    gone), `connection.rs` (`Connection`/`Transport`, no consumer) and `reqrep::
    recv_match_until` deleted, `exchange::call` (the allocating form) and `MAX_REPLY` deleted;
    app-host `svc_call.rs` = `exchange::call_into` (drain, `SVC_DEADLINE_NS`, `budget_ns`,
    `send_fire_and_forget` deadline gone; the DSL `timeoutMs:` argument is ignored and handed
    to TASK-0077B for removal from the language); imed `set_setting` = one exchange; windowd
    `Delivery::deadline_ns()` → `parks()` (a client-bound send parks until the client drains
    or dies — the 16 ms / 250 ms bounds only chose between two parks); selftest `net_rpc`/
    `expose` = `call_matching`, the four `exchange::call` sites = `call_into`, the loopback
    round trip waits, `ipc_deadline_timeout_probe` + `SELFTEST: ipc deadline timeout ok/FAIL`
    retired (no userspace wait carries a deadline; the kernel's semantics stay host-tested);
    wait-not-poll rule 1 retires every deleted name incl. `Wait::Timeout`, rule 3 is absolute
    (no transport exclusion). Zones: scripts (`check-wait-not-poll.sh`, `qemu-test.sh` marker
    list), docs/rfcs (RFC-0005 marker note). Blast: nexus-ipc consumers, app-host, imed,
    windowd, selftest. Lanes: `just test-all` + visible.
  - **P2-b Pacing and watchdog waits onto timer-notify pairs; rule 4 absolute.** The
    remaining raw non-zero `deadline_ns` sites are not request/reply but pacing or device
    watchdogs: ingressd `PARK_NS`, netstackd facade `FACADE_PARK_NS` (smoltcp poll cadence),
    policyd `recv_with_meta_deadline`, hidrawd `idle_park` (re-probe cadence), gpud
    `block_on_irq` (lost-IRQ recovery), virtio-blk completion wait (2 s device timeout),
    execd `run_recv_wake_probe` (four deadlines = the probe's FAIL witness), selftest
    `settings_watch::{recv_event,settle}`. Each becomes a kernel one-shot/periodic timer on
    a declared timer-notify pair as a waitset member (the inputd/settingsd/gpud pattern;
    pairs declared in `nexus-service-topology`, pinned by init `declared_routes::
    pin_timer_notify`); a device watchdog is a timer, never a recv deadline. Then rule 4
    (`ipc_send_v1`/`ipc_recv_v1`/`ipc_recv_v2` with a non-literal-`0` deadline argument) lands
    at zero with a fixture. Zones: libs (topology), init, drivers, scripts.
  - **P2-c ONE send primitive; every reply inbox sees only awaited replies.** The inventory
    (2026-09-16, below) splits the old single P2-c in two: the invariant and the pairing
    helper here, the correlation model in P2-d. Here: `exchange::send_with_cap` — the ONE
    form for a send whose moved cap is DATA (a VMO, a push-channel SEND), so no service
    hand-builds a `MsgHeader` with `CAP_MOVE` again, plus `exchange::send_nonblocking` for a
    frame nobody answers. Three of the four fire-and-forget sends that moved a REPLY cap
    nobody reads stop moving it (`nexus-log`'s logd sink + its `drain_reply`, statefsd's
    `append_logd_audit`, policyd's `append_logd_deterministic` + its 8-frame pre-drain); the
    fourth, metricsd's retention, is the documented exception below. All 23
    `send_with_cap_move(_wait)` callers onto `exchange::{call_into, call_matching,
    send_with_cap}` and `KernelClient::send_with_cap_move(_wait)` deleted; `SlotPair`
    re-exported from `nexus-ipc`; the bare single-`recv_reply` readers on shared inboxes
    given their protocol's predicate; dsoftbusd's two divergent `rpc_nonce` bodies unified on
    `call_matching`, which takes its 500 ms deadline, its 200 000-spin loop, its
    `Wait::NonBlocking` request send and its `ReplyBuffer` threading (eight files) with them;
    the `cap_close` of a cap CAP_MOVE already consumed on the SUCCESS path deleted; the last
    cap-less logd request deleted as a duplicate of `logd_stats_total`; rule 1 retires
    `send_with_cap_move`; LOC baseline shrunk. Zones: libs (`nexus-log`), config, scripts,
    `docs/rfcs` (three contract notes). Blast: every service, init, selftest.
  - **P2-d ONE correlation model: one inbox, one awaited exchange.** Every hand-rolled
    `cap_clone` + `MsgHeader CAP_MOVE` + `ipc_send_v1` + `ipc_recv_v1/v2` client pair onto
    `exchange`, and `nexus_ipc::reqrep` (`NonceGen`, `ReplyBuffer`, `FrameStash`, `recv_match`)
    deleted with its `pub mod`. Measured surface at P2-c's close: 44 `ipc_hdr::CAP_MOVE`
    occurrences in 28 files outside `nexus-ipc`, of which 36 were client sends. **init's
    outbound asks get their own minted inbox**, which is the one behaviour change in the
    package: `pol_ctl_route_rsp` carried policyd's verdicts AND init's bootctld/bundlemgrd/
    updated answers, so its four stashing readers coexisted with two that failed CLOSED on a
    frame they did not recognise (a route ask → deny, an MMIO cap check → aborted boot). Two
    forms complete `exchange`'s send matrix for cases the inventory turned up:
    `send_call` (an exchange whose answer is collected later — execd loads the ELF while
    bundlemgrd streams, settingsd serves while statefsd commits) and
    `send_with_cap_nonblocking` (a cap-moving registration the caller RETRIES rather than waits
    out — windowd must not block its frame loop on settingsd). Rule 1 retires the deleted names
    with a fixture; RFC-0019's mechanism gets a superseded-by note. Zones: libs
    (`nexus-service-topology` untouched; `storage` gains a `nexus-ipc` dep), init, config,
    scripts, `docs/rfcs`. Blast: init, keystored, execd, rngd, abilitymgr, imed, ingressd,
    inputd, app-host, windowd, updated, settingsd, statefs, storage, selftest.
  - **P2-e A server answers exactly the senders that moved a reply cap.** Seeded as "the
    one-way statefs write op", rewritten 2026-09-16 after the survey: the idea behind a
    one-way write is "do not make a client await what it does not need", and the best
    realization is not a new wire op but the rule logd already follows since P2-c. Applied to
    **metricsd**, which is where it pays: bundlemgrd's fire-and-forget counters were the only
    cap-less senders, metricsd answered them on its own response endpoint **with a blocking
    send**, and NOTHING in the tree reads that endpoint — the acks piled up in an 8-deep queue
    and the next one would have blocked the metrics sink for good. A live, armed 0049B wedge,
    found by asking whether the rule was safe to apply. Also here: **statefsd's cap-less reply
    was flipped from `Wait::NonBlocking` to `Wait::Blocking` by the clock sweep (TASK-0324
    P7-d, `e70091b2`) while the comment above it kept saying "Drop IMMEDIATELY on a full
    queue"** — a second armed wedge, in the store every service depends on, restored to what
    its own comment describes; the send is not a clock, it is a drop. The dead
    `Option<reply inbox>` in `StatefsClient` and `MetricsClient` is deleted (unreachable in
    every OS build: `@reply` resolves for every caller, and an unresolvable inbox is now a
    construction failure). The selftest `updated` reply pump's 256-frame pre-drain of the
    harness' shared `@reply` inbox — it CONSUMED and discarded other probes' awaited replies —
    is gone with its `VecDeque` stash and ~170 threaded parameter positions across the OTA
    probe chain. Zones: libs (`nexus-service-topology` test, `nexus-metrics`), config, scripts.
  - **P2-e finding: the metricsd exception's cause is LATENT, not active.** P2-c recorded that
    awaiting statefsd from metricsd deadlocks because statefsd's quota gate waits on metricsd
    from inside its PUT handler. The chain is real, but it cannot close today for ONE reason:
    no `statefsd → metricsd` route is declared, so the deny counter's route ask fails and it
    latches itself off after one attempt. That was an accident of the topology; it is now a
    rule — `test_reject_route_that_would_close_the_metrics_wait_cycle` fails the build for
    statefsd, policyd and ingressd, the three services that flush a `DenyCounter` from inside
    a request handler.
  - **P2-f statefsd adopts the reply rule; the invariant has no exception left.** The three
    blockers turned out smaller than the seed feared. The selftest ladder is ONE helper
    (`statefs_send_recv_deadline`), not 50 edit sites — its ~30 callers keep their signatures.
    dsoftbusd's proxy is one function. And the `demo.minidump` payload needs no new
    instruction at all: its `MsgHeader` is built at COMPILE time, so the moved slot number and
    the `CAP_MOVE` flag are constants in the generated image; execd grants it a private
    endpoint the same way it mints the app event and timer channels. Both statefsd route
    declarations flip `SharedResponse` → `ReplyInbox` (the request endpoint is unchanged —
    `request_ep` falls through to the server pair — only the RECV moves to the caller's inbox).
    With that, statefsd answers exactly the senders that moved a reply cap, its shared response
    queue has no reader and no writer, and **P2-c's metricsd exception is closed**: the
    retention writes move no cap, so no ack exists. Zones: libs (topology), config, scripts.
  - **P2-c/P2-d inventory (2026-09-16).** 23 `send_with_cap_move(_wait)` callers and 21
    hand-rolled pairs, classified: (1) seven callers move a VMO or a push-channel SEND, not
    a reply cap — `exchange` had no form for them, which is why `ipc_send_v1` kept leaking
    into service code; `send_with_cap` is that form. (2) **The invariant is violated in four
    places**, each a send that moves a reply cap onto a SHARED inbox whose ack nobody reads:
    `nexus-log`'s sink (bound by six processes), statefsd's audit append (the smell named in
    `nexus-ipc/src/policyd.rs`), policyd's audit append, metricsd's retention writes. Three
    are answered by NOT moving the cap — logd journals the record before it decides where to
    reply and simply drops the response for a sender without one (`logd/src/os_lite.rs`),
    which is already execd's crash-append form. The fourth, metricsd's retention writes, is the
    ONE documented exception: statefsd ALWAYS answers, so a cap-less write is answered on its
    shared response queue with a blocking send (the 0049B wedge) — and awaiting the status
    deadlocks, measured: statefsd's quota gate runs inside its PUT handler
    (`abi_seam_os::put_gates` → `QuotaState::admit_put`), a deny flushes `quota_denies_total`
    through `nexus_metrics::DenyCounter`, and that flush WAITS for metricsd's answer, so a
    metricsd waiting on statefsd and a statefsd waiting on metricsd close a cycle. The fix is a
    one-way write op in the statefs protocol — a wire change, hence P2-d with its own seed. (3) Two second-order bugs fall out: `nexus-log`'s
    `drain_reply` discards up to four frames of ANY kind from an inbox whose exchanges it does
    not own, and `updated/src/os_lite.rs` already reads a stale logd ack as its bundlemgrd
    answer (`reply-malformed`) — the invariant is not a future risk, it is a live defect.
    (4) The rule "logd answers exactly the senders that moved a reply cap" had one hole that only
    a boot could find: the harness asked logd for its record count with a PLAIN send and read the
    answer on logd's own shared response endpoint — the only request in the tree that did, and a
    byte-for-byte duplicate of `logd_stats_total`, which asks the same op over the CAP_MOVE reply
    inbox. Deleting the duplicate closed it. Reading the code said nobody read that endpoint; the
    marker ladder said otherwise, in one hung phase.
    (5) No service in the fleet spawns a thread, so there is no true request concurrency and
    nothing needs PARKING: every out-of-order reply comes from an abandoned clock-bounded
    exchange (dsoftbusd only), a fire-and-forget ack, or two protocols sharing one inbox.
    `recv_match` does not even park a foreign protocol — it discards it (`reqrep.rs`), so the
    stashes never delivered what their header claims. (6) `ReplyBuffer`/`FrameStash` are
    already dead weight: rngd threads one through four signatures for a call site that
    ignores it, keystored through ten for one, execd's is provably always empty.
  - **P2-b findings (2026-09-15):** (1) a waitset reports a timer member READY once without a
    fire — the kernel's EOF latch is set on an endpoint whose only SEND cap is its owner's
    (init closes its minting cap) — so every timer wake is confirmed by `NotifyTimer::drain()`;
    (2) core-plane services (virtioblkd, policyd, bundlemgrd) run before the wiring phase —
    declared timer endpoints are minted and pinned in the spawn-time pass, once per service;
    (3) **kernel:** the EOF latch was cleared by every receive, so a peer that wrote and died
    left a waitset blind to its death once its last frame was received (execd's probe); the
    latch is now consumed only when a receiver observes an empty queue (`ipc_msg.rs`,
    `ipc_recv_v2.rs`), and the probe's two markers are required by the harness; (4) gpud's
    device bring-up commands wait before the IRQ is bound — the watchdog exists from queue
    creation, the IRQ attaches later (one shared watchdog for both queues); (5) the fleet-
    reserved slots (`DEVICE_MMIO_SLOT` 0x30, `INPUT_MMIO_SLOTS` 0x32–0x34, `STAGE_FENCE_SLOT`
    0x38) are caught by `test_reject_slot_in_reserved_range`; (6) structure gate: the grown
    files split into child modules (`virtqueue/ring_wait.rs`, `mmio/watchdog.rs`,
    `os_lite/recv_wake_probe.rs`, `slots/selftest_client.rs`) that see their parent's
    private fields.
  - Parked (not this task): the selftest ingress probe's socket-status retries
    (`WOULD_BLOCK` from the facade's non-blocking socket ops, bounded by
    `STEP_DEADLINE_NS`) are the network family's poll and go with blocking/notify socket
    semantics there.
- **P2-g The server hot path allocates nothing, and the login seam gets a gate.**
  Opened 2026-09-17 on a user report ("kein handoff nach greeter") against the boot of
  2026-09-17T10-05-44. The survey moved this package TWICE before a line of fix was written,
  and both moves are recorded because each one is a measurement that killed a plausible story.
  - **What the report actually is.** The greeter DOES come up (the pixel proof of the same build
    shows wallpaper, avatar, password field and the three session buttons). What is missing is
    the handoff AFTER it: no `apphost: dsl svc session.login`, no `windowd: dsl login detected`,
    no `windowd: session shell visible`, no `desktop-shell` launch. The login was never pressed,
    because input died first: at 14.82 s hidrawd reports `inputd send fail backpressure` and from
    19.5 s on runs at `rx hz=351 ev hz=1088 tx hz=0` — HID events arriving, nothing forwarded.
  - **Killed story #1: "windowd's shared response endpoint filled".** windowd answers cap-less
    senders with `server.send(.., Wait::Blocking)` on its OWN shared response endpoint in five
    branches — the 0049B pattern P2-c/e/f removed from logd, metricsd and statefsd, and one that
    would stop the COMPOSITOR. (It also shows P2-f's "the invariant has no exception left" was
    too strong: **28 files** fleet-wide still answer that way.) So the branches were collapsed
    into ONE counted function (`compositor/reply_route.rs`, which also took `compositor/mod.rs`
    from 618 to 605 LOC) that announces the fallback at every power of two — the endpoint depth
    is 8, so `n=8` IS the wedge. Measurement: **`reply(ch=5 cap=0 shared=0)`** in a boot AND under
    the flood below. The fallback is stone cold. Story dead; the instrument stays, because it is
    now the thing that would notice if it ever warmed up.
  - **Killed story #2: "hidrawd cannot drain inputd's acks".** inputd's ack frames are up to 70 B
    (`VISIBLE_STATE_FRAME_LEN`) and hidrawd drains with a 64-byte stack buffer, which looked like
    a permanent head-of-line block. It is not: `recv_into` passes `IPC_SYS_TRUNCATE`, so the
    oversized frame is consumed, not requeued. Story dead by reading the flag, not by guessing.
  - **The actual cause, with a boot witness.** A QMP pointer flood (900 moves/s for 45 s against
    a live greeter — the harness already drives QMP for its visible-input proof, so the mouse hand
    is scriptable) reproduces the report exactly, and the log says why:
    `heap-watermark svc=inputd` climbs **50 % → 75 % → 90 %**, then
    `alloc-fail svc=inputd site=alloc size=0x36` and `alloc_error svc=inputd`. **inputd dies of
    heap exhaustion.** `KernelServer::recv_request_with_meta` returns a fresh `Vec` per request;
    the os-lite heap never frees; at ~400 HID batches a second, 54 bytes a request walks the
    384 KiB bump heap to death in under twenty seconds. A dead inputd stops consuming, hidrawd's
    sends back up, `tx hz=0`, and input is gone fleet-wide — so the greeter can never be clicked
    through. Same defect class as the hidrawd ingest path fixed in 2026-06
    ([[os-service-bump-allocator-no-free]]); inputd never got it.
  - **The fix is the API the tree already documents as preferred.**
    `recv_request_with_meta_into` takes a caller buffer; windowd's compositor loop already uses
    it. inputd now owns ONE `[u8; MAX_HID_BATCH_FRAME_LEN]` for the whole loop. And because a
    bump heap that never frees turns "slow leak" into "dies later", the remaining **13 os-lite
    server loops** on the allocating form go the same way, with a gate so the allocating variant
    cannot return to a loop.
  - **Why no lane caught it.** No lane floods input, and no lane gates the login seam: the proof
    manifest has no marker for `apphost: dsl svc session.login ok`, `windowd: dsl login detected`
    or `windowd: session shell visible`. P2-g adds both — the flood as a permanent regression lane
    and the three markers to the manifest — so the seam the user noticed is gated from here on.
  - **P2-g result (2026-09-17) — the cause, and the two stories it replaced.** The wedge is
    inputd dying of heap exhaustion, not a reply-routing wedge. Reproduced by script (900 pointer
    moves/s for 45 s against a live greeter) and read straight off the boot:
    `heap-watermark svc=inputd` 50 % → 75 % → 90 %, then `alloc-fail svc=inputd size=0x36` and
    `alloc_error`. `KernelServer::recv_request_with_meta` returns a fresh `Vec` per request and
    the os-lite heap never frees; ~400 HID batches a second × 54 bytes walks 384 KiB in under
    twenty seconds. Counter-proof under an IDENTICAL flood after the fix: **no heap watermark at
    all, no `alloc-fail`, and `tx hz` tracks `rx hz` to the end (3507/3507 where it had been
    `tx hz=0`)**. One transient backpressure remains and recovers — which is what backpressure is.
  - **P2-g result — the rule and its gate.** All 14 os-lite receive loops move to the `_into` API
    with one buffer hoisted out of the loop; `scripts/check-ipc-bounds.sh` rule 3 fails the build
    if the allocating receive returns to a service loop, and rule 2 now scans the whole fleet, so
    three further private copies of the 8 KiB frame cap (statefsd `IPC_MAX_FRAME_BYTES`, keystored
    `MAX_REQUEST_FRAME`, vfsd `PKGFS_REPLY_BUF`) fold into `nexus_abi::IPC_PAYLOAD_MAX`.
    `just input-flood` is a lane in `test-all`: every other lane drives input politely, and that
    politeness is what hid this.
  - **P2-g result — what was NOT changed, and why.** app-host's `REPLY_BUF = 512` sizes a STACK
    array at eight call sites: folding it into the transport cap would put 8 KiB on the app-host
    stack per effect call. RFC-0096 names the 512-byte ceiling as a real hazard, so the end state
    is ONE reusable heap buffer threaded through the effect handlers — recorded, not bumped blind.
    vfsd's per-request `vec![0u8; PKGFS_REPLY_BUF]` in the packagefs hop is the same shape at a
    much lower rate (per `pkg:/` read, not per event); it needs `&mut self` to hoist.
  - **P2-g result — the login seam is named, not yet required.** The three markers
    (`apphost: dsl svc session.login ok`, `windowd: dsl login detected`,
    `windowd: session shell visible`) are registered in the proof manifest with what they prove.
    Requiring them needs the visible injector to drive a REAL login, and the greeter's login is a
    Tap on a `Circle`, not an Enter key — so it needs a hit rect the greeter DECLARES, the way the
    existing injector follows `inputd::visible_contract`. A screenshot-derived coordinate would rot
    exactly like the seam it is meant to protect. That is the next step, and it is now a step.
  - Zones: `source/services/inputd` + the other os-lite loops, `source/services/windowd`,
    proof manifest, `scripts`, `tools`. Blast: every service loop. Lanes: `just check`, the flood
    lane, smp1, visible, `just test-all`.
- **P3a Measure the payload distribution; delete the copy that was already redundant.**
  Split out 2026-09-16 by this task's OWN method: P1 established "numbers before assertions",
  and `IPC_SHORT_MAX = 64` is currently a guess. The only number the instrument had was an
  average (`copy_bytes / copies`), and an average cannot choose a tier size — a few 4 KiB frames
  and many 8-byte ones average like all-medium ones, and this average moves between 30 and 88
  bytes depending on how much idle traffic the window spans. So: a payload-size HISTOGRAM in the
  existing `ipc/stats.rs` instrument, printed next to the stats line, and the boots pick the
  constant P3b builds against. Shipped with it, because it needs no constant and
  no new tier: the `payload.clone()` in `sys_ipc_send_v1` is redundant —
  `Router::send_returning_message` already hands the `Message` back on error, so the clone that
  exists "in case the attempt fails" duplicates a value the failure path already returns.
  Deleting it takes the measured **2.000 kernel heap allocations per message to 1.000**, for
  every message, before any tier exists. Zones: kernel. Lanes: `just test-all`.
  - **P3a result (2026-09-16) — the measurement contradicts RFC-0096's guess, and it is not a
    percentage.** Read over **13 windows from two workload families**: the standard boot (smp1,
    headless, visible, reset, ota-downgrade, ota-tamper — 5505 to 22 285 payloads) and the OTA
    bundle lanes (flip, bundle, resume, delta, fallback — 7554 to 29 880). In the standard boot
    every bucket ABOVE 32 B is constant to a rounding error: `le64` is **exactly 160 in all eight
    windows**, the whole > 32 B population is 1529 ± 5 (le128 454–461, le256 62, le512 264–269,
    le1k 576–579, gt1k 11). That is the boot's FIXED work. Everything that SCALES with the window
    is ≤ 32 B — 3978 → 20 755, i.e. **72.3 % to 93.1 %** of all messages. The OTA bundle lanes
    shift the fixed part without changing the shape: `le64` 526–531 and the 513–1024 B band grows
    to ~1750 messages of bundle streaming, so ≤ 32 B is 53.5–63.6 % there.
    Consequences for D4 / `IPC_SHORT_MAX`: a 32-byte tier covers every message that scales, in
    BOTH families, and raising it to 64 does not buy a percentage — it buys a FIXED **160
    messages per standard boot** (0.7–2.9 points) or **~530 per OTA bundle boot** (1.8–7.0
    points) for twice the inline footprint in every `Message`, which is moved on every send,
    push, pop and error return and sits in `VecDeque`s across 384 endpoints. **Decision for P3b:
    `IPC_SHORT_MAX = 32`**; RFC-0096 is amended with these numbers rather than keeping 64. The
    513–1024 B band is the argument for the VMO bulk path, not for a bigger inline tier. An
    average could not have decided this: the mean payload of those windows runs from 30 B to
    88 B — it tracks idle traffic, not message shape.
  - **P3a result — the redundant copy, measured gone.** The histogram total equals `heap_allocs`
    EXACTLY in all 13 windows, which is what "one kernel heap allocation per payload" means; it
    was 2.000 per message in every run from P1 until this package. `copies / sends` fell by ~1.0
    per message (3.12–3.38 → 2.05–2.64), precisely the one copy removed. Unlike the absolute
    totals (see the P2-c correction), the per-message RATIO holds in every window, which is what
    makes it assertable.
  - **P3a finding, NOT fixed here (out of scope, named so it is not lost).** The send path's
    `debug_uart` block had to be re-read after the payload moved, so it was compiled for the first
    time in a while: `cargo clippy -p neuron --target riscv64imac-unknown-none-elf --features
    debug_uart` fails with 11 errors, ALL in `core/trap/handler.rs` (a drifted `uart_write_hex`
    signature and `Pid as usize` casts) and none in the IPC path — the block this package touched
    type-checks. No lane builds that feature, so it rotted silently. Follow-up: either repair the
    casts and add `--features debug_uart` to `just lint-kernel` so it cannot rot again, or delete
    the feature; do not leave a debug switch in the tree that does not compile.
  - **P3a by-product, for P3b/P4.** `heap_allocs` and the histogram total exceed `sends` by
    exactly **11 in each of the five OTA bundle lanes** and by 0 everywhere else. Those 11 are
    messages the kernel copied in and then failed to enqueue — the pair of counters reports the
    failed-send count for free, with no new counter. Worth a look when P4 calibrates budgets:
    nothing in the OTA path is supposed to lose a send.
- **P3b The payload tier, and ONE size bound with one owner.** Reviewed against the tree
  2026-09-17 before implementation; the survey moved four things, each recorded with what it is
  based on.
  - **The idea.** A short control message must cost the kernel no heap, and the boundary to bulk
    must be explicit, named, and carry its own errno so bulk cannot drift inline unnoticed.
  - **(1) The tier.** `Message.payload` is a `Vec<u8>`, so every non-empty message allocates —
    and P3a measured that 72.3–93.1 % of them are ≤ 32 B. `ipc/payload.rs` holds
    `Payload::{Inline{len,[u8; IPC_SHORT_MAX]}, Heap(Vec<u8>)}`; `Message` carries it; the copy-in
    in `sys_ipc_send_v1` writes into the inline array directly instead of a `Vec`, and
    `record_payload_alloc` fires only on the heap arm. The uses are contained: `len()` (12 sites),
    `as_ptr()` (the two copy-out sites), `as_slice()` (one), and `Message::new`'s truncate.
    **Falsifiable prediction, to be checked by the boot:** after this, `heap_allocs` ≈ the >32 B
    population P3a found — about **1530 per standard boot regardless of window length**, where it
    is 5549 / 14 077 / 22 115 today.
  - **(2) ONE bound.** `8 * 1024` is written FOUR times as a private literal
    (`ipc_msg.rs`, `ipc_recv_v2.rs`, `ipc/endpoint.rs`, `selftest/mod.rs`) and is invisible to
    userspace. `IPC_SHORT_MAX = 32` and `IPC_PAYLOAD_MAX = 8192` become public `nexus-abi`
    constants and the four literals go.
  - **(3) `E2BIG` is ADR-0054 applied to the size bound.** Today an oversize payload returns
    `AddressSpaceError::InvalidArgs` → `EINVAL`: userspace cannot tell "your message is too big"
    from "you passed a bad pointer" — the exact failure class ADR-0054 was written for. New
    `IpcError::TooBig` → `errno(E2BIG=7)` in `core/trap/errno.rs` (whose no-wildcard rule makes
    the arm mandatory), the decode arm in `nexus-abi`, and the four exhaustive matches it breaks
    (`execd/os_lite.rs`, `statefsd/emit_os.rs`, `init/helpers.rs`,
    `selftest-client/.../bundlemgrd.rs`). `nexus_ipc::IpcError` gets NO new variant — it already
    carries the kernel error as `Kernel(..)`. The recv-side check stays `EINVAL`: an out-buffer
    larger than any possible message is a caller bug, not an oversize message. The endpoint byte
    budget keeps returning `NoSpace`.
  - **(4) The last 512-byte ceiling sits in a backend that compiles nowhere — MEASURED.**
    RFC-0096 names a "triplicated 512-byte ceiling"; P2 fixed two, the third is `MAX_FRAME = 512`
    in `userspace/nexus-ipc/src/os_lite.rs`. That file is one of THREE backends exporting the same
    public names (`os.rs` 315 LOC for `os` without `os-lite`, `os_kernel.rs` 361 LOC for
    `os-lite + kernel-ipc`, `os_lite.rs` 242 LOC for `os-lite` without `kernel-ipc`). Probe
    (2026-09-17): a `compile_error!` at the top of `os.rs` AND `os_lite.rs` still builds
    `just build-os-workspace`, `just diag` (host + os + kernel cfgs), `just build-kernel` AND
    `cargo build -p init-lite --target riscv64imac-unknown-none-elf` — the shipped init ELF, the
    one crate whose manifest suggested it might select the lite backend (its graph resolves
    `nexus-ipc` with `kernel-ipc` too). Both are dead in every build path. 557 LOC of dual
    structure under the live backend's own names → deleted here, with the ceiling.
  - **(5) The host test the plan relies on runs NOWHERE — MEASURED.** The plan said "declare
    `payload.rs` un-gated at the crate root like `ipc_stats`, so its host test actually runs".
    It compiles, but nothing runs it: `just test-host` is `cargo test --workspace --exclude neuron
    --exclude neuron-boot`, and no other recipe invokes `cargo test -p neuron`. Run by hand it
    passes **45 tests** (`ipc_stats`, `ipc_eof`, `va_space_tests`, `vmo_ro`, `waitset`, `sync`) —
    45 kernel tests that no gate has ever executed. A counting-allocator test added here would be
    fake-green by construction. So P3b adds `just test-kernel-host` to the `test-all` chain
    (ci-parity keeps it reachable), and only then land the counting allocator (0 allocations for a
    ≤ 32 B send, 1 above it) and `test_reject_oversized_inline`.
  - **(6) Measure, do not assume.** `Message` grows by the inline array and is moved on every
    send, push, pop and error return. Nothing prints kernel heap use except the OOM handler
    (`ALLOC.0.lock().used()/.free()`), so the `KSELFTEST: ipc stats` line gains `kheap_used=`, and
    the package is booted twice on the same profile — instrument first, tier second — so the
    tier's memory cost is a measured number and not an argument.
  - **P3b result (2026-09-17) — the tier, measured against its own prediction.** The package was
    built instrument-first: the `kheap_used=` field landed and booted BEFORE the tier, so the
    comparison is two runs of the same profile and not a memory of one.
    | | before | after (smp1) | after (visible) |
    |---|---|---|---|
    | `sends` | 5571 | 5523 | 13 507 |
    | `heap_allocs` | 5571 | **1527** | **1538** |
    | per message | 1.000 | 0.276 | **0.114** |
    | histogram > 32 B | 1527 | 1527 | 1538 |
    | `kheap_used` | 1 964 136 | 1 974 728 | — |
    The prediction in this ledger was "`heap_allocs` ≈ the > 32 B population, about 1530 per boot
    regardless of window length". It is not approximately that. Across **all 12 windows of the
    final `test-all` gate**, in both workload families, `heap_allocs` equals the histogram's
    > 32 B sum **exactly** — 1527, 1531, 1289, 1538, 2764, 2776, 2777, 2779, 14 010, 1535, 1527,
    1527 — from two counters that know nothing about each other. That is an identity, not a
    correlation, and it is the tier's definition made observable: a message allocates if and only
    if it does not fit inline. Allocations per message land between **0.114 and 0.465** depending
    on the workload (the OTA bundle lanes carry the ~1 KiB band P3a found), and no longer scale
    with the boot's length at all.
  - **P3b result — the price, also measured.** `size_of::<Payload>()` is `IPC_SHORT_MAX + 8` = 40,
    so `Message` grew 16 bytes, and it is moved on every send, push, pop and error return. On the
    running system that is **+10 592 bytes of steady kernel heap (+0.54 %)** — 2.65 bytes of
    standing memory per allocate/free pair removed. A host test pins `size_of::<Payload>()` so a
    later edit cannot quietly widen it.
  - **P3b result — `E2BIG` is boot-proven, not asserted.** `SELFTEST: ipc oversize rejected ok`
    sends `IPC_PAYLOAD_MAX + 1` REAL bytes (a zero-filled `.bss` static, not a short buffer with a
    long length, so the probe never depends on the kernel's validation ORDER) and accepts only
    `TooBig`. Success fails the probe too.
  - **P3b finding — 45 kernel host tests ran in NO gate.** The plan said "declare `payload.rs` at
    the crate root like `ipc_stats` so its host test actually runs". It compiles, but `just
    test-host` is `cargo test --workspace --exclude neuron` and nothing else invoked
    `cargo test -p neuron`; by hand it passed 45 tests nobody watched. A counting-allocator test
    added under that assumption would have been fake-green by construction. `just test-kernel` now
    runs them inside `test-all` and CI's kernel job; the count is **52**, including the counting
    `#[global_allocator]` that proves 0 allocations at `IPC_SHORT_MAX` and exactly 1 one byte
    above. Verified against a negative control: with the tier disabled the test FAILS.
  - **P3b finding, not fixed here.** `ipc/mod.rs`'s own `#[cfg(test)]` block — 222 LOC including a
    2000-step router state-machine fuzz — can never compile: `mod ipc` is `cfg(target_os = "none")`
    and the target build is not a test build. The Router cannot move to the crate root because
    `Message` carries a `crate::cap::Capability`. Follow-up: either type-check the target-only
    tests (`cargo check -p neuron --target riscv64… --tests`) in `lint-kernel`, or lift the
    Router's pure state machine the way `Payload` was lifted here. Named so it is not lost.
  - Zones: kernel, libs (`nexus-abi`, `nexus-ipc`), `justfile`/`scripts` (the test recipe).
    Blast: all IPC. Lanes: `just check`, smp1 ×2 (before / after), visible, `just test-all`.
- **P4 `ipc_call` / `ipc_reply_recv`: one trap per side, the reply in registers.**
  Reviewed against the tree 2026-09-17 before implementation. The IDEA is unchanged and is the
  end state: a request/reply exchange should cost ONE kernel entry per side, with no clock and no
  re-executed send. Two things the survey found change HOW, and both make the design smaller.
  - **(1) "One hook in the `handler.rs` epilogue" was wrong — there are FIVE resume points.**
    A task's saved frame is restored at `handler.rs:174` (syscall return), `:250` (preemption
    switch), `:716` (the post-runtime restore) and `fault.rs:434,454`. A copy-out hook would have
    to exist at every one of them, and a missed one is a silently truncated reply. So the design
    does not copy out at resume at all.
  - **(2) The reply fits in REGISTERS, and P3a already proved that covers the traffic.** Syscall
    arguments arrive in a0–a5 and the result leaves in a0, so **a1–a5 are free on return: 40
    bytes**, more than `IPC_SHORT_MAX = 32`. A reply of at most 32 bytes therefore completes the
    caller entirely in its saved frame — `a0 = length`, `a1..a4 = the payload` — which is
    address-space independent, needs no SATP, no hook, and no cross-address-space primitive (the
    non-goal stays a non-goal). P3a measured that **72.3 % to 93.1 %** of all messages are ≤ 32 B,
    so the register path IS the common path, and the inline tier and the fastpath become one
    decision instead of two.
  - **The commit point, and what happens above the tier.** Phase 1 (re-entrant): validate, resolve
    the reply cap, and enqueue the request; a full target queue blocks in `BlockReason::IpcSend`
    exactly as today, with nothing committed. Phase 2 (commit): the task records
    `CallState { reply_ep }`, registers as a recv-waiter on its reply endpoint and blocks in
    `BlockReason::IpcCall`. Phase 3: a send to that endpoint that finds the task in `IpcCall`
    writes the answer into its saved frame (`sepc + 4`, `a0`, `a1..a4`) and wakes it — the
    precedent is `core/trap/phased.rs`, which already completes a syscall this way. A reply LARGER
    than the register tier is left queued and the waiter is woken as an ordinary recv-waiter: its
    `CallState` says "the request is already sent", so the re-executed syscall skips phase 1 and
    does the receive in its own context. That reuses the existing `Error::Reschedule` machinery
    instead of inventing a second completion path, and it is why a committed send can never run
    twice. EOF (RFC-0079) completes the call with `EPIPE` through the same frame write.
  - **Split, by the same evidence rule the earlier packages used.** P4a: the completion state,
    `ipc_call`, EOF, and the negative tests — provable on its own with a selftest exchange and the
    kernel host tests that now RUN (P3b). P4b: `ipc_reply_recv`, the server half. P4c: the direct
    handoff (D3) and the budget marker, which is the only part that needs the scheduler and the
    only part whose numbers must be calibrated rather than asserted.
  - Watch, from the survey: `MAX_SYSCALL` is 64 and 57 is taken, so 58/59 are free as planned;
    `BlockReason` has four match sites that a new variant breaks (`task/mod.rs:1008`,
    `api/mod.rs:264,327`, `runtime.rs:507`); and a blocked task's frame is never re-saved while it
    is blocked (only an INTERRUPTED task's is), which is what makes the frame write safe.
  - **P4b review (2026-09-17) — the server half is composition, not new machinery.** A reply is
    already a plain `ipc_send_v1` on the moved capability's slot (`KernelServer::send_on_cap_wait`),
    and the next request is already `ipc_recv_v2`'s descriptor. So `ipc_reply_recv(reply_slot,
    hdr, frame, recv_desc)` is those two, with the SAME commit-point rule between them — and
    because its first half is the ordinary send path, P4a's completion hook fires from it
    unchanged: the client's `ipc_call` finishes in registers while the server is still inside its
    own single trap. Client one trap, server one trap, no third mechanism.
  - **The commit state collapses to one field, and that is a simplification of P4a.** A syscall
    that returns `Reschedule` re-executes the same instruction with the SAME registers, so the
    re-run can simply re-read its own arguments; `CallState` never needed to carry `out_ptr` and
    `out_max`. What it must carry is the one thing the arguments cannot say: "my outbound half is
    already out, do not send it again". One state, two users, told apart by the block reason — an
    `ipc_call` waiter blocks in `IpcCall` (and is completed in its frame), a `reply_recv` waiter
    blocks in `IpcRecv` (and finishes the receive itself on re-execution).
  - **P4c review (2026-09-17) — split, because the handoff has to beat a number that does not
    exist yet.** D3 is the only scheduler change in this task: today a wake is
    `scheduler.purge` + `enqueue_on_cpu` + an IPI when the waiter's home hart is elsewhere, and a
    handoff would instead switch to the waiter on this hart and enqueue the SENDER. That is a
    policy change, so it is the one place where "measure before claiming" is not a style rule but
    the difference between an optimisation and a regression.
    - **P4c-1 (numbers first).** The new traps already run; what is missing is their COST next to
      the old path. The existing `SELFTEST: ipc bench` times 64 two-trap exchanges
      (`exchange::call_into`) against samgrd's ping. A second bench times 64 `ipc_call`s against
      the SAME server with the SAME payload — samgrd answers with an ordinary send, which is
      exactly what completes a call in registers — so the two lines are a like-for-like
      comparison of one trap against two. Printed, not asserted: the budgets in
      `core/trap/budgets.rs` get their values from this, the way P1 said they would.
    - **P4c-1 result (2026-09-17).** Same server, same 12-byte request and reply, same 64
      rounds, smp1 + icount, over two boots: two traps **208 µs then 204 µs**, one trap **193 µs
      both times** — the fastpath is **5.4 % to 7.2 %** cheaper per exchange, and it is also the
      STEADIER of the two, which is its own small argument. `calls_in_regs=65` (64 bench rounds + the correctness probe) says
      every single one completed in the caller's registers; not one fell back to the queued tier.
      The modest size of the win is itself the finding: the trap is not what an exchange costs,
      the scheduling round trip is — which is exactly what P4c-2 targets, and now it has a
      baseline to beat instead of an assumption.
    - **P4c-2 (the handoff, if the numbers want it).** Only after P4c-1 says what a call costs
      today, and only with `handoff_hit` / `handoff_miss` moving from placeholders to real
      counts. `handoff_hit` has been printed as a hardcoded 0 since P1 — that is honest only
      while no handoff exists, and it is the first thing P4c-2 must fix.
  - Zones: kernel, `source/libs/nexus-abi`. Blast: all IPC. Lanes: `just check`, `just test-kernel`,
    smp1, visible, `just test-all`.
- **P5 Seam flip** — `exchange::call_into` → `ipc_call`; `KernelServer` → `reply_recv`
  (loop form `next = reply_recv(reply)`); the 8 hand-rolled `recv_v2` loops onto
  `KernelServer`; `ReplyCap::reply_and_close_wait` deleted; markers `SELFTEST: ipc fastpath
  ping ok (rt=<n>us)`, `SELFTEST: ipc fastpath reply ok`, `SELFTEST: ipc bulk-vmo path ok`.
  Zones: libs (`nexus-service-entry` if touched). Blast: everything. Lanes: `just test-all`
  + visible + 8/8 visible boots.
- **P6 Closure** — RFC-0096 Implemented, the RFC-0005 amendment, an RFC-0026 note,
  `docs/testing/README.md` bench lane, CHANGELOG (proof line, the `timeoutMs:` removal),
  IMPLEMENTATION-ORDER, ledger Done, memory handoff. Zones: `docs/rfcs`.

### Definition of Done

Host: alloc / op gates, RFC-0005 compatibility tests, the `test_reject_*` above, gate rule 4
with its fixture. QEMU (registered in `source/apps/selftest-client/proof-manifest/markers/
ipc_kernel.toml` + `scripts/qemu-test.sh` headless / smp1):
`KSELFTEST: ipc call budget ok (rt=<n>us handoff_miss=<m> alloc=0)`,
`SELFTEST: ipc fastpath ping ok (rt=<n>us)`, `SELFTEST: ipc fastpath reply ok`,
`SELFTEST: ipc bulk-vmo path ok`. Grep-gone list empty: `send_with_cap_move`, `recv_match`,
`Connection<`, `deadline_after`, `SVC_DEADLINE_NS`, `timeoutMs`, `MAX_FRAME_BYTES`,
`reply_and_close_wait`. `just test-all` EXIT=0, 8/8 visible boots.

### Touched paths

`source/kernel/neuron/src/{ipc/{payload.rs,stats.rs,mod.rs},syscall/{mod.rs,api/{mod.rs,
ipc_call.rs,ipc_msg.rs,ipc_recv_v2.rs,eof_scan.rs,sched_telemetry.rs}},task/{mod.rs,
completion.rs},core/trap/{handler.rs,budgets.rs}}`, `source/libs/nexus-abi/src/{lib.rs,
syscall/ipc.rs}`, `source/libs/nexus-log/src/lib.rs`, `userspace/nexus-ipc/src/{exchange.rs,
os_kernel.rs,budget.rs,reqrep.rs,connection.rs,os_lite.rs,policyd.rs,lib.rs}`,
`source/services/app-host/src/{svc_call.rs,effect_host.rs}`, every request/reply consumer
(inventory in P2), `source/apps/selftest-client/src/os_lite/{phases,probes}/ipc_kernel*`,
`source/apps/selftest-client/proof-manifest/markers/ipc_kernel.toml`, `scripts/qemu-test.sh`,
`scripts/check-wait-not-poll.sh`, `config/loc-baseline.txt`, `docs/rfcs/RFC-0096-*.md`,
`docs/rfcs/RFC-0005-*.md`, `docs/adr/0064-*.md`, `docs/testing/README.md`.

### Dependencies

None open: TASK-0324 P0–P9 are Done and RFC-0093 is Implemented. (The old line "P3 parked
replies until `@ready`" is deleted — routing v2 never parks on readiness.)

### Progress

| Package | Status |
|---|---|
| P0 Paper | Done 2026-09-15 — ledger rewrite, IMPLEMENTATION-ORDER rows, RFC-0096 seed + index rows (RFC-0093 index text corrected), ADR-0064 seed + index row, three-lens verdict above; zones released: `docs/rfcs` (P0), kernel for P1 |
| P1 Measure | Done 2026-09-15 — `just test-all` EXIT=0 (10 lanes); PROOF: smp1 214 ok / 41 KSELFTEST / total_ms 1257, `rt=209us n=64`, `sends=5700 heap_allocs=11400 copies=17665 copy_bytes=1690977 wake_ipis=0 handoff_miss=5172`; visible pixel 31.76, same numbers → **2 allocations and ~3 copies per message, 0.9 runqueue hops per message (one hart)** — `ipc/stats.rs` (declared at the crate root as `ipc_stats` so its unit test runs on host, like `ipc_eof`), `KSELFTEST: ipc stats (…)` next to the BKL line, `SELFTEST: ipc bench (rt=…us n=64)` in the `ipc_kernel` phase; counts sends, payload allocs, payload copies + bytes (zero-copy line), wake IPIs, handoff misses |
| P2-a Clocks out of request/reply | Done 2026-09-15 — `just test-all` EXIT=0 (10 lanes); PROOF: `just check` 0 (wait-not-poll absolute), smp1 213 ok / 41 KSELFTEST / total_ms 1257 (the retired deadline marker is the −1), visible pixel 31.76; no `apphost: svc reply desync` line in any lane |
| P2-b Pacing/watchdog waits onto timer pairs; rule 4 absolute | Done 2026-09-15 — `just test-all` EXIT=0 (10 lanes); PROOF: check 0 (rule 4 at zero), smp1 213 ok / 41 KSELFTEST / total_ms 1258 with `blk: watchdog on` + both probe markers, visible pixel 31.76; KERNEL: EOF latch consumed on observed emptiness only (approval used) |
| P2-c ONE send primitive; only awaited replies on a reply inbox | Done 2026-09-16 — `just test-all` EXIT=0 (10 lanes); PROOF: `just check` 0 (wait-not-poll rule 1 covers `send_with_cap_move`, structure/slot/init-sync at zero), smp1 EXIT=0 with the marker set IDENTICAL to the last P2-b green run (209 `ok` / 65 KSELFTEST, total_ms 1256, FAILs only the allow-listed dsoftbus pair), visible EXIT=0 pixel proof diff vs splash 31.76. MEASURED on the P1 instrument, same profile: `sends=5401 heap_allocs=10802 copies=17473` against P2-b's `5601 / 11202 / 18903` — **CORRECTED 2026-09-16 (P2-d):** this row first claimed 200 fewer messages, 400 fewer allocations and 1430 fewer copies per boot from `5401 / 10802 / 17473` against P2-b's `5601 / 11202 / 18903`. It does not hold. Four smp1 runs read `sends=5601, 13249, 5401, 13961` — bimodal and independent of the package, with byte-identical UART logs between the 5401 and 13961 runs. The stats line counts a WINDOW, so its totals scale with the idle background traffic the window spans. Stable across all four runs: **exactly 2.000 kernel heap allocations per message**, 3.12–3.38 copies per message with the traffic mix. P4 calibrates from the ratios, and the window needs a defined span before any total is asserted. `rt=242us n=64` |
| P2-d ONE correlation model (`reqrep` deleted, hand-rolled pairs gone) | Done 2026-09-16 — `just test-all` EXIT=0 (10 lanes); PROOF: `just check` 0 (rule 1 covers `ReplyBuffer`/`FrameStash`/`NonceGen`/`recv_match` with its fixture), smp1 EXIT=0 with the marker set IDENTICAL to P2-c (209 `ok` / 65 KSELFTEST, total_ms 1250, FAILs only the allow-listed dsoftbus pair) — init boots on its NEW ask inbox; visible EXIT=0 pixel proof 31.77. 44 `ipc_hdr::CAP_MOVE` occurrences in 28 files → 9, of which 8 are server-side flag reads (P5) and 1 is the documented metricsd exception. Stable measurement: exactly 2.000 kernel heap allocations per message, 3.12 copies per message |
| P2-e A server answers exactly the senders that moved a reply cap | Done 2026-09-16 — `just test-all` EXIT=0 (10 lanes, incl. all four OTA lanes on the rebuilt reply pump); PROOF: `just check` 0, smp1 EXIT=0 with the marker set IDENTICAL to P2-c/P2-d (209 `ok` / 65 KSELFTEST, total_ms 1255, FAILs only the allow-listed dsoftbus pair) and `metricsd: drop reply (no cap moved)` in the log — the boot's own witness that the cap-less path is real and no longer queues into the wedge; visible EXIT=0 pixel proof 31.76; `cargo test -p nexus-service-topology` 11 passed incl. the new wait-cycle guard |
| P2-f statefsd adopts the reply rule; no exception left | Done 2026-09-16 — `just test-all` EXIT=0 (10 lanes); PROOF: `just check` 0, smp1 EXIT=0 with the marker set IDENTICAL to P2-c/d/e (209 `ok` / 65 KSELFTEST, total_ms 1256, FAILs only the allow-listed dsoftbus pair). The boot is the witness for the whole chain in one log: `execd: minidump statefs route granted`, `child: minidump start`, `execd: minidump written` (the hand-assembled payload's PUT with a moved cap) and both `drop reply (no cap moved)` lines. Two cap-less clients the survey missed were found BY the boot: the harness' bootctl persist probe and init's supervision persist |
| P2-g The server hot path allocates nothing per request; the input chain survives a real drag | Done 2026-09-17 — opened on a user report. TWO hypotheses killed by measurement first (windowd's shared fallback reads `shared=0`; `recv_into` already truncates). CAUSE: **inputd dies of heap exhaustion** — `recv_request_with_meta` allocates per request on a heap that never frees; at ~400 HID batches/s that is `alloc_error` in under 20 s, and a dead inputd takes the whole input chain (`tx hz=0`). PROOF: same 45 s / 900-per-second QMP flood before and after — `heap-watermark` 50/75/90 % + `alloc-fail` → **no watermark, no alloc-fail, `tx` tracks `rx` (3507/3507)**. All 14 os-lite loops on the `_into` API, gate rule 3 against the allocating receive, `just input-flood` a lane in `test-all` (both its assertions fire on the pre-fix log), three more private frame caps folded, login seam markers registered |
| P3a Measure the payload distribution; delete the redundant copy | Done 2026-09-16 — `just test-all` EXIT=0 (10 lanes); PROOF: `just check` 0, smp1 EXIT=0 with the marker set diffed against the last P2-f boot: 264 → 265 distinct markers, the ONE addition being `KSELFTEST: ipc payload hist`, nothing lost, 213 `ok` markers in both, FAILs only the allow-listed dsoftbus pair; visible EXIT=0 pixel proof 31.76. **13 windows, two workload families:** `le64` is exactly 160 in all eight standard-boot windows (> 32 B population 1529 ± 5), ≤ 32 B is 72.3–93.1 % there and 53.5–63.6 % in the five OTA bundle lanes → **`IPC_SHORT_MAX = 32`**, RFC-0096 amended. The histogram total equals `heap_allocs` in all 13 windows = **one** kernel allocation per payload, from 2.000; `copies / sends` 3.12–3.38 → 2.05–2.64 |
| P3b The payload tier + ONE size bound with one owner (inline 32 B, `E2BIG`, dead backends deleted, kernel host tests gated) | Done 2026-09-17 — `just test-all` EXIT=0 (10 lanes); PROOF: `just check` 0 incl. the new `ipc-bounds` gate, smp1 + visible EXIT=0, pixel proof 31.77. **`heap_allocs / sends` 1.000 → 0.276 (smp1), 0.114 (visible)**, and `heap_allocs` = the histogram's > 32 B population EXACTLY in ALL 12 windows of the gate, both workload families — an identity, not a correlation; kernel allocations no longer scale with the boot. Price **+10 592 B steady kernel heap (+0.54 %)**, measured with the new `kheap_used=` field. `SELFTEST: ipc oversize rejected ok` proves `E2BIG`. 52 kernel host tests now run in a gate (they ran in NONE before), incl. the counting allocator |
| P4a `ipc_call` + completion state + EOF | Done 2026-09-17 — syscall 58. The reply rides home in REGISTERS (a1–a5 are free, 40 B > `IPC_SHORT_MAX`), so the "one epilogue hook" the plan assumed is not needed at all — the tree has FIVE resume points and a missed one would silently truncate a reply. Phase 1 IS `sys_ipc_send_v1` (no duplicated validation/CAP_MOVE/tier/budget); the commit point is a `CallState` and the peer finishes the syscall in the caller's saved frame. PROOF: `just check` 0, `just test-kernel` 56 passed (register packing host-tested at every edge length before any kernel wiring), smp1 EXIT=0 with `SELFTEST: ipc call ok` (nonce echo checked) and **`calls_in_regs=1`** — a counter that knows nothing about the probe. Splits the gate asked for: `task/block_reason.rs` (task/mod.rs 1292 → 1270), queue peek to `ipc/endpoint.rs` |
| P4b `ipc_reply_recv` (server half) | Done 2026-09-17 — syscall 59. Composition of the existing send + `recv_v2` with P4a's commit rule between them; P4a's completion hook fires from it unchanged, so client and server are each ONE trap without a third mechanism. The commit state collapsed to ONE field (a re-executed syscall re-reads its own arguments) — a simplification of P4a too. PROOF: `just check` 0, `just test-all` EXIT=0, smp1 EXIT=0 with `SELFTEST: ipc reply_recv ok` — the harness plays both roles against its own endpoint (two queued requests, answer one and receive the other in one trap) and then reads the answer off its reply inbox, so a reply that went nowhere fails instead of passing quietly. No service had to be rewritten to prove the trap |
| P4c-1 What a call costs, measured against the two-trap path | Done 2026-09-17 — same server, same 12-byte payload, same 64 rounds, smp1 + icount over two boots: two traps **208 µs / 204 µs**, one trap **193 µs both times** (**−5.4 % to −7.2 %**, and the steadier of the two), `calls_in_regs=65` so every round took the register path. The modest win IS the finding: the trap is not what an exchange costs, the scheduling round trip is — P4c-2 now has a baseline instead of an assumption. `just check` 0, smp1 EXIT=0 |
| P4c-2 Direct handoff (D3) + budget assert | Draft — the only scheduler change in this task; `handoff_hit` is still a hardcoded 0 and that is the first thing it must fix |
| P5 Seam flip | Draft |
| P6 Closure | Draft |

## End-state rewrite 2026-09-09 — historical, superseded by the 2026-09-15 rewrite above

**Ground truth 2026-09-09:** zero fastpath code (`source/kernel/neuron/src/ipc/` = `mod.rs`
718, `trace.rs` 279, `endpoint.rs` 151, `header.rs` 92 LOC), no IPC microbench anywhere, no
design pass recorded. The world it must be designed for landed since the draft: phased
syscalls (`core/trap/phased.rs`), the lock-free syscall class and cpu0 right-of-way (ADR-0049,
BKL wait 90.8 → ~6 ms), routing v2 with parked replies (TASK-0324 P3). Every service today
does request/reply as `send_with_cap_move_wait` + `recv` pairs, and app-host carries a stale-
reply drain (`source/services/app-host/src/svc_call.rs`) because the pair can desynchronize.

### Goal (end system)

A written IPC performance contract with deterministic gates, and the fastpath sized to it:
ONE `call` syscall for short control messages (≤ 64 B inline, zero heap) with direct reply
handoff — inside the phased/lock-free/right-of-way world, with RFC-0005 semantics untouched.

### Non-goals

New IPC primitive or ABI redesign; lock-free router experiments; MM changes; any change to
`sender_service_id`, CAP_MOVE rights/rollback, endpoint close/EOF (RFC-0079) semantics.

### Invariants

- RFC-0005 contract semantics preserved; identity stamped by the kernel on every path.
- `IPC_SHORT_MAX = 64` bytes inline; `IPC_PAYLOAD_MAX = 8192` hard cap → `E2BIG` above (no
  "temporary" oversized inline exception, ever).
- Hot path bounded: no unbounded queue scans, no logging, no heap growth for ≤ 64 B.
- Budgets are measured under `smp1` + icount (deterministic virtual time), printed with their
  numbers, and asserted — never a bare `ok`.

### Decisions

- **D1 Contract first.** RFC-0096 "IPC performance contract + `call` fastpath" (amends RFC-0005
  by reference) + ADR-0063. Budgets `IPC_CALL_RT_BUDGET_NS` (64 B ping-pong round trip),
  `IPC_CALL_WAKE_HOPS = 0` (the reply switches to the caller without a runqueue hop when the
  caller is on the serving hart or cpu0 right-of-way applies), `IPC_CALL_ALLOCS = 0` for ≤ 64 B —
  calibrated in P1 on the current path, then asserted.
- **D2 One trap for request+reply.** `SYSCALL_IPC_CALL_V1` (next free number, verified at P0):
  send the request with the moved reply cap and block on the reply endpoint in ONE trap. The
  reply is an ordinary `ipc_send_v1`; when the receiver is found in `BlockReason::IpcCall`, the
  payload is copied straight into the caller's registered buffer and the caller is made
  runnable / switched to (direct handoff). Lock class = BKL (router + scheduler); the ≤ 64 B
  copy needs no phased split; declared in the `LockClass` table with the safety argument
  (ADR-0049's "add a match arm" seam).
- **D3 Kernel-internal payload shape.** `Message.payload: Vec<u8>` becomes
  `Payload::{Inline([u8; 64], u8), Heap(Vec<u8>)}` — ABI unchanged, allocation-free short path.
- **D4 One request/reply API in userspace, old pairing deleted.**
  `nexus_ipc::KernelClient::call()` on the new syscall is the ONLY request/reply API. The
  `send_with_cap_move_wait` + `recv` pairing helpers and app-host's stale-reply drain
  (`svc_call.rs:38-68`) are DELETED in the same package — the desync class becomes impossible by
  construction. Gate: `tests/sdk_surface` asserts the pair helpers are gone; nexus-ipc
  `test_reject_oversized_inline` (E2BIG) + `test_reject_call_without_reply_cap`.
- **D5 Bench gate, TASK-0318 pattern.** Host op-count / alloc-count tests in
  `source/kernel/neuron/src/ipc/` (router ops per call, zero allocs for short messages) run in
  the `just test-all` kernel stage; QEMU prints the numbers in the kernel gate line.

### Packages (each names its blast radius + lanes to re-prove)

- **P0** RFC-0096 + ADR-0063 + `architecture-review` pass + explicit user approval (kernel +
  `source/libs` zones). Blast: paper.
- **P1** Measurement only: `SELFTEST: ipc bench (rt=<n>us hops=<h>)` on the current path (no
  assert) to calibrate D1. Blast: `ipc_kernel` phase; lanes headless + smp1.
- **P2** `Payload::Inline` + hard cap + E2BIG. Blast: every lane (all IPC) — full `test-all`.
- **P3** `SYSCALL_IPC_CALL_V1` + direct handoff + `KSELFTEST: ipc call budget ok (rt=<n>us
  hops=0 alloc=0)`. Blast: all lanes incl. smp/bkl budgets; TASK-0324 P3 parked-reply rules
  must hold (a `call` before the callee's `@ready` parks like any request).
- **P4** `KernelClient::call()` + migration of every consumer + deletion of the pair helpers
  and the app-host drain. Blast: all services, app-host; lanes visible (DSL shell), ota-*,
  ingress/egress, reset.
- **P5** Docs (`docs/testing/README.md` bench lane, RFC-0005 fastpath note, budgets with
  provenance).

### Definition of Done

Host: op-count/alloc gates; RFC-0005 compatibility tests green; `test_reject_*` above.
QEMU (registered in `proof-manifest/markers/ipc_kernel.toml`, `scripts/qemu-test.sh`
headless + smp1 lists, `tools/nx/chains/markers.txt`):
`KSELFTEST: ipc call budget ok (rt=<n>us hops=0 alloc=0)`,
`SELFTEST: ipc fastpath ping ok (rt=<n>us)`, `SELFTEST: ipc fastpath reply ok`,
`SELFTEST: ipc bulk-vmo path ok` (inline > 8 KiB → E2BIG, VMO path succeeds).
Docs: `docs/testing/README.md`, RFC-0096, ADR-0063.

### Touched paths

`source/kernel/neuron/src/{ipc/mod.rs,syscall/mod.rs,syscall/api/ipc_msg.rs,syscall/api/
ipc_call.rs (new),core/trap/{phased.rs,budgets.rs},task/mod.rs}`,
`source/libs/nexus-abi/src/syscall/ipc.rs`, `userspace/nexus-ipc/src/{os_lite.rs,connection.rs}`,
`source/services/app-host/src/svc_call.rs`, every request/reply consumer,
`source/apps/selftest-client/src/os_lite/{phases,probes}/ipc_kernel*`, `docs/rfcs/RFC-0096-*.md`,
`docs/adr/0063-*.md`.

### Dependencies (active work only)

TASK-0324 P3 (routing v2: parked replies until `@ready`) and P5 (stage fence / wake
determinism findings) land first — the `call` syscall must honour both.

## Rebase (2026-08-14) — historical, superseded by the end-state rewrite above

### Verified baseline

- **Zero fastpath code exists.** `source/kernel/neuron/src/ipc/` is 4 files
  (`mod.rs` 718, `trace.rs` 279, `endpoint.rs` 151, `header.rs` 92 LOC) with no
  short-message fastpath, and there is no IPC microbench harness anywhere in
  the repo. The goal below **stands**: short control-message fastpath +
  reply/wake tightening + VMO-first bulk rule + deterministic microbenches.

### Stale baseline — design against the phased/lockfree world

The ledger's queueing/wake framing predates TASK-0288's addendum, which landed:

- **phased syscalls** (`source/kernel/neuron/src/core/trap/phased.rs`),
- a **lock-free syscall class** and **cpu0 right-of-way**,
- BKL wait 90.8 ms → ~6 ms — ADR-0049 Accepted and implemented
  (`docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md`).

Do NOT re-derive wake/queue costs from the pre-0288 BKL picture. Before any
code, the fastpath design must answer explicitly: which trap phase the
fastpath executes in, whether its syscalls can qualify for the lock-free
class, and how reply/wake interacts with cpu0 right-of-way.

### Process gate

`source/kernel/**` and `source/libs/**` are approval zones: run the
`architecture-review` skill on the design and obtain explicit user approval
before editing kernel files. Status stays Draft until that design pass exists.

## Context — historical, superseded by the end-state rewrite above

The current IPC architecture is directionally right for the system:

- endpoint capabilities,
- small typed control messages,
- large payloads out-of-band via VMO/filebuffer,
- channel-bound identity via `sender_service_id`.

That architecture should remain. However, once `windowd`, launcher, settings, overlays, and animation-heavy UI
start emitting lots of small control messages, the system needs a **fast path** for those short request/reply flows.

This task focuses on the **existing IPC ABI**, not a redesign. The goal is to make the hot path smaller and more
predictable while pushing bulk bytes harder onto the VMO/data plane.

## Goal — historical, superseded by the end-state rewrite above

Deliver a kernel IPC fastpath suitable for UI/control-plane traffic:

1. **Short-message fastpath**:
   - optimize the common case of small control messages,
   - keep queueing/wake bookkeeping bounded and cheap.
2. **Reply/wake hot-path tightening**:
   - reduce avoidable wake/unblock overhead in request/reply flows,
   - keep timeout/deadline semantics intact.
3. **VMO-first bulk discipline**:
   - document and enforce the rule that large payloads should move via VMO/filebuffer,
   - avoid “temporary” oversized inline message paths for surfaces/media/documents.
4. **Evidence and benchmarks**:
   - provide deterministic host microbenches and bounded QEMU selftests for ping/reply latency.

## Non-Goals

- New distributed IPC design.
- Replacing endpoint capabilities with a different primitive.
- General-purpose lock-free router experiments.
- MM mapping optimization beyond what is necessary for IPC data-plane handoff (`TASK-0054D` owns MM perf).

## Constraints / invariants (hard requirements)

- Preserve RFC-0005 contract semantics and identity binding rules.
- Do not break `sender_service_id`, endpoint close-on-exit, waiter wake, or CAP_MOVE correctness.
- Large bulk payloads remain VMO/filebuffer-first.
- Hot-path work must be bounded:
  - no unbounded queue scans,
  - no unbounded logging,
  - no opportunistic heap growth in the fast path.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security considerations

IPC is a trust boundary and must remain fail-closed while being optimized.

### Threat model

- **Identity drift**: optimization accidentally bypasses `sender_service_id` authority.
- **Capability mishandling**: fastpath changes breaking CAP_MOVE or close semantics.
- **Queue exhaustion / DoS**: optimization reintroducing unbounded buffering or unfair wake behavior.
- **Oversized inline payload drift**: services bypassing VMO/filebuffer discipline and bloating kernel copies.

### Security invariants (MUST hold)

- `sender_service_id` remains authoritative for security-sensitive consumers.
- CAP_MOVE remains rights-bounded and rollback-safe.
- Endpoint close / owner-exit wake semantics remain deterministic.
- Large payloads do not silently expand the control-plane attack surface.

### DON'T DO

- DON'T add a “fast path” that skips identity binding or endpoint rights checks.
- DON'T create a parallel IPC ABI just for UI traffic.
- DON'T add a giant inline-payload exception for convenience.
- DON'T optimize by weakening existing timeout or waiter semantics.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Microbench / contract tests prove improvements for:
  - ping-pong request/reply,
  - wake/unblock common case,
  - large-payload path selecting VMO/filebuffer instead of inline copy.
- Compatibility tests remain green for existing RFC-0005 ABI/layout guarantees.

### Proof (OS/QEMU) — gated

Deterministic markers / evidence:

- `SELFTEST: ipc fastpath ping ok`
- `SELFTEST: ipc fastpath reply ok`
- `SELFTEST: ipc bulk-vmo path ok`

Optional additive perf evidence:

- bounded latency counters exported by the selftest harness or `perfd`

## Touched paths (allowlist)

- `source/kernel/neuron/src/ipc/`
- `source/kernel/neuron/src/syscall/`
- `source/kernel/neuron/src/task/`
- `source/libs/nexus-abi/`
- `userspace/nexus-ipc/`
- `source/apps/selftest-client/`
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (only if contract wording needs explicit fastpath notes)
- `docs/testing/README.md`

## Plan (small PRs)

1. Identify and tighten the small-message request/reply hot path.
2. Preserve RFC-0005 behavior while reducing avoidable wake/queue overhead.
3. Make VMO/filebuffer-first bulk discipline explicit in UI/media-facing paths.
4. Add microbench + QEMU evidence without inventing new fake-success markers.
