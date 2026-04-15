# RFC-0036: DSoftBus core no_std transport abstraction v1

- Status: Draft
- Owners: @runtime
- Created: 2026-04-14
- Last Updated: 2026-04-14
- Links:
  - Execution SSOT: `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
  - Program gate track: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
  - Status board: `tasks/STATUS-BOARD.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
    - `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md`
    - `docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md`

## Status at a Glance

- **Phase A (contract lock + boundary freeze)**: 🟨
- **Phase B (host core proofs + reject paths)**: ⬜
- **Phase C (OS-compilable integration + marker discipline)**: ⬜
- **Phase D (deterministic perf + zero-copy discipline)**: ⬜
- **Phase E (closure sync + handoff evidence)**: ⬜

Definition:

- "Complete" means this contract is implemented with green proof gates. It is a production-class closure level for the DSoftBus distributed stack, not a blanket production-ready claim for unrelated subsystems.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Execution checklist and proofs live in `TASK-0022`.

- **This RFC owns**:
  - no_std + alloc DSoftBus core boundary contract,
  - transport abstraction contract (core vs backend responsibilities),
  - zero-copy-first data-path policy at core/backend boundaries,
  - Rust API discipline for core surfaces (`newtype`, ownership, `#[must_use]`, reviewed `Send`/`Sync`).
- **This RFC does NOT own**:
  - enabling OS QUIC transport (`TASK-0023`),
  - QUIC tuning/perf matrix breadth (`TASK-0044`),
  - changing mux v2 wire semantics already governed by `RFC-0033`,
  - reopening `TASK-0021` transport-selection/fallback contract.

### Relationship to tasks (single execution truth)

- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md` is authoritative for stop conditions and proof commands.
- If this RFC and task disagree on contract semantics, this RFC is authoritative and task text must be aligned.
- If they disagree on progress/proof status, task is authoritative.

## Program alignment (production-class language)

- This contract targets `DSoftBus & Distributed` `production-floor` trajectory in `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.
- Wording policy: we use **production-class** (quality class) for this slice, not broad "production-ready" branding.
- Kernel/runtime production-ready expectations must remain cleanly preserved: no kernel boundary changes, no regression in modern virtio-mmio QEMU proof ladders, and no fake-success markers.

## Context

`TASK-0021` closed a host-first QUIC scaffold with deterministic fallback semantics, while OS QUIC remains disabled by default. The next blocker is architectural: DSoftBus protocol/state logic is still coupled to std-era backend choices, limiting direct reuse in no_std OS contexts.

Without this split, `TASK-0023` OS enablement risks either duplicated logic or unsafe drift between host and OS behavior.

## Distributed fabric guidance (adapted)

Mature distributed service-fabric decomposition is used here as architectural guidance, adapted to this codebase's capability/security model:

- Keep **discovery/auth/transmission** as separable concerns with explicit interfaces.
- Keep **authentication** separate from **policy authority** (auth success must not grant permission by itself).
- Keep service identity **channel-authoritative** (`sender_service_id`) rather than payload-declared.
- Keep protocol semantics **transport-agnostic** so TCP/QUIC/OS backends do not fork behavior.
- Keep data plane **bulk-oriented and zero-copy-first** while control plane remains compact and bounded.

## Goals

- Define a no_std-capable DSoftBus core contract (`#![no_std]` + `alloc`) with deterministic state transitions.
- Define explicit transport adapter responsibilities so host and OS backends can share core protocol logic.
- Preserve all externally visible `TASK-0021` behavior contracts while refactoring internals.
- Enforce zero-copy-first behavior for bulk data paths where feasible.

## Non-Goals

- Enabling OS QUIC data path in this RFC/task.
- Introducing new mux semantics beyond `RFC-0033`.
- Kernel MMIO, scheduler, or memory-management contract changes.

## Constraints / invariants (hard requirements)

- **Kernel untouched**: no kernel API or behavior changes.
- **no_std floor**: core crate compiles as `#![no_std]` + `extern crate alloc`.
- **Determinism**: bounded retries, stable marker semantics, explicit error outcomes.
- **No fake success**: no success markers for placeholder/stub paths.
- **Zero-copy-first**: metadata may be copied in bounded fashion; bulk paths should prefer borrowed/VMO-backed buffers where possible.
- **Rust discipline**: apply `newtype`, ownership boundaries, `#[must_use]`, and reviewed `Send`/`Sync` behavior where safety-relevant.
- **Modern MMIO proof floor**: OS/QEMU validation stays on modern virtio-mmio defaults; no legacy-mode dependency for closure claims.

## Proposed design

### Contract / interface (normative)

- Introduce a transport-neutral core surface for session/state/protocol progression:
  - core state machine consumes validated events/frames,
  - backend adapters own IO specifics (host TCP/QUIC today, OS transport later),
  - policy authority and identity truth remain outside transport core.
