# RFC-0037: DSoftBus QUIC v2 OS enablement gated contract

- Status: In Progress
- Owners: @runtime
- Created: 2026-04-15
- Last Updated: 2026-04-15
- Links:
  - Execution SSOT: `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`
  - Follow-up implementation task: `tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md`
  - Program gate track: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
  - Status board: `tasks/STATUS-BOARD.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0034-dsoftbus-production-closure-v1.md`
    - `docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md`
    - `docs/rfcs/RFC-0036-dsoftbus-core-no-std-transport-abstraction-v1.md`

## Status at a Glance

- **Phase A (contract lock + gated decision)**: ✅
- **Phase B (host proof: fail-closed gate integrity)**: ✅
- **Phase C (OS-gated fallback proof, no fake QUIC success)**: ✅
- **Phase D (feasibility unlock criteria + deterministic perf/security budgets)**: ⬜
- **Phase E (closure sync + queue handoff evidence)**: ⬜

Definition:

- "Complete" means this gated contract is implemented and proven by task-owned tests/markers.
- "Gated" is an explicit outcome, not a placeholder: OS QUIC remains blocked until feasibility evidence exists.
- `In Progress` means the gate contract is being actively closed; it does **not** claim OS QUIC enablement.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Execution truth remains in tasks.

- **This RFC owns**:
  - gated contract semantics for OS QUIC v2 in `TASK-0023`,
  - explicit fail-closed behavior while OS QUIC remains blocked,
  - feasibility unlock criteria and proof shape,
  - security, determinism, and boundedness requirements for any future unlock.
- **This RFC does NOT own**:
  - implementation of OS secure UDP transport (owned by `TASK-0024`),
  - QUIC tuning/performance breadth (owned by `TASK-0044`),
  - mux v2 protocol semantics (owned by `RFC-0033`),
  - kernel/MMIO contract changes (owned by kernel/driver RFC/task families).

### Relationship to tasks (single execution truth)

- `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md` is authoritative for stop conditions and proof commands.
- If this RFC and task disagree on contract semantics, this RFC is authoritative and task text must be aligned.
- If this RFC and task disagree on progress/proof status, task is authoritative.

## Program alignment (production-floor + modern virtio-mmio)

- This RFC keeps `DSoftBus & Distributed` aligned to `production-floor` closure direction in `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.
- Marker honesty and bounded behavior are mandatory for this gate; no "half-enabled" QUIC claims.
- OS/QEMU closure claims in this task family must keep the modern virtio-mmio floor (`virtio-mmio.force-legacy=off`) via canonical harness usage.

## Context

`TASK-0021` closed host-first QUIC selection/fallback semantics and intentionally kept OS QUIC disabled by default.
`TASK-0022` closed the no_std core seam (`dsoftbus-core`) so host/OS boundaries are explicit and reusable.

The remaining question for OS QUIC v2 is feasibility under no_std + deterministic timer/resource constraints. Until that is proven, the contract requires explicit gating and fallback proof.

## Goals

- Define an honest, production-floor gate contract for OS QUIC v2 status (`Blocked` until feasibility proof exists).
- Preserve strict fail-closed downgrade/cert/ALPN behavior already proven on host.
- Lock clear routing to follow-up execution tasks (`TASK-0024` implementation, `TASK-0044` tuning).
- Define Rust discipline expectations for follow-up implementation (newtypes, ownership, `#[must_use]`, reviewed `Send`/`Sync`).

## Non-Goals

- Claiming OS QUIC session success while feasibility is unresolved.
- Shipping partial QUIC behavior with fake-success markers.
- Kernel changes or legacy virtio-mmio proof paths.

## Constraints / invariants (hard requirements)

- **Determinism**: bounded retries, stable marker order, explicit error outcomes.
- **No fake success**: no OS QUIC success markers while the task remains blocked.
- **No silent fallback**: strict QUIC mode must fail closed; fallback is explicit/auditable only in allowed mode.
- **Bounded resources**: parser/input/retry/timer paths remain bounded and reject oversized/malformed inputs.
- **Security floor**:
  - cert/ALPN/identity validation failures are hard rejects,
  - unauthenticated data paths are rejected before payload processing,
  - security decisions remain policy-aware and audit-friendly.
- **Rust discipline floor** (for unlock/follow-up code):
  - domain IDs and mode/state selectors use `newtype` wrappers,
  - decision-bearing return values are `#[must_use]`,
  - ownership transfer across transport/session boundaries is explicit,
  - `Send`/`Sync` assumptions are reviewed and validated via compile-time assertions (no unsafe blanket impls).
- **Modern virtio-mmio floor**: QEMU proof claims must use canonical modern virtio-mmio path; no legacy-mode closure claims.

## Proposed design

### Contract / interface (normative)

- Transport selection contract remains `DSOFTBUS_TRANSPORT=tcp|quic|auto`.
- While OS QUIC is blocked:
  - `quic` mode in OS remains unavailable and must fail closed by contract,
  - `auto` mode in OS must emit deterministic fallback markers when QUIC is unavailable,
  - no marker may imply successful OS QUIC session establishment.
- Feasibility unlock requires explicit proof that selected QUIC stack can satisfy:
  - no_std viability (or strictly isolated std boundary with auditable constraints),
  - deterministic timer behavior without hidden runtime assumptions,
  - bounded memory/resource behavior under loss/retry pressure,
  - security invariant preservation (identity/cert/ALPN and reject-path behavior).

### Marker contract (normative while blocked)

