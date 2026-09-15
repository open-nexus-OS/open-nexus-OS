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
- **P2 Userspace ONE API** — all 48 pairing functions + 20 helper callers onto `exchange`;
  the D5 deletion list; app-host `svc_call.rs` = a wrapper around `call_into`; `timeoutMs:`
  removed (effect_host + DSL grammar / lint in `tools/nx`); gate rule 4; LOC baseline
  shrunk (`config/loc-baseline.txt`). Zones: libs (`nexus-log`, one site), config, scripts.
  Blast: every service, init, selftest. Lanes: `just test-all` + visible.
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
| P2 Userspace ONE API | Draft |
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
