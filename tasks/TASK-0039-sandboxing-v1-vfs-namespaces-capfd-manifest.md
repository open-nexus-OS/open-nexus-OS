---
title: TASK-0039 Sandboxing v1 (userspace): per-subject VFS namespaces + CapFd rights + manifest permissions (host-first, OS-gated)
status: In Review
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0006
  - TASK-0008
  - TASK-0019
  - TASK-0028
follow-up-tasks:
  - TASK-0043
  - TASK-0189
links:
  - Vision: docs/agents/VISION.md
  - Contract seed (this task): docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md
  - Playbook: docs/agents/PLAYBOOK.md
  - IPC/caps model: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - VFS today: source/services/vfsd/src/os_lite.rs
  - Depends-on (policy authority): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Depends-on (ABI guardrails): tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md
  - Depends-on (ABI arg matching, optional): tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want app/service sandboxing without kernel changes by combining:

- per-subject VFS **namespace views** (what paths exist),
- rights-scoped **file descriptor capabilities** (CapFd) instead of raw paths for sensitive operations,
- declarative **app manifests** (permissions) used at spawn time,
- enforcement in `execd` + `vfsd` + `nexus-abi` wrappers, audited.

Repo reality today:

- OS-lite `vfsd` only supports **read-only `pkg:/`** and returns file bytes (no state/tmp, no namespaces).
- Many higher-level components referenced by the prompt (updated/statefs, strong `/state` semantics) are still planned tasks.

So Sandboxing v1 must be **host-first** and **OS-gated** with honest limits.

## Goal

Deliver a userspace sandboxing v1 system where:

- `execd` (or a small broker) constructs a per-subject namespace from a manifest,
- `vfsd` enforces namespace confinement and issues rights-scoped CapFds,
- sensitive operations can be expressed as “CapFd + rights” (deny-by-default on paths),
- host tests prove confinement, rights checks, and replay protection deterministically,
- OS markers are added only once the OS bring-up path can actually enforce these constraints.

## Non-Goals

- A kernel-enforced sandbox against malicious code executing raw syscalls (`ecall`) or bypassing wrappers.
- Full POSIX filesystem semantics.
- Shipping a full “app runtime” ecosystem (this is a security foundation).

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Bounded parsing and bounded tables:
  - namespace spec size caps,
  - max number of mounts per namespace,
  - max CapFds per subject and global LRU.
- Determinism: stable error mapping and stable markers (no timestamps in marker lines).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (security boundary honesty)**:
  - With kernel unchanged, this is a **userspace confinement mechanism** for processes that only hold the intended capabilities.
    If a process can directly invoke privileged syscalls or holds direct caps to `packagefsd/statefsd`, it can bypass `vfsd`.
  - Therefore the real security boundary depends on **capability distribution at spawn**:
    - “apps” must not receive direct caps to filesystem services; they should only get `vfsd` (and optionally a broker).
    - `execd/init` must be the single authority for initial cap grants.
- **RESOLVED (manifest source of truth)**:
  - Packaging baseline is aligned on `manifest.nxb`. App permission metadata still must remain in a deterministic,
    signed location and must not reintroduce format fallback drift.
- **YELLOW (CapFd authenticity)**:
  - CapFds must be non-forgeable and replay-resistant (nonce + MAC) using a key managed by a trusted service (`keystored` or a dedicated broker key).

## Security considerations

### Threat model

- **Sandbox escape**: Malicious app bypasses VFS namespace confinement
- **CapFd forgery**: Attacker crafts fake CapFd to access unauthorized paths
- **Path traversal**: Attacker uses `../` to escape namespace bounds
- **Capability bypass**: App holds direct caps to filesystem services, bypassing `vfsd`
- **Manifest spoofing**: Attacker modifies app manifest to grant unauthorized permissions
- **CapFd replay**: Attacker reuses old CapFd after revocation

### Security invariants (MUST hold)

- Namespace confinement MUST be enforced by `vfsd` (apps cannot escape bounds)
- CapFds MUST be unforgeable (MAC-protected with nonce)
- Path traversal (`../`) MUST be rejected deterministically
- Apps MUST NOT receive direct caps to `packagefsd`/`statefsd` (only `vfsd`)
- Manifest permissions MUST be signed and integrity-protected
- CapFd revocation MUST invalidate all copies (nonce/expiry enforced)

### DON'T DO

- DON'T grant direct filesystem service caps to apps (only `vfsd`)
- DON'T allow path traversal outside namespace bounds
- DON'T accept unsigned or tampered app manifests
- DON'T trust CapFd without MAC verification
- DON'T skip namespace checks for "trusted" apps
- DON'T claim this is kernel-enforced (it's userspace confinement)

### Attack surface impact

- **NOT a kernel-enforced sandbox**: Apps with raw syscall access can bypass `vfsd`
- **Userspace confinement only**: Effective for compliant apps, not malicious code
- **Capability distribution is critical**: Apps must not receive direct filesystem caps
- **True enforcement**: Requires kernel-level namespace enforcement (future task)

### Real Security Boundary

**v1 Reality**: Userspace confinement for processes that:

- Only hold `vfsd` capability (not direct `packagefsd`/`statefsd` caps)
- Use `nexus-vfs` client library (not raw syscalls)
- Are spawned by `execd` with controlled capability grants

**NOT protected against**:

- Malicious code executing raw `ecall` syscalls
- Apps that receive direct filesystem service capabilities
- Kernel-level exploits

