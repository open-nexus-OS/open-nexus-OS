# RFC-0027: DSoftBusd modular daemon structure v1

- Status: In Progress
- Owners: @runtime
- Created: 2026-03-12
- Last Updated: 2026-03-12
- Links:
  - Tasks: `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md` (execution + proof)
  - ADRs: `docs/adr/0005-dsoftbus-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md`
    - `docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md`
    - `docs/rfcs/RFC-0010-dsoftbus-cross-vm-harness-v1.md`

## Status at a Glance

- **Phase 0 (module boundary definition)**: ⬜
- **Phase 1 (behavior-preserving extraction)**: ⬜
- **Phase 2 (proof + docs sync)**: ⬜

Definition:

- “Complete” means the contract is defined and the proof gates are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - the internal architectural boundaries for `source/services/dsoftbusd/` during the refactor
  - the rule that transport IPC, discovery, session lifecycle, gateway/local IPC, and observability become explicit internal seams
  - the compatibility floor that this refactor must preserve existing DSoftBus behavior, marker semantics, and wire formats
  - the handoff seam between daemon-local refactor work in `TASK-0015` and later shared-core extraction in `TASK-0022`
- **This RFC does NOT own**:
  - new transport features such as QUIC, mux, remote-fs, or statefs protocols
  - any kernel, `netstackd`, or `userspace/dsoftbus` contract changes
  - execution checklists, touched-path allowlists, or proof commands beyond what the task defines

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- This RFC links only the contract for the `dsoftbusd` internal refactor; `TASK-0015` remains the execution single source of truth.

## Context

`source/services/dsoftbusd/src/main.rs` currently contains most of the OS daemon behavior in one file:

- entry/wiring,
- nonce-correlated `netstackd` IPC transport helpers,
- UDP discovery state,
- session lifecycle / reconnect FSM,
- Noise XK handshake orchestration,
- cross-VM remote proxy handling,
- local IPC handling,
- metrics/logd helpers.

This concentration raises review risk and makes follow-on work (`TASK-0016`, `TASK-0020`, `TASK-0021`, `TASK-0022`) expensive because any change re-opens the full daemon instead of one seam. We need an explicit internal architecture for `dsoftbusd` before adding more behavior.

## Goals

- Define a stable internal module boundary for `dsoftbusd` that separates transport IPC, discovery, session lifecycle, gateway, and observability responsibilities.
- Preserve the externally visible behavior of the current daemon while refactoring.
- Make later DSoftBus follow-on tasks able to reuse clear seams instead of editing a monolithic `main.rs`.

## Non-Goals

- Defining a new public ABI or on-wire DSoftBus protocol version.
- Moving this logic into shared `userspace/dsoftbus` crates in this RFC.
- Adding new markers, new features, or new service authority boundaries.

## Constraints / invariants (hard requirements)

- **Determinism**: marker ordering, retry budgets, and bounded loops must remain deterministic.
- **No fake success**: no `ok/ready` marker semantics may change as part of the refactor.
- **Bounded resources**: extraction must preserve explicit caps/budgets for would-block retries, inbox correlation, and record sizes.
- **Security floor**:
  - authenticated session gating remains intact,
  - remote proxy remains deny-by-default,
  - nonce-correlated reply handling remains fail-closed,
  - secrets/session material never appear in logs.
- **Stubs policy**: refactor may not replace real behavior with placeholder abstractions that still claim success.
- **No silent contract drift**: wire formats and proof marker names stay unchanged in this RFC.

## Proposed design

### Contract / interface (normative)

This RFC defines the internal architecture contract for `dsoftbusd` v1:

- `main.rs` becomes a thin environment/entry wrapper.
- OS daemon responsibilities are split into explicit internal modules, with at least these seams:
  - runtime entry / top-level orchestration,
  - `netstackd` IPC adapter,
  - discovery state + announce/peer learning,
  - session lifecycle + reconnect FSM,
  - handshake / encrypted record handling,
  - local IPC / remote gateway surface,
  - observability helpers.
- The refactor is **behavior-preserving**:
  - same wire formats,
  - same marker names,
  - same single-VM proof behavior,
  - same cross-VM proof behavior.

Versioning strategy:

- This is an internal daemon-structure contract, not a public wire/ABI version.
- If a later task changes public transport, gateway, or shared-core contracts, that follow-on work must create or update a dedicated RFC rather than silently expanding this one.

### Phases / milestones (contract-level)

- **Phase 0**: Define and land the internal module boundary for `dsoftbusd` without changing proof behavior.
- **Phase 1**: Extract existing logic into those seams while preserving marker and wire semantics.
- **Phase 2**: Prove parity via canonical single-VM and cross-VM runs, and sync developer docs to the new daemon shape.

## Security considerations

- **Threat model**:
  - refactor regressions weaken authenticated-session gating
  - stale replies become misassociated if nonce handling drifts
  - remote proxy surface widens accidentally during extraction
  - marker/proof drift hides regressions behind “refactor only” claims
- **Mitigations**:
  - preserve existing nonce-correlated transport adapter semantics
  - preserve deny-by-default remote proxy behavior
  - keep canonical QEMU proofs unchanged and rerun them after extraction
  - add narrow unit tests where extraction makes reply/FSM seams independently testable
- **Open risks**:
  - the cross-VM path may still require careful duplication cleanup if single-VM and cross-VM helpers share only part of the transport stack
  - the best seam for future `TASK-0022` reuse may become clearer only during implementation

## Failure model (normative)

- If the refactor changes marker names, marker meaning, wire layouts, or retry semantics, the refactor is considered a failure for this RFC.
- If fallback behavior exists during extraction, it must remain explicit and behavior-equivalent; no silent fallback that changes proof semantics is allowed.
- If the refactor reveals a missing external contract, work must stop and either:
  - narrow the task back to the existing contract, or
  - create a new RFC/ADR for the newly discovered boundary.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh
```

### Deterministic markers (if applicable)

- `dsoftbusd: auth ok`
- `SELFTEST: dsoftbus ping ok`
- `dsoftbusd: discovery cross-vm up`
- `dsoftbusd: cross-vm session ok`
- `SELFTEST: remote resolve ok`
- `SELFTEST: remote query ok`

## Alternatives considered

- Keep `main.rs` monolithic and defer cleanup until a feature task.
  - Rejected because every upcoming DSoftBus feature would continue to reopen the whole daemon and increase drift risk.
- Extract directly into shared crates now (`userspace/dsoftbus` / new libs).
  - Rejected because that overlaps with `TASK-0022` and would broaden scope beyond a behavior-preserving refactor.
- Split aggressively by speculative future feature (`quic`, `mux`, `remote-fs`, `statefs`).
  - Rejected because it bakes in future assumptions before the current daemon seams are stabilized.

## Open questions

- Which extracted pieces should later move into shared no_std-capable crates under `TASK-0022`?
- Should encrypted request/response record handling remain daemon-local, or become a reusable protocol module later?

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: module boundary defined and reflected in `dsoftbusd` structure — proof: `just diag-os && just diag-host`
- [ ] **Phase 1**: behavior-preserving extraction completed for transport/discovery/session/gateway seams — proof: `cargo test -p dsoftbusd -- --nocapture`
- [ ] **Phase 2**: single-VM and cross-VM proofs remain green after refactor — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