- `dsoftbus: quic os disabled (fallback tcp)`
- `SELFTEST: quic fallback ok`

No QUIC success marker is allowed while this gate is blocked.

### Phases / milestones (contract-level)

- **Phase A**: lock gate outcome and ownership boundaries (`TASK-0023` blocked; routing to `TASK-0024`/`TASK-0044` explicit).
- **Phase B**: host fail-closed proofs stay green with requirement-named negative tests.
- **Phase C**: OS fallback marker contract remains deterministic and honest.
- **Phase D**: feasibility unlock criteria + deterministic perf/security budget proofs are implemented and green.
- **Phase E**: docs/testing/board/order/handoff sync to actual proof state.

## Security considerations

### Threat model

- silent downgrade from requested QUIC behavior,
- invalid/untrusted cert or ALPN drift being accepted,
- malformed/oversized input causing resource pressure or parser abuse,
- confusion between transport handshake state and policy authorization state.

### Mitigations

- strict fail-closed reject semantics for QUIC validation failures,
- explicit fallback markers in blocked state (no hidden success),
- requirement-named `test_reject_*` suites for negative paths,
- bounded parser/retry/timer behavior and deterministic evidence.

### DON'T DO

- DON'T emit OS QUIC success markers while blocked.
- DON'T treat cert/ALPN/auth failures as warnings.
- DON'T bypass policy checks due to transport mode.
- DON'T claim feasibility closure without explicit proof evidence.

## Failure model (normative)

- `mode=quic` in blocked OS path: explicit hard failure (no implicit TCP success claim).
- `mode=auto` in blocked OS path: deterministic fallback with required markers.
- ALPN/cert/identity mismatch: explicit reject and no session promotion.
- Oversized/malformed transport input: explicit reject before unbounded allocation/state growth.

## Behavior-first proof selection (Rule 07)

### Target behavior (must be true at done)

- Blocked-state contract is explicit and honest: OS QUIC remains unavailable until feasibility is proven.
- Strict fail-closed security semantics remain enforced and regression-safe.
- Fallback behavior is deterministic, auditable, and marker-backed.

### Main break point (dishonest/unsafe if broken)

- Any regression that silently downgrades strict QUIC mode or emits QUIC success while still blocked would make this contract security-dishonest.

### Minimal proof shape (smallest honest set)

- **Primary proof (host security/fail-closed behavior)**:
  - `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
  - includes required rejects:
    - `test_reject_quic_strict_mode_downgrade`
    - `test_reject_quic_invalid_or_untrusted_cert`
    - `test_reject_quic_wrong_alpn`
- **Secondary proof (real OS boundary blind spot)**:
  - canonical fallback marker proof in QEMU:
    - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- **Only when feasibility unlock is claimed**:
  - add unlock-specific no_std/runtime/crypto proof gates and (if distributed claims are made) `tools/os2vm.sh`.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus --test quic_selection_contract -- --nocapture
```

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture
```

```bash
cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic
```

### Proof (OS/QEMU blocked-state)

```bash
cd /home/jenning/open-nexus-OS && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Proof (unlock feasibility criteria, when re-opened)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus -- quic --nocapture
```

- Additional dedicated feasibility spike proof commands are required before changing status from blocked.

### Regression / floor

```bash
cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp
```

### Deterministic markers (required while blocked)

- `dsoftbus: quic os disabled (fallback tcp)`
- `SELFTEST: quic fallback ok`

## Baseline evidence refresh (2026-04-15)

- Host gate baseline (green):
  - `just test-dsoftbus-quic`
- OS blocked-state marker baseline (green):
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - observed markers:
    - `dsoftbus: quic os disabled (fallback tcp)`
    - `SELFTEST: quic fallback ok`

## Alternatives considered

- Enable partial OS QUIC now and "fix later": rejected (fake-success and security drift risk).
- Fold gate semantics into older RFCs (`RFC-0035`/`RFC-0036`): rejected (scope drift; seed-per-follow-up rule).
- Skip explicit blocked RFC and track in task text only: rejected (contract ambiguity across follow-up tasks).

## Open questions

- Which concrete QUIC stack profile (or isolated boundary strategy) can satisfy no_std/runtime/security constraints without hidden std/runtime coupling? (owner: @runtime, tracked via `TASK-0023` feasibility gate)
- If feasibility eventually unlocks, should the first OS QUIC closure claim remain `production-floor` only until `TASK-0044` tuning closure is complete? (owner: @runtime)

## RFC Quality Guidelines (for authors)

When updating this RFC, ensure:

- blocked vs unlocked semantics stay explicit and test-linked,
- marker contract stays deterministic and anti-fake-success,
- follow-up ownership boundaries (`TASK-0024`, `TASK-0044`) stay explicit,
- Rust safety discipline requirements remain normative for follow-up implementation.

---

## Implementation Checklist

**This section tracks implementation progress.**

- [x] **Phase A**: gated contract lock + routing boundaries synchronized — proof: `TASK-0023` + RFC alignment.
- [x] **Phase B**: host fail-closed reject suites green — proof: `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture` and `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`.
- [x] **Phase C**: OS blocked-state fallback markers green — proof: `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`.
- [ ] **Phase D**: feasibility unlock criteria proven with deterministic budgets/security rejects — proof: task-owned dedicated feasibility gates.
- [ ] **Phase E**: closure sync across task/testing/board/order/handoff completed — proof: documentation and status sync review.
- [x] Task linked as execution SSOT (`tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`).
- [x] Security-relevant negative tests exist and are green (`test_reject_*` QUIC suite).