**Future (kernel-enforced namespaces)**:

- Kernel tracks per-process namespace ID
- Syscalls validate namespace bounds at kernel level
- Capability distribution enforced by kernel (not userspace)

### Mitigations

- Namespace confinement enforced by `vfsd` (path prefix checks)
- CapFds are MAC-protected (HMAC with service-local key)
- Path traversal rejected via normalization + prefix validation
- `execd` controls initial capability grants (deny-by-default)
- App manifests signed and embedded in bundle (integrity-protected)
- CapFd nonces prevent replay attacks

### Security proof

#### Audit tests (negative cases)

- Command(s):
  - `cargo test -p vfsd -- --nocapture`
  - `cargo test -p nexus-vfs -- --nocapture`
  - `cargo test -p execd --lib test_reject_direct_fs_cap_bypass_at_spawn_boundary -- --nocapture`
- Required tests:
  - `test_reject_path_traversal` — `../` escape → denied
  - `test_reject_forged_capfd` — tampered CapFd → rejected
  - `test_reject_forged_capfd_service_path` — forged token rejected through dispatcher service path
  - `test_reject_replayed_capfd` — nonce replay → rejected
  - `test_reject_unauthorized_namespace_path` — access outside namespace → denied
  - `test_reject_capfd_rights_mismatch` — rights escalation → denied
  - `test_reject_direct_fs_cap_bypass_at_spawn_boundary` — direct fs service cap set rejected

#### Hardening markers (QEMU)

- `vfsd: namespace ready` — namespace created
- `vfsd: capfd grant ok` — CapFd issued
- `vfsd: access denied` — confinement enforced
- `SELFTEST: sandbox deny ok` — escape attempt blocked
- `SELFTEST: capfd read ok` — legitimate access works

## Production-grade gate mapping (TRACK alignment)

This task is part of **Gate B (Security, Policy & Identity)** in
`tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.

Inside `TASK-0039` scope:

- deny-by-default userspace namespace confinement contract,
- deterministic reject-path behavior for traversal/forgery/replay/unauthorized access,
- explicit capability-distribution authority at spawn (`execd/init` -> app).

Follow-up production-grade breadth remains explicit and unabsorbed:

- `TASK-0043` for quotas/egress/ABI-audit hardening breadth,
- `TASK-0189` for sandbox profile distribution and policy plumbing hardening.

## Contract sources (single source of truth)

- Capability identity binding: `sender_service_id` enforcement patterns (RFC-0005 + existing policyd/os-lite logic).
- VFS scheme contract: `pkg:/` today via `packagefsd`.
- Audit sink contract: TASK-0006 (logd) once implemented.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic host tests (`tests/sandbox_host/` or equivalent crate-host test modules):

- namespace confinement:
  - `pkg:/...` read allowed
  - writes denied
  - traversal escapes (`..`) denied
- CapFd rights:
  - grant READ → read ok, write denied
  - grant WRITE → write ok within allowed prefixes
- CapFd integrity:
  - tamper serialized CapFd → deterministically rejected (`EINTEGRITY`)
- broker minting:
  - request CapFd for allowed path ok
  - forbidden path denied with stable reason.

### Proof (OS / QEMU) — gated

Once OS has:

- `execd` in the loop for spawning apps (and controlling cap grants),
- a `vfsd` that supports namespaces beyond `pkg:/`,
- an audit sink or deterministic markers,

then extend `scripts/qemu-test.sh` with:

- `vfsd: namespace ready`
- `vfsd: capfd grant ok`
- `vfsd: access denied`
- `SELFTEST: sandbox deny ok`
- `SELFTEST: capfd read ok`

Notes:

- Postflight scripts must delegate to canonical tests/harness; no independent “log greps = success”.

## Touched paths (allowlist)

- `source/services/vfsd/` (namespace views + CapFd issuance; host first, OS later)
- `source/services/execd/` (sandbox bootstrap: build NamespaceSpec + initial CapFds)
- `source/services/keystored/` (optional: CapFd MAC key service; or broker-local key)
- `tools/` (manifest schema/tooling for app permissions; packaging embedding)
- `tests/`
- `source/apps/selftest-client/` (OS-gated)
- `docs/security/sandboxing.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. **Define App Manifest v1 (permissions)**
   - Keep schema small: fs prefixes (state/tmp), pkg read, net connect/bind ranges (optional), ipc allowlist (optional).
   - Embed in bundle metadata in a deterministic location and ensure it is covered by signing (ties to supply-chain).

2. **Implement namespaces in vfsd (host-first)**
   - NamespaceSpec: list of mounts + prefix allowlists.
   - Scheme mapping:
     - `pkg:/` read-only (existing)
     - `tmp:/app/<id>/` (host-only tmp store for tests)
     - `state:/app/<id>/` (gated until statefs exists; can be simulated in host tests)

3. **CapFd design + issuance**
   - CapFd is opaque: includes id + rights + expiry/nonce + MAC.
   - vfsd/broker verifies MAC and rights subset.

4. **Spawn-time bootstrap (OS-gated)**
   - `execd` reads app manifest, constructs namespace, requests namespace + initial CapFds from vfsd/broker,
     passes them to the child via bootstrap message.
   - Ensure children do not get direct caps to underlying fs services.

5. **ABI filter integration (optional)**
   - Add v2 rules for “path prefix” and “rights” to keep deny-by-default at wrapper level.

6. **Docs**
   - Threat model: what this does and does not protect against under “kernel unchanged”.
