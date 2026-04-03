# RFC-0033: DSoftBus Streams v2 mux/flow-control/keepalive (host-first, OS-gated)

- Status: In Progress
- Owners: @runtime
- Created: 2026-03-27
- Last Updated: 2026-03-27
- Links:
  - Tasks: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (execution + proof; SSOT)
  - ADRs: `docs/adr/0005-dsoftbus-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
    - `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md`
    - `docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md`

## Status at a Glance

- **Phase 0 (contract + determinism lock)**: 🟨
- **Phase 1 (host mux engine + security proofs)**: ⬜
- **Phase 2 (OS-gated marker closure)**: ⬜

Definition:

- "Complete" means the contract is defined and the proof gates are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - stream-multiplexing contract over authenticated DSoftBus sessions,
  - bounded flow-control and keepalive failure semantics,
  - deterministic reject behavior and marker contract for mux proof closure.
- **This RFC does NOT own**:
  - QUIC transport evolution (`TASK-0021`),
  - no_std core/backend extraction (`TASK-0022`),
  - kernel networking or kernel transport changes.

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
  - priority classes with bounded-starvation requirement (high-pri favored, low-pri still progresses).
- **Keepalive**:
  - bounded heartbeat cadence and timeout thresholds,
  - missing keepalive response leads to explicit deterministic teardown.
- **Error model**:
  - invalid state transition, unknown stream, oversize frame, credit violation -> fail-closed reject path.

### Phases / milestones (contract-level)

- **Phase 0**: lock deterministic limits/reject labels and typed ownership boundaries.
- **Phase 1**: host mux engine + negative tests + deterministic fairness/backpressure/keepalive proofs.
- **Phase 2**: OS-gated integration markers and optional 2-VM mux proof once backend is ready.

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

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

Optional when 2-VM mux proof exists:

```bash
cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh
```

### Deterministic markers (if applicable)

- `dsoftbus:mux session up`
- `dsoftbus:mux data ok`
- `SELFTEST: mux pri control ok`
- `SELFTEST: mux bulk ok`
- `SELFTEST: mux backpressure ok`

## Alternatives considered

- Full yamux compatibility now: rejected (scope/coupling too broad for this slice).
- Delay all mux work until `TASK-0022` completion: rejected (host-first contract/proofs can be established now).
- Async runtime-first design: rejected (adds runtime coupling and determinism risk for OS bring-up).

## Open questions

- Should priority fairness use weighted round-robin or strict-priority + starvation budget as default contract?
- Which stream naming/registry constraints are fixed in v2 contract versus left task-owned?

## RFC Quality Guidelines (for authors)

When updating this RFC, ensure:

- scope boundaries with `TASK-0020`/`TASK-0021`/`TASK-0022` remain explicit,
- deterministic/bounded rules and reject labels stay normative,
- security invariants remain linked to concrete negative tests,
- marker contracts remain stable and only reflect real behavior.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: Contract + determinism lock (`newtype`/ownership/`#[must_use]` boundaries + reject-label lock) — proof: task-owned host checks in `TASK-0020`.
- [ ] **Phase 1**: Host mux engine + security rejects/fairness/backpressure/keepalive proofs — proof: `cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus -- --nocapture`.
- [ ] **Phase 2**: OS-gated marker closure and optional 2-VM mux proof — proof: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` (+ optional `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`).
- [ ] Task linked with stop conditions + proof commands (`TASK-0020` as SSOT).
- [ ] QEMU markers (if enabled) appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
