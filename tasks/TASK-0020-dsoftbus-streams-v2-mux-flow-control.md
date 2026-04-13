---
title: TASK-0020 DSoftBus Streams v2: multiplexing + flow control + keepalive (host-first, OS-gated)
status: Done
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - RFC (streams v2 contract seed): docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md
  - RFC (modular daemon boundary): docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - Depends-on (modularization base): tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md
  - Depends-on (OS streams): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Related (completed baseline): tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md
  - Related (completed baseline): tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md
  - Related (completed baseline): tasks/TASK-0017-dsoftbus-remote-statefs-rw.md
  - Related (core/backend extraction boundary): tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md
  - Follow-on (transport evolution): tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
  - Testing methodology: docs/testing/index.md
  - Testing contract: scripts/qemu-test.sh
  - Testing contract (2-VM): tools/os2vm.sh
---

## Context

Current DSoftBus framing supports a single reliable stream with a `channel: u32` field, but it does
not provide:

- per-stream flow control,
- robust backpressure,
- priorities/fair scheduling,
- keepalive/heartbeat,
- clean open/accept lifecycle for multiple independent logical streams.

Remote services (remote-fs, samgr/bundlemgr proxies) will benefit from a yamux-like MUX layer.

## Goal

Provide a robust stream-multiplexing layer over the existing authenticated transport so that:

- multiple logical streams can be opened/accepted independently,
- streams have priorities and fair scheduling,
- window-based flow control provides backpressure (no unbounded buffering),
- keepalive detects dead peers deterministically.

## Target-state alignment (post TASK-0015 / RFC-0027)

- Mux integration into OS path must follow modular seams (session/netstack/gateway), not reopen monolithic
  `dsoftbusd` control flow.
- Mux state machine and frame accounting should be backend/core-owned and reusable for TASK-0022 extraction.
- Observability and marker emission should stay deterministic and routed through existing helper boundaries.

## RFC-0034 extracted requirements (legacy 0001..0020 closure)

Execution sequencing rule for this phase:

- sequencing lock (enforced during execution): do not start implementation/proof execution for `TASK-0021+` while `TASK-0020` is `In Progress`,
- if `RFC-0034` introduces mandatory closure gates needed for legacy `TASK-0001..0020` production-readiness,
  extract those gates into this task slice and prove them here first.

Extracted closure set currently owned by `TASK-0020`:

- real single-VM + 2-VM mux marker ladders,
- deterministic distributed performance budgets (`tools/os2vm.sh` phase `perf`),
- bounded distributed hardening soak (`tools/os2vm.sh` phase `soak`),
- machine-readable release evidence bundle (`release-evidence.json`),
- no-fake-success guards for mux/remote fail markers.

## Current state snapshot (2026-03-27)

- Structural and protocol-adjacent prerequisites are complete and stable:
  - `TASK-0015` (`dsoftbusd` modular daemon seams) is `Done`.
  - `TASK-0016` (remote packagefs RO over authenticated streams) is `Done`.
  - `TASK-0016B` (`netstackd` modularization + deterministic networking proofs) is `Done`.
  - `TASK-0017` (remote statefs RW with ACL/audit) is `Done`.
- This task should improve multiplexing/flow-control/keepalive within those seams without changing
  completed task contracts or re-opening their scope.
- `userspace/dsoftbus/src/os.rs` remains placeholder; therefore host-first proofs are mandatory and OS
  proofs remain explicitly gated.

## Non-Goals

- Full yamux protocol compatibility.
- QUIC or datagram transport.
- Kernel changes.

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **Bounded memory**:
  - cap number of concurrent streams,
  - cap per-stream receive buffer,
  - cap max frame payload size (chunking required).
- **Determinism**:
  - keepalive intervals and timeouts are bounded and deterministic,
  - tests do not rely on “wall clock jitter” for correctness.
- **No fake success**: markers only emitted after real multiplexed data transfer.
- **No async runtime requirement**: prefer a pump/poll API that works in host tests and OS bring-up
  without pulling in an async executor.
- **OS proof environment stability**:
  - OS/QEMU proofs must use canonical harness defaults that enforce modern virtio-mmio behavior,
  - no reliance on legacy-only virtio behavior or non-canonical ad-hoc QEMU wiring.

## Rust/API hygiene (hard requirements)

- **Typed protocol domain**:
  - stream identity, window/credit counters, and priority classes should be represented by `newtype`
    wrappers at public/internal seam boundaries where this prevents class-mix bugs.
