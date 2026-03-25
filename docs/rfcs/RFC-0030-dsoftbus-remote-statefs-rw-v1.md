# RFC-0030: DSoftBus remote statefs RW v1

- Status: Draft
- Owners: @runtime
- Created: 2026-03-24
- Last Updated: 2026-03-24
- Links:
  - Tasks: `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md` (execution + proof)
  - Task dependencies:
    - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
    - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
    - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
    - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
    - `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
    - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - Follow-on tasks:
    - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
    - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
    - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
  - ADRs: `docs/adr/0005-dsoftbus-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md`
    - `docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md`
    - `docs/rfcs/RFC-0010-dsoftbus-cross-vm-harness-v1.md`
    - `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
    - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
    - `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md`

## Status at a Glance

- **Phase 0 (contract + ACL model)**: [ ]
- **Phase 1 (security hardening + negative tests)**: [ ]
- **Phase 2 (proof integration + docs sync)**: [ ]

Definition:

- "Complete" means the contract is defined and the proof gates are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - v1 remote statefs RW contract over authenticated DSoftBus streams
  - ACL model (deny-by-default, writable remote namespace constraints)
  - deterministic audit requirements for remote state mutations
  - bounded request model and fail-closed error behavior for remote statefs operations
- **This RFC does NOT own**:
  - full remote VFS semantics
  - packagefs RO contract (already in RFC-0028)
  - mux/flow-control transport redesign (TASK-0020)
  - QUIC transport contract (TASK-0021)
  - shared no_std core extraction (TASK-0022)

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- `TASK-0017` is the execution SSOT for this RFC and carries implementation/proof sequencing.

## Context

`statefsd` provides local persistent key/value state. DSoftBus authenticated session paths are already present, but a bounded, audited, fail-closed remote RW state contract does not yet exist. We need a narrow proxy contract so distributed workflows can use remote state updates without opening unrestricted remote filesystem semantics.

## Goals

- Define a minimal versioned v1 remote statefs RW contract (`GET`, `PUT`, `DELETE`, `LIST`, `SYNC`) over authenticated streams.
- Enforce deny-by-default ACL for remote writes/deletes and deterministic fail-closed behavior for rejects.
- Require deterministic audit evidence for every remote mutation.
- Keep host/QEMU proof strategy deterministic and reproducible.

## Non-Goals

- Full remote `/state` access.
- Generic remote filesystem capabilities.
- Transport-layer redesign (mux/quic/no_std core).

## Constraints / invariants (hard requirements)

- **Determinism**: bounded loops/retries and deterministic marker/error semantics.
- **No fake success**: no success marker unless real remote statefs behavior occurred.
- **Bounded resources**: explicit limits for key length, value length, list/chunk size, and concurrent request handling.
- **Security floor**:
  - authenticated session identity is mandatory for remote RW requests,
  - remote write scope is deny-by-default and constrained to allowed namespace,
  - path normalization/prefix checks are fail-closed,
  - remote mutation audit evidence is mandatory.
- **Stubs policy**: bring-up stubs must be explicitly labeled and non-authoritative.

## Proposed design

### Contract / interface (normative)

Define a compact v1 byte-frame RPC contract for remote statefs operations:

- operations: `GET`, `PUT`, `DELETE`, `LIST`, `SYNC`
- response model: deterministic success/error status with bounded payloads
- mandatory validation before mutation/read side effects:
  - authenticated session identity
  - canonicalized key path/namespace ACL check
  - request/size bounds
  - operation-specific argument validation
- remote mutation audit contract:
  - each successful or rejected remote `PUT`/`DELETE` emits deterministic audit evidence
  - evidence path is logd-backed when available, with deterministic fallback marker if not.

Versioning strategy:

- v1 is a bounded bring-up byte-frame contract.
- any migration to schema-based RPC belongs to a new or follow-on RFC (out of scope here).

### Phases / milestones (contract-level)

- **Phase 0**: lock v1 remote statefs RW contract + ACL/audit invariants.
- **Phase 1**: enforce security invariants with negative tests and deterministic fail-closed behavior.
- **Phase 2**: integrate marker proofs for host/single-VM/2-VM and sync docs/contracts.

## Security considerations

- **Threat model**:
  - unauthenticated or spoofed remote mutation attempts
  - ACL bypass via prefix/path-normalization confusion
  - oversize payload abuse causing resource pressure
  - missing/ambiguous audit evidence for remote mutations
- **Mitigations**:
  - authenticated session identity gating
  - canonical key normalization + deny-by-default ACL
  - explicit bounded sizes and deterministic reject paths
  - mandatory mutation audit emission contract
- **Open risks**:
  - request idempotency under reconnect/retry needs careful handling to avoid duplicate side effects
  - byte-frame v1 contract can accrue debt if follow-on migration is delayed

### DON'T DO list

- DON'T allow remote mutation outside approved ACL namespace.
- DON'T trust client-provided identity fields.
- DON'T emit success markers when ACL/auth/audit guarantees are not proven.
- DON'T silently downgrade audit failures to warning-only behavior.

## Failure model (normative)

- Invalid auth/ACL/bounds/frame conditions must fail closed with deterministic status/error behavior.
- No silent fallback to broader access scope.
- Retry/reconnect handling must not create unbounded loops or duplicate mutation side effects without explicit contract handling.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd --tests -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh
```

### Deterministic markers (if applicable)

- `dsoftbusd: remote statefs served`
- `SELFTEST: remote statefs rw ok`

## Alternatives considered

- Reuse packagefs RO contract shape without adding explicit ACL/audit mutation semantics.
  - Rejected because RW mutation requires stricter security and audit contracts.
- Defer remote statefs RW until mux/quic work lands.
  - Rejected because current seams are ready and this capability is a direct follow-on slice.

## Open questions

- Which idempotency strategy (if any) should be mandatory for duplicate remote mutation frames under reconnect?
- Should ACL namespace remain `/state/shared/*` only in v1, or allow an explicit additional policy-defined prefix?

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- scope boundaries are explicit and linked to follow-on ownership,
- determinism and bounded resources are specified in constraints,
- security invariants and DON'T DO rules are explicit,
- proof strategy is concrete and reproducible,
- v1/on-wire behavior and migration intent are explicit.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: v1 remote statefs RW contract + ACL/audit invariants finalized — proof: `just diag-os`
- [ ] **Phase 1**: security-negative tests for ACL/auth/bounds are green — proof: `cargo test -p dsoftbusd --tests -- --nocapture`
- [ ] **Phase 2**: host + QEMU proofs are green and docs/contracts synced — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
