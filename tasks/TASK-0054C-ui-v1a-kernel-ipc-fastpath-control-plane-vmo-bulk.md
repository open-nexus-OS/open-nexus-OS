---
title: TASK-0054C Kernel IPC performance contract + `call` / `reply_recv` fastpath (one trap per side, direct handoff, inline ≤ 64 B, VMO-first bulk)
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
  the moved reply cap; identity is kernel-stamped on both legs; ≤ 64 B ⇒ 0 allocations,
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
  the encoding larger than the fields (≥ 24 B floor before any payload — ~40 % of a 64-byte
  tier). RFC-0005 §"Relationship to our existing IDL + filebuffer/VMO hybrid" ("Control
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
trap, the request handed off to the server; payload ≤ 64 B inline without kernel heap; an
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
- `IPC_SHORT_MAX = 64` (inline, zero allocations) and `IPC_PAYLOAD_MAX = 8192` (`E2BIG`
  above) are public constants in `nexus-abi`; the kernel-private `MAX_FRAME_BYTES` is
  deleted.
- `ipc_call` / `ipc_reply_recv` have NO deadline argument (RFC-0093 §7). Exactly two things
  end a `call`: the reply, or the death of the last peer (EOF → `EPIPE`).
- A committed syscall is never re-executed: the commit point is "request / reply enqueued";
  from there the peer (or the EOF scan) completes the syscall through the task's call state.
- Hot path bounded: no queue scan, no logging, no heap for ≤ 64 B.
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
  completion by the peer: an inline reply into the waiter's 64-byte stage buffer, a heap
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
  (`copies`, `copy_bytes`, `heap_allocs`) are the baseline: today a non-empty message costs
  two allocations and three payload copies (user → heap, heap → heap clone, heap → user).
- **D6 Bench gate.** Host: a counting `#[global_allocator]` in the kernel host tests
  (0 allocations for ≤ 64 B send / recv / call) and router op counters in the state machine,
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
  - **P2-f (seed) statefsd adopts the reply rule.** Blocked on three cap-less clients that read
    statefsd's shared response endpoint: the selftest statefs ladder (~50 call sites in 10
    files, ~15 required markers), dsoftbusd's remote-statefs proxy leg, and the
    `demo.minidump` child — hand-assembled RISC-V in `userspace/apps/demo-exit0/build.rs`
    whose `MsgHeader` is zeroed, so moving a cap means writing the `cap_clone` + CAP_MOVE by
    hand or replacing the payload. Closing it also closes P2-c's metricsd exception and lets
    the harness' logd and statefsd legs drop from `SharedResponse` to `ReplyInbox`.
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
- **P3 Kernel payload** — `ipc/payload.rs`, `IPC_SHORT_MAX` / `IPC_PAYLOAD_MAX` in
  `nexus-abi`, `E2BIG`, the counting-allocator test, `test_reject_oversized_inline`. Zones:
  kernel, libs. Blast: all IPC. Lanes: `just test-all`.
- **P4 Kernel call + reply_recv** — `syscall/api/ipc_call.rs`, `task/completion.rs`,
  `BlockReason::IpcCall`, handoff (D3), EOF integration, `nexus-abi` wrappers (`ipc_call`,
  `ipc_reply_recv`), budgets + `KSELFTEST: ipc call budget ok (...)`, kernel host tests
  (`test_reject_call_without_reply_cap`, `test_reject_call_reply_cap_foreign_endpoint`,
  `test_reject_reply_recv_without_recv_right`, `test_call_completes_on_peer_death`,
  handoff same-hart, enqueue cross-hart, `test_call_commit_is_never_reexecuted`). Zones:
  kernel, libs. Lanes: `just test-all` incl. smp / bkl.
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
| P2-f statefsd adopts the reply rule (3 cap-less clients first) | Draft |
| P3 Kernel payload | Draft |
| P4 Kernel call + reply_recv | Draft |
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