- **Ownership-first mutable state**:
  - mux session mutable state (stream tables, scheduler queues, credit accounting) must have explicit
    ownership (single-writer/event-loop model by default), with bounded handoff points.
- **`Send`/`Sync` discipline**:
  - do not add blanket or `unsafe` `Send`/`Sync` impls for mux/session state,
  - only rely on auto-traits from composition unless a narrowly justified contract requires otherwise.
- **`#[must_use]` on critical outcomes**:
  - mark state-transition and flow-control outcomes that can silently desync invariants if ignored
    (e.g., scheduler step results, credit-application results, keepalive verdicts).

## Security considerations

### Threat model

- Malformed/hostile frames attempt stream-state corruption or cross-stream confusion.
- Window/credit abuse causes memory growth or starvation (availability attack).
- Keepalive abuse induces false liveness or false teardown.

### Security invariants (MUST hold)

- Mux operation is accepted only on authenticated session context (no unauthenticated bypass path).
- Stream IDs and stream lifecycle transitions are validated fail-closed.
- Frame payload, window deltas, and buffered bytes are strictly bounded.
- Backpressure semantics are explicit (`WouldBlock`/credit exhaustion), never hidden by unbounded queues.
- Priority policy guarantees bounded starvation (high-priority traffic must not permanently starve lower levels).

### DON'T DO

- DON'T allocate per-frame/per-stream buffers without hard caps.
- DON'T accept WINDOW_UPDATE overflows/underflows.
- DON'T emit mux success markers before confirmed multiplexed roundtrip.

### Required negative tests

- `test_reject_mux_frame_oversize`
- `test_reject_invalid_stream_state_transition`
- `test_reject_window_credit_overflow_or_underflow`
- `test_reject_unknown_stream_frame`

### Deterministic reject labels (lock in Phase 0)

