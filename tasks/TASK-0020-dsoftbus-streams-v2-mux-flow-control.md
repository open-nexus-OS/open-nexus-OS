---
title: TASK-0020 DSoftBus Streams v2: multiplexing + flow control + keepalive (host-first, OS-gated)
status: In Progress
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

### API/Rust hygiene gate — required

- Newtype wrappers are used where stream/credit/priority class confusion would otherwise be possible.
- Session/mux mutable state ownership is explicit (no implicit shared mutable global state).
- Critical transition/accounting outcomes carry `#[must_use]` where ignored results could violate invariants.
- No new daemon-path `unsafe` usage introduced for `Send`/`Sync` workarounds.

### Proof (OS / QEMU) — gated on OS backend

Once OS DSoftBus streams exist:

- extend `scripts/qemu-test.sh` with:
  - `dsoftbus:mux session up`
  - `dsoftbus:mux data ok`
  - `SELFTEST: mux pri control ok`
  - `SELFTEST: mux bulk ok`
  - `SELFTEST: mux backpressure ok`
- when 2-VM mux proof is available, validate via `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- keep QEMU proofs sequential (single-VM then 2-VM)

Notes:

- Postflight scripts (if added) must **only** delegate to canonical harness/tests; no `uart.log` greps as “truth”.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (mux module + integration points)
- `source/services/dsoftbusd/` (use mux after handshake; OS-gated)
- `source/apps/selftest-client/` (OS-gated mux markers)
- `tests/` (host mux tests)
- `docs/distributed/`
- `scripts/qemu-test.sh` (only when OS backend is ready)

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
