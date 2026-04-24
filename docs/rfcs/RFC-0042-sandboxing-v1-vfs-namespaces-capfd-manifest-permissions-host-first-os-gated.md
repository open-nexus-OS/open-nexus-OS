# RFC-0042: Sandboxing v1 userspace confinement (VFS namespaces + CapFd + manifest permissions, host-first, OS-gated)

- Status: Done
- Owners: @runtime @security
- Created: 2026-04-23
- Last Updated: 2026-04-24
- Links:
  - Tasks: `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md` (execution + proof)
  - Related RFCs: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
  - Related RFCs: `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
  - Related RFCs: `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md`
  - Production gate policy: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`

## Status at a Glance

- **Phase 0 (contract floor + host reject proofs)**: 🟩
- **Phase 1 (OS-gated enforcement markers)**: 🟩
- **Phase 2 (production-grade breadth handoff boundaries)**: 🟩

Definition:

- "Done" means this RFC's userspace-confinement contract is implemented with deterministic proof gates and explicit anti-drift boundaries to follow-up hardening tasks.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - v1 userspace sandbox contract: per-subject namespace views, CapFd rights model, and manifest-permission bootstrap flow.
  - deterministic deny-by-default behavior for traversal/forgery/replay/unauthorized path access.
  - honest security-boundary language for kernel-unchanged scope.
- **This RFC does NOT own**:
  - kernel-enforced namespaces or syscall-level sandbox guarantees.
  - full production hardening breadth from quota/egress/profile-distribution follow-ups (`TASK-0043`, `TASK-0189`).
  - unrelated storage/packagefs closure tasks (`TASK-0032`, `TASK-0033`).

### Relationship to tasks (single execution truth)

- `TASK-0039` is the execution SSOT for this RFC.
- Task stop conditions and proof commands are authoritative for closure.
- Follow-up scope stays explicit and must not be silently absorbed.

## Context

Current repo posture is kernel-unchanged with userspace service authority patterns. `vfsd` today is minimal and `pkg:/`-centric, so sandboxing v1 must begin host-first and only claim OS closure where actual enforced behavior exists.

Without a clear contract, sandbox claims can drift into "policy by convention" and fake-success signals that do not prove confinement.

## Goals

- Define a deterministic userspace sandbox v1 contract around namespace confinement, CapFd rights, and manifest permission bootstrap.
- Keep boundary honesty explicit: userspace confinement with controlled capability distribution, not kernel syscall containment.
- Require proof shapes that test intended behavior (Soll) rather than implementation internals (Ist mirroring).

## Non-Goals

- Kernel-level namespace enforcement or raw-syscall containment.
- Full POSIX filesystem semantics.
- Broad production hardening breadth outside TASK-0039 scope (`TASK-0043`, `TASK-0189`).

## Constraints / invariants (hard requirements)

- **Determinism**: stable reject reasons and marker strings; no variable marker payloads.
- **No fake success**: readiness/ok markers only after real enforced behavior.
- **Bounded resources**: caps for namespace mounts, path lengths, CapFd tables, and replay windows.
- **Security floor**:
  - deny-by-default path access outside namespace bounds,
  - CapFd authenticity and replay resistance,
  - no direct fs-service caps to app subjects.
- **Rust API discipline**:
  - use newtypes for identity-bearing handles (`SubjectId`, `NamespaceId`, `CapFdId`) where boundaries cross components,
  - decision-bearing results use `#[must_use]` where sensible,
  - ownership is explicit for capability transfer/revocation state,
  - `Send`/`Sync` guarantees are explicit and reviewed (no blanket unsafe shortcuts).

## Proposed design

### Contract / interface (normative)

- Namespace contract: every sandboxed subject resolves paths through a per-subject namespace map, with canonicalized traversal rejection.
- CapFd contract: rights + subject binding + bounded validity window + authenticity tag; validation is fail-closed.
- Manifest contract: permissions are deterministic and integrity-protected in signed bundle metadata.
- Bootstrap authority contract: `execd/init` remains the single authority for capability distribution at spawn time.

### Phases / milestones (contract-level)

- **Phase 0**: Host-first namespace/CapFd/manifest contract + reject proofs.
- **Phase 1**: OS-gated marker ladder proving enforcement, not just wiring.
- **Phase 2**: Explicit production-grade boundary handoff to `TASK-0043` and `TASK-0189` with no scope leakage.

## Security considerations

- **Threat model**:
  - path traversal / namespace escape,
  - CapFd forgery and replay,
  - manifest spoofing,
  - capability-distribution drift.
- **Mitigations**:
  - canonical path normalization + prefix-bound checks,
  - authenticity-protected CapFd with bounded freshness semantics,
  - signed manifest permissions,
  - spawn-time capability allowlist authority.
- **DON'T DO**:
  - do not grant direct filesystem service caps to app subjects,
  - do not accept unsigned/tampered permission manifests,
  - do not claim kernel-level sandbox protection in v1 scope.
- **Open risks**:
  - kernel-unchanged boundary remains userspace confinement only; this must stay explicit in docs/markers/release claims.

## Failure model (normative)

- Unauthorized path, traversal, forged/replayed CapFd, and malformed permission data are deterministic rejects.
- No silent fallback to broader permissions or unscoped path access.
- If integrity/auth checks fail, request handling is fail-closed.

## Production-grade gate mapping (TRACK alignment)

This RFC maps to **Gate B (Security, Policy & Identity)** in
`tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.

Inside task/RFC scope:

- userspace deny-by-default confinement contract,
- deterministic reject-path evidence for critical security failures,
- authority discipline for capability distribution.

Follow-up production-grade breadth (out-of-scope here):

- `TASK-0043` (quota/egress/ABI-audit hardening breadth),
- `TASK-0189` (profile distribution + policy plumbing hardening).

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p vfsd -- --nocapture && cargo test -p nexus-vfs -- --nocapture && cargo test -p execd --lib test_reject_direct_fs_cap_bypass_at_spawn_boundary -- --nocapture
```

Host proof must include behavior-oriented rejects (Soll):

- `test_reject_path_traversal`
- `test_reject_forged_capfd`
- `test_reject_replayed_capfd`
- `test_reject_unauthorized_namespace_path`
- `test_reject_capfd_rights_mismatch`
- `test_reject_direct_fs_cap_bypass_at_spawn_boundary`
- `test_reject_forged_capfd_service_path`

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers (if applicable)

- `vfsd: namespace ready`
- `vfsd: capfd grant ok`
- `vfsd: access denied`
- `SELFTEST: sandbox deny ok`
- `SELFTEST: capfd read ok`

## Alternatives considered

- Rely on path-prefix checks without CapFd authenticity (rejected: forge/replay risk too high).
- Claim kernel-level sandbox guarantees in v1 docs (rejected: dishonest with kernel untouched scope).

## Open questions

- Which service owns CapFd signing material in v1 (`keystored` vs dedicated broker key custody), while preserving single-authority and bounded-key-usage contracts?

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: host-first namespace/CapFd/manifest contract with deterministic reject proofs — proof: `cd /home/jenning/open-nexus-OS && cargo test -p vfsd -- --nocapture && cargo test -p nexus-vfs -- --nocapture && cargo test -p execd --lib test_reject_direct_fs_cap_bypass_at_spawn_boundary -- --nocapture`
- [x] **Phase 1**: OS-gated marker ladder proves real enforcement — proof: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- [x] **Phase 2**: production-grade boundaries to `TASK-0043` and `TASK-0189` remain explicit and synchronized — proof: `cd /home/jenning/open-nexus-OS && rg "TASK-0043|TASK-0189" tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md`
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
