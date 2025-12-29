---
title: TASK-0019 Security v2 (OS): userland ABI syscall guardrails (filter chain + profiles + audit)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (policy model): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want a “seccomp-like” syscall policy, but **kernel remains unchanged**. The best achievable v2 in
that constraint is a **userland guardrail**:

- centralize all syscalls behind `nexus-abi` wrappers,
- apply deterministic allow/deny checks before issuing an ecall,
- emit audit events (deny-by-default profiles) and stable errors.

This complements (not replaces) capability enforcement + policyd decisions.

## Goal

In QEMU, prove:

- OS services fetch/apply a syscall profile at startup,
- denied calls fail deterministically with stable errors,
- deny decisions produce audit events via logd,
- selftest demonstrates allow+deny for statefs and a network bind attempt.

## Non-Goals

- A true sandbox against malicious code issuing raw `ecall` instructions.
- Kernel-level syscall filters (future v3).

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **Single syscall entry path (for compliant code)**: all OS components we ship must use `nexus-abi` wrappers, not ad-hoc inline asm.
- **Determinism**: profiles are deterministic; matching bounded; errors stable; tests bounded.
- **Performance**: do not “audit everything always”. Audit denies and optionally sampled/aggregated allows.

## Red flags / decision points

- **RED (must document explicitly)**:
  - **Not a security boundary**: without kernel enforcement, a process can bypass userland filters by executing `ecall` directly (inline asm).
    This v2 is a **defense-in-depth guardrail for compliant binaries** and a strong hygiene tool, not a sandbox.
    If we need a true sandbox, we must add a kernel task later (“seccomp v3”).
- **YELLOW (risky / needs careful design)**:
  - **Profile source of truth**: avoid introducing a parallel policy tree (`recipes/abi/*`) if we already have `recipes/policy/*`.
    Prefer extending the existing policy model (nexus-sel/policyd) to also serve syscall profiles, or document a clean migration.
  - **Subject identity**: do not trust payload strings. Use kernel-derived identity:
    - `BootstrapInfo.service_id` for “who am I” (available without kernel changes),
    - optional display name from the RO meta name page for logging only.
- **GREEN (confirmed assumptions)**:
  - Kernel already publishes `BootstrapInfo` with stable `service_id` and name pointer/len (provenance-safe).

## Contract sources (single source of truth)

- Identity token: `source/kernel/neuron/src/bootstrap.rs` (`BootstrapInfo.service_id`)
- Existing policy baseline: `recipes/policy/base.toml`
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host)

- Deterministic tests for:
  - profile parsing + matching precedence,
  - stable error mapping,
  - audit event formatting to a mock sink.

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend expected markers with:
    - `abi-filterd: ready` (or `policyd: abi profiles ready` if we choose policyd as the server)
    - `SELFTEST: abi filter deny ok`
    - `SELFTEST: abi filter allow ok`
    - `SELFTEST: abi netbind deny ok`

## Touched paths (allowlist)

- `source/libs/nexus-abi/` (central dispatcher + filter chain; route wrappers through it)
- `source/services/policyd/` (preferred: serve profiles; alternatively introduce `abi-filterd`)
- `recipes/policy/` (extend schema for syscall profile rules)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `docs/security/abi-filters.md`

## Plan (small PRs)

1. **Centralize syscalls in `nexus-abi`**
   - Provide a `Syscall` enum and a single `Abi::call()` path used by all wrappers.
   - Filters run before the ecall; on deny return stable `AbiError`/`errno`.

2. **Profile format + matcher**
   - TOML or binary profile, but bounded and deterministic.
   - Default deny; first-match-wins rules for:
     - syscall kind
     - path prefix (statefs)
     - ports (net bind)
     - size bounds.

3. **Profile distribution**
   - Preferred: `policyd` serves syscall profiles (single policy authority) and emits audit.
   - Alternative: `abi-filterd` as a small loader service (only if policyd coupling is undesirable).

4. **Audit**
   - Denies emit audit events via logd (TASK-0006).
   - Allows are sampled or counted, not line-by-line spam.

5. **Selftest**
   - Denied: `statefs.put("/state/forbidden", ...)` (or another clearly forbidden op).
   - Allowed: `statefs.put("/state/app/selftest/token", ...)` (or allowed prefix).
   - Denied: net bind without permission.
   - Emit markers listed in Stop conditions.

## Acceptance criteria (behavioral)

- Profiles are deny-by-default, deterministic, and bounded.
- Denied calls return stable errors and are audited.
- Documentation clearly states this is not a sandbox without kernel enforcement.

## Future direction (not in v2): kernel-level seccomp (v3)

If/when we need true enforcement against malicious code:

- add a kernel syscall filter hook keyed by `service_id` and/or a domain token,
- keep the userspace profiles as the authoring format and compile them to a kernel-checked form,
- prove it via negative tests (raw ecall denied even if bypassing `nexus-abi`).

This “true enforcement” work is tracked as: `TASK-0188`.
