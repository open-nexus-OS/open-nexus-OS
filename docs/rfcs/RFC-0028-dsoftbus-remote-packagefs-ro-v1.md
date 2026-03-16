# RFC-0028: DSoftBus remote packagefs RO v1

- Status: In Progress
- Owners: @runtime
- Created: 2026-03-12
- Last Updated: 2026-03-12
- Links:
  - Tasks: `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md` (execution + proof)
  - Task dependencies:
    - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
    - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - Follow-on tasks:
    - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
    - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
    - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
    - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
  - ADRs: `docs/adr/0005-dsoftbus-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md`
    - `docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md`
    - `docs/rfcs/RFC-0010-dsoftbus-cross-vm-harness-v1.md`
    - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`

## Status at a Glance

- **Phase 0 (RO protocol contract + handler boundaries)**: 🟨
- **Phase 1 (security hardening + negative tests)**: ⬜
- **Phase 2 (host/QEMU proof integration + docs sync)**: ⬜

Definition:

- “Complete” means the contract is defined and the proof gates are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - the v1 remote packagefs read-only protocol contract over authenticated DSoftBus streams
  - request bounds, fail-closed validation behavior, and deterministic marker expectations
  - the gateway/session integration boundary for packagefs RO handling in `dsoftbusd`
- **This RFC does NOT own**:
  - remote mutable state (`TASK-0017`)
  - mux/flow-control contracts (`TASK-0020`)
  - QUIC transport contracts (`TASK-0021`)
  - shared no_std core/backend extraction (`TASK-0022`)

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- `TASK-0016` is the execution SSOT for this RFC and carries the implementation/proof sequence.

## Context

`packagefsd` already serves local read-only package artifacts, and DSoftBus cross-VM/authenticated session paths are available from prior tasks. What is missing is a bounded, fail-closed remote packagefs RO contract that can be served over authenticated streams without reopening monolithic daemon control flow.

`TASK-0015` closed the structural prerequisite (`dsoftbusd` modular seams). This RFC defines the contract for the next functional slice (`TASK-0016`) so remote packagefs behavior can be added deterministically and securely.

## Goals

- Define a minimal v1 remote packagefs protocol for read-only operations (`stat/open/read/close`) over authenticated streams.
- Define strict validation and bounded resource behavior (path length, read size, concurrent handles).
- Keep marker and proof behavior deterministic for host-first and QEMU validation.

## Non-Goals

- Generic remote VFS semantics (rename/write/delete/dir mutations).
- Remote execution, capability transfer, or policy-surface expansion.
- Transport evolution (mux or QUIC) in this RFC.

## Constraints / invariants (hard requirements)

- **Determinism**: bounded retries/loops and deterministic marker semantics.
- **No fake success**: `ok/ready` markers only after real remote packagefs behavior is observed.
- **Bounded resources**: explicit caps for path/read lengths and active remote handles.
- **Security floor**:
  - authenticated session required before serving packagefs requests,
  - path traversal and non-packagefs schemes fail-closed,
  - no secret/session material in logs.
- **Stubs policy**: placeholder paths must not emit authoritative success markers.

## Proposed design

### Contract / interface (normative)

Define a compact versioned byte-frame protocol for v1 remote packagefs RO requests and responses:

- request surface: `STAT`, `OPEN`, `READ`, `CLOSE`
- response surface: `OK + payload` or deterministic error code
- all frame inputs validated before processing:
  - scheme/path whitelist (`pkg:/...` and `/packages/...` only),
  - max path length,
  - max read length/chunk,
  - max open handles per session.

Integration boundary:

- request routing and auth gating at `dsoftbusd` gateway/session seams,
- packagefs resolution delegated to local packagefs/vfs path,
- observability helpers emit deterministic markers for proof.

Versioning strategy:

- v1 is explicitly a bring-up byte-frame contract.
- if schema-based RPC is adopted later (mux/quic follow-ons), this RFC remains scoped to v1 behavior and migration must be handled by a new or follow-on RFC.

### Phases / milestones (contract-level)

- **Phase 0**: finalize v1 RO contract and bounded validation requirements.
- **Phase 1**: enforce security invariants with negative tests and fail-closed behavior.
- **Phase 2**: validate host + QEMU proof gates and sync docs.

## Security considerations

### Threat model

- unauthenticated or stale session attempts to access package content
- path traversal and scheme confusion escaping package namespace
- oversized path/read payloads causing resource pressure
- silent fallback behavior that masks security regressions

### Security invariants (MUST hold)

- remote packagefs service path is reachable only from authenticated sessions
- canonicalized request path remains under package namespace
- input bounds are enforced before expensive operations
- fail-closed behavior is deterministic for invalid auth/path/sizes

### DON'T DO list

- DON'T expose write-like operations in this RFC scope
- DON'T accept non-`pkg:/` and non-`/packages/` namespaces
- DON'T downgrade invalid auth/path validation to warning-only behavior

### Proof strategy (security)

- host negative tests:
  - `test_reject_unauthenticated_stream_request`
  - `test_reject_path_traversal`
  - `test_reject_non_packagefs_scheme`
  - `test_reject_oversize_read_or_path`
- QEMU markers:
  - `dsoftbusd: remote packagefs served`
  - `SELFTEST: remote pkgfs read ok`

## Failure model (normative)

- invalid request framing, auth, scheme/path, or bounds must return deterministic errors without side effects
- no silent fallback to broader filesystem scope
- close/cleanup semantics must be idempotent and bounded under reconnect/retry

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p remote_e2e -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh
```

### Deterministic markers (if applicable)

- `dsoftbusd: remote packagefs served`
- `SELFTEST: remote pkgfs read ok`

## Alternatives considered

- route remote packagefs directly through shared `userspace/dsoftbus` OS backend first
  - rejected for this slice because shared backend extraction is explicitly tracked by `TASK-0022`
- defer all remote packagefs work until mux/quic lands
  - rejected because it blocks a useful RO capability that is already enabled by current daemon seams

## Open questions

- should v1 path normalization rules be shared as a reusable helper before `TASK-0017` starts?
- at what point should v1 byte frames be migrated to schema-based RPC in follow-on transport work?

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- scope boundaries are explicit; cross-RFC ownership is linked
- determinism + bounded resources are specified in constraints section
- security invariants are stated (threat model, mitigations, DON'T DO)
- proof strategy is concrete (not "we will test this later")
- if claiming stability: define ABI/on-wire format + versioning strategy
- stubs (if any) are explicitly labeled and non-authoritative

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: v1 remote packagefs RO contract + bounded validation finalized — proof: `just diag-host`
- [ ] **Phase 1**: security-negative tests for auth/path/scheme/size are green — proof: `cargo test -p dsoftbusd -- --nocapture`
- [ ] **Phase 2**: host + QEMU proof markers are green and docs synced — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