- Plane separation contract (normative):
  - discovery plane publishes bounded, validated peer/service facts,
  - auth/session plane establishes peer authenticity and session lifecycle,
  - transmission plane carries framed control messages and bounded bulk data.
  - no plane may silently absorb another plane's authority decisions.
- Stable boundary rules:
  - identity inputs are channel-authoritative (`sender_service_id`) and never payload strings,
  - frame parsing is bounded and explicit about reject paths,
  - backpressure signals are explicit, bounded, and testable.
- Data-path policy:
  - control-plane messages remain compact and bounded,
  - bulk payload path documents copy vs borrow decisions and prefers zero-copy-capable forms.

### Phases / milestones (contract-level)

- **Phase A**: lock boundary contracts + invariants (`TASK-0022` task text + links synced).
- **Phase B**: host reject/behavior suites prove core invariants and no regression in baseline contracts.
- **Phase C**: OS build integration proves no_std boundary can compile and run bounded marker ladders when touched.
- **Phase D**: deterministic perf budget and zero-copy discipline proofs are green.
- **Phase E**: docs/board/handoff sync complete with evidence.

## Security considerations

### Threat model

- Confused deputy via identity drift during core/backend split.
- Replay or correlation bugs introduced by refactor.
- Copy-heavy fallback paths causing hidden unbounded memory pressure.
- Unsound concurrency assumptions (`Send`/`Sync`) on shared session/core state.

### Security invariants

- Identity/auth decisions remain fail-closed and transport-agnostic.
- Session correlation remains bounded and deterministic.
- Parser and frame handling enforce hard size bounds before allocation/use.
- No secret/session material leakage in logs/errors.
- Any concurrency boundary must preserve ownership guarantees without unsafe blanket trait shortcuts.
- Auth success is necessary but not sufficient: policy/entitlement checks remain explicit and external to transport core.

### DON'T DO

- DON'T move policy authority into transport core.
- DON'T accept payload identity claims as authority truth.
- DON'T add hidden std dependencies to no_std core paths.
- DON'T regress strict reject/fallback semantics proven in `TASK-0021`.
- DON'T force `Send`/`Sync` with unsafe blanket impls.

## Failure model (normative)

- Invalid state transition: explicit reject (no implicit repair path).
- Correlation nonce mismatch/stale reply: explicit reject and bounded cleanup.
- Oversize frame/record: explicit reject before unbounded allocation.
- Unauthenticated message path: explicit reject and audit-friendly label.
- Missing OS backend capability in this phase: explicit unsupported/degraded behavior, never "ok".

## Proof / validation strategy (required)

### Proof (Host baseline freeze)

```bash
cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic
```

### Proof (Host core/reject suites)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus -- reject --nocapture
```

Required reject assertions for this contract family include:

- `test_reject_unauthenticated_message_path`
- `test_reject_payload_identity_spoof_vs_sender_service_id`

### Proof (OS/QEMU + hygiene when touched)

```bash
cd /home/jenning/open-nexus-OS && just dep-gate && just diag-os
```

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Proof (Regression floor)

```bash
cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp
```

### Deterministic markers (if OS path exercised)

- `dsoftbusd: ready`
- `dsoftbusd: auth ok`

## Alternatives considered

- Keep separate host/OS protocol stacks: rejected (drift and duplicated security logic risk).
- Make core std-first and shim no_std later: rejected (defers boundary correctness and increases migration risk).
- Fold policy decisions into transport abstraction: rejected (authority boundary violation).

## Open questions

- Which concrete bulk-path abstraction should be canonical in core-facing adapter contracts for zero-copy evidence (`Bytes`-style borrow view vs explicit VMO/filebuffer handles)? (owner: @runtime)
- Should `Send`/`Sync` expectations be encoded as compile-time trait assertions per core boundary type set in host tests? (owner: @runtime)

## RFC Quality Guidelines (for authors)

When updating this RFC, ensure:

- contract boundaries stay explicit (core vs backend vs policy authority),
- production-class language is used consistently (no broad production-ready overclaim),
- zero-copy and Rust-discipline rules remain normative and test-linked,
- proof commands remain concrete and match `TASK-0022`.

---

## Implementation Checklist

**This section tracks implementation progress.**

- [x] **Phase A**: contract lock + boundary freeze — proof: task + RFC alignment in `TASK-0022`.
- [ ] **Phase B**: host reject/core suites + baseline freeze remain green — proof: `just test-dsoftbus-quic` and `cargo test -p dsoftbus -- reject --nocapture`.
- [ ] **Phase C**: OS compile/integration boundary green where touched — proof: `just dep-gate && just diag-os && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`.
- [ ] **Phase D**: deterministic perf + zero-copy discipline evidence green — proof: task-owned perf/bounds suites.
- [ ] **Phase E**: docs/testing/status/handoff synchronized with proof evidence.
- [x] Task linked as execution SSOT.
- [ ] Security-relevant negative tests exist and are green (`test_reject_*` family for state/correlation/bounds/auth).
