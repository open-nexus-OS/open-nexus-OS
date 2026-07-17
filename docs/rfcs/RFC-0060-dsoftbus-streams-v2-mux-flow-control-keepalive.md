# RFC-0033: DSoftBus Streams v2 mux/flow-control/keepalive (host-first, OS-gated)

- Status: Done
- Owners: @runtime
- Created: 2026-03-27
- Last Updated: 2026-04-11
- Links:
  - Tasks: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (execution + proof; SSOT)
  - ADRs: `docs/adr/0005-dsoftbus-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
    - `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md`
    - `docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md`

## Status at a Glance

- **Phase 0 (contract + determinism lock)**: ✅
- **Phase 1 (host mux engine + security proofs)**: ✅
- **Phase 2 (OS-gated marker closure)**: ✅

Definition:

- "Done" means the contract is defined and the proof gates are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - stream-multiplexing contract over authenticated DSoftBus sessions,
  - bounded flow-control and keepalive failure semantics,
  - deterministic reject behavior and marker contract for mux proof closure.
- **This RFC does NOT own**:
  - QUIC transport evolution (`TASK-0021`),
  - no_std core/backend extraction (`TASK-0022`),
  - cross-task production hardening program (`RFC-0034`),
  - kernel networking or kernel transport changes.

## Production-closure bridge (RFC-0034 boundary)

This RFC remains intentionally narrow so it can reach `Complete` once `TASK-0020` closure proofs are green.

- `RFC-0033` is authoritative for mux v2 contract semantics only.
- `RFC-0034` is authoritative for multi-task production closure (gates, perf budgets, and legacy hardening mapping from `TASK-0001..0020`).
- Any requirement that spans multiple follow-on tasks must be added to `RFC-0034` rather than expanding this RFC into a backlog.

### Relationship to tasks (single execution truth)

- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` is execution truth (stop conditions, proof commands, and rollout sequencing).
- If task and RFC disagree on progress/proofs, task is authoritative.
- If task and RFC disagree on contract/interface semantics, RFC is authoritative and task must be updated.

## Context

Current DSoftBus transport supports encrypted framing on authenticated sessions but lacks explicit per-stream lifecycle, windowed backpressure, and keepalive contract for multiplexed traffic classes. Completed prerequisite seams (`TASK-0015`, `TASK-0016`, `TASK-0016B`, `TASK-0017`) make it possible to add mux behavior without reopening monolithic daemon flow.

## Goals

- Define a deterministic, bounded mux contract for multiple logical streams over one authenticated session.
- Define fail-closed flow-control and keepalive behavior with explicit ownership of mutable session state.
- Keep host-first implementation/proofs mandatory while OS backend remains gated.

## Non-Goals

- Full yamux wire compatibility.
- QUIC/datagram transport in this RFC.
- Kernel changes or kernel-level networking policy extensions.

## Constraints / invariants (hard requirements)

- **Determinism**: stable reject labels, bounded retries, and reproducible marker order.
- **No fake success**: no mux success markers before confirmed multiplexed transfer behavior.
- **Bounded resources**: explicit caps for stream count, frame payload, buffered bytes, and credit/window deltas.
- **Security floor**:
  - mux operations only on authenticated session context,
  - strict fail-closed stream-state validation,
  - explicit backpressure (`WouldBlock`/credit exhaustion), never unbounded buffering.
- **Rust/API hygiene floor**:
  - typed domain boundaries (`newtype` where class confusion is possible),
  - explicit ownership of mutable mux state (single-writer event-loop model by default),
  - critical transition/accounting outcomes marked `#[must_use]`,
  - no blanket/unsafe `Send`/`Sync` workarounds.
- **OS harness floor**:
  - OS/QEMU proofs use canonical harness defaults with modern virtio-mmio behavior,
  - no legacy-only assumptions for proof success.

## Proposed design

### Contract / interface (normative)