- `mux.reject.frame_oversize`
- `mux.reject.invalid_stream_state_transition`
- `mux.reject.window_credit_overflow_or_underflow`
- `mux.reject.unknown_stream_frame`
- `mux.reject.unauthenticated_session`

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture`
  - `cargo test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture`
  - `cargo test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture`
- Required tests:
  - `test_reject_mux_frame_oversize`
  - `test_reject_invalid_stream_state_transition`
  - `test_reject_window_credit_overflow_or_underflow`
  - `test_reject_unknown_stream_frame`
  - `test_reject_unauthenticated_session`

### Hardening markers (QEMU, if applicable)

- `dsoftbus:mux session up`
- `dsoftbus:mux data ok`
- `SELFTEST: mux pri control ok`
- `SELFTEST: mux bulk ok`
- `SELFTEST: mux backpressure ok`

## Red flags / decision points

- **RED (blocking)**:
  - `userspace/dsoftbus` OS backend is currently a placeholder (`userspace/dsoftbus/src/os.rs`).
    Note: OS bring-up streams exist via os-lite services (`netstackd` + `dsoftbusd`) as of TASK-0005,
    but the reusable library backend remains stubbed until TASK-0022 refactors DSoftBus core for OS/no_std.
    This task must therefore be **host-first** with OS-gated follow-up steps.
- **YELLOW (risky)**:
  - Scheduling fairness can become flaky if expressed in terms of “milliseconds”. Prefer deterministic
    byte/credit accounting and ordering constraints.
  - Priority mapping must be conservative: “high priority” must not starve lower priority streams
    indefinitely (bounded starvation).

## Contract sources (single source of truth)

- DSoftBus high-level traits: `userspace/dsoftbus` (`Stream::send/recv`, channel+bytes model)
- QEMU marker contract: `scripts/qemu-test.sh` (only once OS backend exists)

## Stop conditions (Definition of Done)

### Phase 0 (contract + determinism lock) — required

- Lock deterministic limits and reject labels before feature expansion:
  - explicit maxima for streams, payload chunk size, and credit/window bounds,
  - stable fail-closed reject labels for oversize/invalid-state/unknown-stream/credit-overflow paths.
- Lock deterministic priority/fairness default before feature expansion:
  - priority classes are `0..=7` (`0` highest),
  - enforce strict-priority with bounded starvation by requiring one lower-priority dequeue after
    `HIGH_PRIORITY_BURST_LIMIT` consecutive high-priority dequeues while lower-priority work is pending.
- Confirm dependency/sequencing remains drift-free:
  - this task does not claim to unblock already completed tasks,
  - this task does not absorb `TASK-0022` scope.

### Proof (Host) — required

Add deterministic host tests (new crate `tests/dsoftbus_mux_host` or under `userspace/dsoftbus` tests):

- interleaving with priorities (high-pri favored, but low-pri progresses),
- slow consumer enforces backpressure (writer blocks/returns WouldBlock; no OOM),
- keepalive timeout (missing PONG tears down),
- RST propagation,
- fuzz-ish deterministic state-machine test (seeded) for frame ordering/credit invariants.
- security rejects from `test_reject_*` suite are green.
- deterministic ownership/concurrency test surface exists for scheduler + credit application invariants
  (no hidden shared mutable state behavior).

#### Phase 0 evidence snapshot (2026-04-10)

- Added mux contract lock module:
  - `userspace/dsoftbus/src/mux_v2.rs`
  - `userspace/dsoftbus/tests/mux_contract_rejects_and_bounds.rs`
- Host proof commands executed:
  - `cargo test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture` (11 passed, 0 failed)
  - `cargo test -p dsoftbus -- --nocapture` (full package green, includes mux phase-0 tests)
- Required negative tests now present and green:
  - `test_reject_mux_frame_oversize`
  - `test_reject_invalid_stream_state_transition`
  - `test_reject_window_credit_overflow_or_underflow`
  - `test_reject_unknown_stream_frame`
- Deterministic bounded behavior additionally proven in host tests:
  - bounded backpressure via explicit `WouldBlock`,
  - bounded keepalive timeout via tick-based deterministic policy,
  - bounded starvation floor via deterministic scheduler burst budget.
- OS/QEMU mux-marker closure now has explicit proof wiring in `dsoftbusd` selftest path (no marker-only closure).

#### Phase 1 evidence snapshot (2026-04-10, test-first)

- Added host engine behavior surface (frame/state handling) in:
  - `userspace/dsoftbus/src/mux_v2.rs`
  - `userspace/dsoftbus/tests/mux_frame_state_keepalive_contract.rs`
- Test-first Soll-verhalten proofs (before/with implementation):
  - deterministic lifecycle (`OPEN`/`OPEN_ACK`/`CLOSE`/`RST`),
  - oversize DATA fail-closed reject,
  - invalid `OPEN_ACK` transition reject,
  - keepalive PONG liveness reset semantics,
  - bounded `WINDOW_UPDATE` credit evolution,
  - seeded deterministic sequence for credit invariants,
  - idempotent `RST` behavior with fail-closed close-after-reset reject.
- Commands:
  - `cargo test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture`
  - `cargo test -p dsoftbus -- --nocapture`
- Mandatory phase regression set (per plan):
  - `just test-e2e`
  - `just test-os-dhcp`

#### Phase 2 evidence snapshot (2026-04-10, test-first)

- Added host integration surface for stream registry + wire-event pump in:
  - `userspace/dsoftbus/src/mux_v2.rs` (`MuxHostEndpoint`, named stream open/accept, ingest/drain)
  - `userspace/dsoftbus/tests/mux_open_accept_data_rst_integration.rs`
- Test-first Soll-verhalten proofs:
  - `open_stream`/`accept_stream` named-stream behavior,
  - control+bulk multiplexed traffic on one session (bounded buffered accounting),
  - close/reset propagation with fail-closed send rejection on reset stream,
  - duplicate stream-name reject on ingest/open path,
  - unauthenticated endpoint fail-closed rejects,
  - invalid teardown transition reject (`close` after `reset`).
- Commands:
  - `cargo test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture`
  - `cargo test -p dsoftbus -- --nocapture`
- Mandatory phase regression set (per plan):
  - `just test-e2e`
  - `just test-os-dhcp`

#### Phase 3/4 evidence snapshot (2026-04-11)

- Canonical single-VM smoke (phase-3 gate execution):
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh` -> green harness completion with mux markers:
    - `dsoftbus:mux session up`
    - `dsoftbus:mux data ok`
    - `SELFTEST: mux pri control ok`
    - `SELFTEST: mux bulk ok`
    - `SELFTEST: mux backpressure ok`