- **Mux frame model** (versioned):
  - `OPEN`, `OPEN_ACK`, `DATA`, `WINDOW_UPDATE`, `RST`, `PING`, `PONG`, `CLOSE`.
- **Flow control**:
  - per-stream credit windows with bounded deltas,
  - sender decrements on `DATA`, receiver restores with bounded `WINDOW_UPDATE`,
  - overflow/underflow is deterministic protocol rejection.
- **Scheduling**:
  - priority classes `0..=7` (`0` highest),
  - default contract is strict-priority selection with deterministic starvation budget:
    - serve highest available priority by default,
    - after `HIGH_PRIORITY_BURST_LIMIT` consecutive high-priority dequeues while lower priority work is pending,
      one lower-priority dequeue is required before resuming high-priority preference.
- **Keepalive**:
  - bounded heartbeat cadence and timeout thresholds,
  - missing keepalive response leads to explicit deterministic teardown.
- **Error model**:
  - invalid state transition, unknown stream, oversize frame, credit violation -> fail-closed reject path.

### Reject labels (normative)

The following reject labels are part of the deterministic contract and must stay stable:

- `mux.reject.frame_oversize`
- `mux.reject.invalid_stream_state_transition`
- `mux.reject.window_credit_overflow_or_underflow`
- `mux.reject.unknown_stream_frame`
- `mux.reject.unauthenticated_session`

### Phases / milestones (contract-level)

- **Phase 0**: lock deterministic limits/reject labels and typed ownership boundaries.
- **Phase 1**: host mux engine + negative tests + deterministic fairness/backpressure/keepalive proofs.
- **Phase 2**: OS-gated integration markers and 2-VM mux proof once backend is ready.

## Security considerations

### Threat model

- Malformed frame sequences attempt stream-state corruption or cross-stream confusion.
- Credit/window abuse attempts memory pressure or starvation.
- Keepalive manipulation attempts false liveness or premature teardown.
- Concurrency/ownership bugs in shared mutable mux state cause hidden consistency failures.

### Security invariants

- Only authenticated sessions can carry mux frames.
- Stream IDs and lifecycle transitions are validated fail-closed.
- Window/credit arithmetic is bounded and validated against overflow/underflow.
- Backpressure is explicit and bounded; no hidden unbounded queues.
- Ownership and concurrency behavior is deterministic; no hidden unsafe shared state path.

### DON'T DO

- DON'T allocate per-frame/per-stream buffers without hard caps.
- DON'T accept credit/window updates that can overflow/underflow counters.
- DON'T emit mux success markers without confirmed mux roundtrip behavior.
- DON'T bypass ownership rules with ad-hoc `unsafe` `Send`/`Sync` impls.

### Mitigations

- `test_reject_*` suite for frame/state/credit violations.
- Deterministic seeded state-machine tests for ordering and credit invariants.
- Explicit API gates (`newtype`, `#[must_use]`) for critical transition/accounting paths.
- Canonical host-first and gated OS proof ladder.

### Open risks

- Host-only mux closure can drift from OS behavior while OS backend remains gated.
- Priority policy can regress into starvation if fairness bounds are not asserted in tests.

## Failure model (normative)

- Unknown stream frame -> deterministic reject (no implicit stream creation).
- Invalid lifecycle transition -> deterministic reject.
- Oversize payload or boundedness violation -> deterministic reject.
- Credit/window overflow/underflow -> deterministic reject and stream/session fail-closed handling per policy.
- Keepalive timeout -> explicit teardown; no silent fallback to "connected" state.

Deterministic reject labels:

- `mux.reject.frame_oversize`
- `mux.reject.invalid_stream_state_transition`
- `mux.reject.window_credit_overflow_or_underflow`
- `mux.reject.unknown_stream_frame`
- `mux.reject.unauthenticated_session`

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus -- --nocapture
```

And task-owned targeted rejects/behavior checks per `TASK-0020` stop conditions, including:
- oversize frame reject,
- invalid stream transition reject,
- credit overflow/underflow reject,
- unknown stream reject,
- fairness/backpressure/keepalive deterministic coverage.

Evidence snapshot (2026-04-11):

- `cargo test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture` -> 11 passed / 0 failed
- `cargo test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture` -> 7 passed / 0 failed
- `cargo test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture` -> 5 passed / 0 failed
- `cargo test -p dsoftbus -- --nocapture` -> package test suite green
- requirement-based host contract surfaces landed in:
  - `userspace/dsoftbus/tests/mux_contract_rejects_and_bounds.rs`
  - `userspace/dsoftbus/tests/mux_frame_state_keepalive_contract.rs`
  - `userspace/dsoftbus/tests/mux_open_accept_data_rst_integration.rs`
- host integration/event-pump surface hardened in `userspace/dsoftbus/src/mux_v2.rs`
- mandatory per-phase regression gates executed for these slices:
  - `just test-e2e`
  - `just test-os-dhcp`
- OS-gated harnesses executed:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh` (green harness with mux markers)
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` (green; summary at `artifacts/os2vm/runs/os2vm_1775990226/summary.json`)
  - 2-VM mux ladder markers proven on both nodes (`tools/os2vm.sh` phase `mux`)
  - deterministic performance budget gate proven (`tools/os2vm.sh` phase `perf`)
  - bounded soak stability gate proven (`tools/os2vm.sh` phase `soak`, rounds=2, fail/panic markers remain zero)
  - release evidence bundle emitted: `artifacts/os2vm/runs/os2vm_1775990226/release-evidence.json`

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh
```

2-VM mux proof:

```bash
cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh
```

### Deterministic markers (if applicable)

- `dsoftbus:mux session up`
- `dsoftbus:mux data ok`
- `SELFTEST: mux pri control ok`
- `SELFTEST: mux bulk ok`
- `SELFTEST: mux backpressure ok`
- `dsoftbus:mux crossvm session up`
- `dsoftbus:mux crossvm data ok`
- `SELFTEST: mux crossvm pri control ok`
- `SELFTEST: mux crossvm bulk ok`
- `SELFTEST: mux crossvm backpressure ok`

## Alternatives considered

- Full yamux compatibility now: rejected (scope/coupling too broad for this slice).
- Delay all mux work until `TASK-0022` completion: rejected (host-first contract/proofs can be established now).
- Async runtime-first design: rejected (adds runtime coupling and determinism risk for OS bring-up).

## Open questions

- None currently; naming/registry fail-closed constraints are now covered by host contract/integration tests and task-owned evidence.

## RFC Quality Guidelines (for authors)

When updating this RFC, ensure:

- scope boundaries with `TASK-0020`/`TASK-0021`/`TASK-0022` remain explicit,
- deterministic/bounded rules and reject labels stay normative,
- security invariants remain linked to concrete negative tests,
- marker contracts remain stable and only reflect real behavior.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: Contract + determinism lock (`newtype`/ownership/`#[must_use]` boundaries + reject-label lock) — proof: task-owned host checks in `TASK-0020`.
- [x] **Phase 1**: Host mux engine + security rejects/fairness/backpressure/keepalive proofs — proof: `cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus -- --nocapture`.
- [x] **Phase 2**: OS-gated marker closure and 2-VM mux proof — proof: `cd /home/jenning/open-nexus-OS && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh` + `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`.
- [x] **Bridge boundary locked**: cross-task production closure is tracked in `RFC-0034`, while this RFC stays mux-contract scoped.
- [x] Task linked with stop conditions + proof commands (`TASK-0020` as SSOT).
- [x] QEMU markers (if enabled) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).

Progress note (2026-04-11):

- host phase-1 proofs remain requirement-based and green.
- phase-2 OS mux-marker closure is now proven with `REQUIRE_DSOFTBUS=1` ladder and marker checks in `scripts/qemu-test.sh`.
- 2-VM distributed mux ladder is now proven via `tools/os2vm.sh` (`phase: mux`, both nodes).