- Canonical 2-VM distributed harness (phase-4 gate execution):
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` -> `result=success` with summary artifacts:
    - `artifacts/os2vm/runs/os2vm_1775990226/summary.json`
    - `artifacts/os2vm/runs/os2vm_1775990226/summary.txt`
    - `artifacts/os2vm/runs/os2vm_1775990226/release-evidence.json`
  - 2-VM mux marker ladder is proven on both nodes (`tools/os2vm.sh` phase `mux`):
    - `dsoftbus:mux crossvm session up`
    - `dsoftbus:mux crossvm data ok`
    - `SELFTEST: mux crossvm pri control ok`
    - `SELFTEST: mux crossvm bulk ok`
    - `SELFTEST: mux crossvm backpressure ok`
  - deterministic runtime budget gate is proven (`tools/os2vm.sh` phase `perf`), with observed timings in summary:
    - discovery `4038ms`, session `12ms`, mux `63ms`, remote `28251ms`, total `35138ms`
  - bounded hardening soak gate is proven (`tools/os2vm.sh` phase `soak`):
    - soak duration `15s`, rounds `2`
    - fail/panic marker hits on both nodes: `0`
- Contract honesty (no fake closure): mux markers are emitted only after real mux-v2 state-machine checks pass (single-VM and 2-VM paths).

### API/Rust hygiene gate — required

- Newtype wrappers are used where stream/credit/priority class confusion would otherwise be possible.
- Session/mux mutable state ownership is explicit (no implicit shared mutable global state).
- Critical transition/accounting outcomes carry `#[must_use]` where ignored results could violate invariants.
- No new daemon-path `unsafe` usage introduced for `Send`/`Sync` workarounds.

### Proof (OS / QEMU)

- Single-VM mux marker closure:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh`
- Distributed baseline:
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - includes explicit cross-VM mux phase marker ladder (`phase: mux`)
- keep QEMU proofs sequential (single-VM then 2-VM)

Notes:

- Postflight scripts (if added) must **only** delegate to canonical harness/tests; no `uart.log` greps as “truth”.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (mux module + integration points)
- `source/services/dsoftbusd/` (use mux contract checks after authenticated session)
- `source/apps/selftest-client/` (mux marker consumption path if needed by selftest sequencing)
- `tests/` (host mux tests)
- `docs/distributed/`
- `scripts/qemu-test.sh` (mux marker contract now wired via `REQUIRE_DSOFTBUS=1`)
- `tools/os2vm.sh` (2-VM mux marker phase gate)

## Plan (small PRs)

1. **Phase 0: contract + determinism lock (host-first)**
   - Finalize bounded constants (max streams/payload/window/credit) and deterministic reject labels.
   - Introduce typed wrappers (`newtype`) and `#[must_use]` outcomes at mux seam boundaries.
   - Lock ownership model for mutable mux session state (single-writer/event-loop by default).

2. **Mux protocol + engine (host-first)**
   - Implement a versioned frame format with:
     - OPEN / OPEN_ACK
     - DATA
     - WINDOW_UPDATE
     - RST
     - PING / PONG
     - CLOSE
   - Flow control: per-stream credit (default 64KiB), sender decrements on DATA, receiver issues WINDOW_UPDATE on consume.
   - Priorities: 0..7 with a simple scheduler (strict priority + round-robin within each level), plus a starvation bound.

3. **Integration into host backend**
   - After Noise handshake, wrap the transport in a `MuxSession`.
   - Provide `open_stream(name, pri)` and `accept_stream()` registry on top of mux streams.

4. **Migrate client/server protocols**
   - Move remote-fs and proxy traffic onto named mux streams with chunking (<= 32KiB per DATA).
   - **RPC Format Note**: TASK-0005/0016/0017 use OS-lite byte frames as bring-up shortcuts. This task is a good migration point to introduce schema-based RPC (Cap'n Proto) on top of mux streams. See TASK-0005 "Technical Debt" section.

5. **OS integration (gated)**
   - Once OS DSoftBus is real, adopt the same mux session in OS build and add QEMU markers/selftest.

## Docs

- Add `docs/distributed/dsoftbus-mux.md` describing:
  - frame format
  - flow control rules
  - priority policy and starvation bound
  - keepalive behavior
  - limits (max streams, max frame size)

## Alignment note (2026-02, low-drift)

- Current OS-lite cross-VM path stabilizes connection lifecycle with an explicit session FSM + epoch ownership
  in `dsoftbusd` (no kernel/protocol contract change).
- Session setup now uses bounded, transport-level readiness checks before stream writes (`WouldBlock` remains the
  backpressure signal).
- Discovery receive polling in setup is rate-limited and non-fatal once peer mapping is known, so session progress
  is not starved by discovery RPC timing.
- Mux v2 should treat these as transport invariants and keep stream-level scheduling/credits orthogonal (no
  incompatible buffering semantics).
