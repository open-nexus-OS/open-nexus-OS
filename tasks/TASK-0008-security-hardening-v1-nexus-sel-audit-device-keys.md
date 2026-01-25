---
title: TASK-0008 Security hardening v1 (OS): policy engine (nexus-sel), audit trail, policy-gated operations (no device key entropy)
status: Done
owner: @runtime
created: 2025-12-22
completed: 2026-01-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC (seed contract): docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md
  - RFC (capability model): docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Existing policy baseline: recipes/policy/base.toml
follow-up-tasks:
  - TASK-0008B: Device identity keys v1 (OS): virtio-rng entropy + rngd + keystored keygen (enables real device keys)
  - TASK-0009: Persistence v1 (enables persistent device keys; not a blocker for v1)
  - TASK-0017: DSoftBus remote statefs R/W (policy + audit semantics)
  - TASK-0019: Security v2 userland ABI/syscall filters (reuse policy authority)
  - TASK-0025: Statefs write-path hardening (policy-gated ops + audit)
  - TASK-0027: Statefs encryption-at-rest (device keys + entropy)
  - TASK-0028: ABI filters v2 (arg match learn/enforce, policy authority)
  - TASK-0029: Supply chain v1 (SBOM/repro/sign/policy, device identity)
  - TASK-0039: Sandboxing v1 (VFS namespaces/capfd/manifest, policy authority)
  - TASK-0043: Security v2 (sandbox quotas/egress/ABI/audit)
  - TASK-0053: Security v3 (signed recovery actions, policy-gated signing)
  - TASK-0108: UI keymintd/keystore/keychain baseline (device keys)
  - TASK-0136: Policy v1 (capability matrix/foreground adapters/audit)
  - TASK-0137: Security & Privacy Settings UI (audit viewer + approvals)
  - TASK-0159: Identity/Keystore v1.1 (lifecycle/rotation, host-first)
  - TASK-0160: Identity/Keystore v1.1 OS (attestd/trust unification/selftests)
---

## Context

The OS already follows a seL4/Fuchsia-style security posture for bring-up:

- kernel enforces capability rights (RFC-0005),
- userland services must not trust requester identity inside payload bytes; use channel-bound identity (`sender_service_id`),
- policy decisions live in `policyd` (today: bring-up byte-frame protocol with deterministic allow/deny).

Security hardening v1 aims to:

- move from “bring-up allow/deny stubs” to a small, enforceable policy engine,
- produce an auditable, queryable trail of decisions,
- harden keystored request parsing and policy-gated operations (without requiring OS entropy in v1),
- harden bundle verification / exec authorization decisions without duplicating authority logic.

Kernel remains unchanged.

## Goal

In QEMU, prove:

- `policyd` evaluates policy based on a deterministic ruleset and emits audit events for allow/deny.
- Sensitive operations (bundle install, exec authorization, keystore signing) are deny-by-default without required capabilities/gates.
- `keystored` enforces policy-gated operations (deny-by-default) with deterministic failure modes.

## Non-Goals

- SELinux clone (labels, MLS, TE complexity). We keep a small “bring-up policy DSL”.
- Kernel policy enforcement changes (no new syscalls / kernel hooks).
- Full persistence backend (statefs/virtio-blk). We can stage the interface, but persistence itself may be deferred.

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **Channel-bound identity**: policy decisions must bind to `sender_service_id` (no trusting subject strings in payloads).
- **Single authority**: avoid duplicating policy logic in multiple services; `policyd` is the decision service.
- **Determinism**: decisions and markers stable; bounded parsing; no unbounded allocations from untrusted inputs.
- **No fake success**: audit markers/logs only emitted when decisions actually occurred.
- **Rust hygiene**: no new `unwrap/expect` in OS daemons; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (blocking / must decide now)**:
  - **Audit sink availability**: audit logs “via logd” depends on TASK-0006. If logd is not implemented yet,
    audit must fall back to deterministic UART markers (explicit) and later be switched to logd sink.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - **Policy file location drift**: repo already has `recipes/policy/base.toml`. Introducing a parallel `recipes/sel/*.toml`
    risks duplicating the source of truth. Prefer evolving the existing policy recipe structure (or explicitly migrating).
  - **Subject naming**: using `subject: &str` in protocols is fragile; prefer `sender_service_id` + an optional display name.
  - **Persistence reality**: device keys can only be “persistent” once `/state` is truly durable (TASK-0009). Until then, device-key behavior must not claim persistence.
- **GREEN (confirmed assumptions)**:
  - `policyd` already enforces identity binding for ROUTE/EXEC checks and has an init-lite proxy exception.
  - `keystored` os-lite shim already scopes keys by `sender_service_id` and enforces size bounds.

## Decisions (v1) — to prevent drift

- **Policy source of truth**: ✅ Evolve `recipes/policy/base.toml` (and `recipes/policy/` generally). Do **not** introduce a parallel `recipes/sel/*` tree.
- **Subject identity**: ✅ `sender_service_id` is the canonical identity for decisions and auditing. Any subject/app/service “name” is display/authoring-only and must never grant authority.

## Security considerations

### Threat model

- **Policy bypass**: Attacker finds path to sensitive operation that skips `policyd` check
- **Privilege escalation**: Service obtains capabilities beyond its policy allowance
- **Identity spoofing**: Attacker forges `service_id` to impersonate another service
- **Key extraction**: Attacker extracts device private keys from keystored
- **Audit evasion**: Attacker performs sensitive operations without audit trail
- **Policy injection**: Attacker modifies policy rules to grant unauthorized access
- **Side-channel attacks**: Timing or error message differences leak policy decisions

### Security invariants (MUST hold)

- ALL sensitive operations MUST go through `policyd` (single authority, no bypass)
- Policy decisions MUST bind to `sender_service_id` from kernel IPC (unforgeable)
- Device private keys MUST NEVER leave keystored (sign operations return signatures, not keys)
- ALL policy allow/deny decisions MUST be audit-logged
- Policy rules MUST be immutable at runtime (loaded at boot from trusted source)
- Signing operations MUST be policy-gated (deny-by-default)
- Error messages MUST NOT leak policy configuration details

### DON'T DO

- DON'T trust subject identity from payload bytes (use kernel-provided `sender_service_id`)
- DON'T duplicate policy logic in multiple services (single authority: `policyd`)
- DON'T expose raw private key bytes via any keystored API
- DON'T allow runtime policy modification without reboot
- DON'T use deterministic/insecure device keys in production (bring-up only, labeled)
- DON'T skip audit logging for any policy decision

### Attack surface impact

- **Critical**: This task defines the core security enforcement layer
- **Policy engine is the trust anchor**: Bugs here compromise the entire system
- **Requires thorough security review**: All changes to policyd/keystored must be reviewed

### Mitigations

- Channel-bound identity via kernel IPC (`sender_service_id` unforgeable)
- Policy rules loaded from immutable `recipes/policy/base.toml` at boot
- Keystored performs signing internally; private keys never exposed
- All policy decisions logged to audit trail (UART or logd)
- Deny-by-default: operations without explicit policy allow are rejected
- Bounded input parsing: reject oversized/malformed policy queries

## Security proof

### Audit tests (negative cases)

- Command(s):
  - `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p policyd --no-default-features --features os-lite -- reject --nocapture`
  - `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p keystored --no-default-features --features os-lite -- reject --nocapture`
- Required tests:
  - `test_reject_forged_service_id` — payload identity ignored, kernel ID used
  - `test_reject_unpolicied_operation` — no policy rule → denied
  - `test_reject_key_extraction` — no API path returns raw private key
  - `test_audit_all_decisions` — every allow/deny produces audit record
  - `test_reject_oversized_policy_query` — bounded input enforced

### Hardening markers (QEMU)

UART markers must remain deterministic and must not leak policy configuration details.
Use stable “proof markers” for the smoke path, and put detailed audit records in logd when available.

- `SELFTEST: policy deny audit ok` — a deny decision occurred and a corresponding audit record was produced
- `SELFTEST: policy allow audit ok` — an allow decision occurred and a corresponding audit record was produced
- `SELFTEST: keystored sign denied ok` — policy-gated signing denies without the required capability/gate

### Fuzz coverage (recommended)

- `cargo +nightly fuzz run fuzz_policy_parser` — policy rule parsing
- `cargo +nightly fuzz run fuzz_keystored_request` — keystored request parsing

## Contract sources (single source of truth)

- **Policy check semantics (current)**: `source/services/policyd/src/os_lite.rs` (PO v1/v2/v3 byte frames)
- **Exec authorization path (current)**: `source/services/execd/src/os_lite.rs` (exec_check via init-lite proxy to policyd)
- **Baseline allowlist**: `recipes/policy/base.toml` (current “caps” concept)

## Stop conditions (Definition of Done)

### Proof (Host)

- Add deterministic unit tests for policy evaluation and decision semantics (wildcards, deny precedence, gate evaluation).
- Minimum proof commands (host-first, os-lite env on host):
  - `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p policyd --no-default-features --features os-lite -- --nocapture`
  - `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p keystored --no-default-features --features os-lite -- --nocapture`
  - `cargo test -p e2e_policy -- --nocapture`

### Proof (OS / QEMU)

- Canonical smoke:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- Faster triage for this task (RFC‑0014 Phase 2):
  - `RUN_PHASE=policy RUN_TIMEOUT=190s just test-os`
- Extend expected markers with stable proofs (no secrets, no variable subject/action strings):
  - `SELFTEST: policy deny audit ok`
  - `SELFTEST: policy allow audit ok`
  - `SELFTEST: keystored sign denied ok`

Notes:

- Postflight scripts (if added) must **only** delegate to canonical harness/tests; no `uart.log` greps as “truth”.

## Touched paths (allowlist)

- `source/services/policyd/` (wire in policy engine; emit audit events)
- `source/libs/nexus-sel/` (policy evaluation library; PROTECTED tree — requires explicit approval when implementing)
- `recipes/policy/` (extend schema to include domains/gates; avoid parallel recipe trees)
- `source/services/keystored/` (device key API + policy-gated signing; verification hardening)
- `source/services/bundlemgrd/` (policy-gated install/verify decisions via policyd; audit)
- `source/services/execd/` (keep policyd as authority; emit audit around decisions, but do not re-implement policy)
- `source/apps/selftest-client/` (deny/allow + device-key markers; optional logd query once TASK-0006 is done)
- `scripts/qemu-test.sh`
- `docs/security/`

## Plan (small PRs)

1. **Define “nexus-sel” policy model as a library used by policyd**
   - Keep policy evaluation in `policyd`; `nexus-sel` is a pure library (no IPC).
   - Prefer rules keyed by **service_id** and/or globbed service names only as a *display/authoring* layer.
   - Extend `recipes/policy/base.toml` schema rather than creating a parallel `recipes/sel`.

2. **Audit trail**
   - If TASK-0006 is available: emit structured audit records via `nexus-log` to logd.
   - Otherwise: emit deterministic UART audit markers (explicitly labeled) and add a follow-up to switch sinks.

3. **policyd hardening**
   - Replace the current hardcoded allow/deny rules with `nexus-sel` decisions.
   - Preserve channel-bound identity binding and the init-lite proxy exception as explicit policy, not ad-hoc.

4. **keystored hardening + device keys**
   - Add device identity public key API.
   - Enforce policy gate for signing operations (deny-by-default).
   - Keep size bounds and sender scoping; add negative tests/markers.

5. **bundlemgrd / execd integration**
   - bundlemgrd: before sensitive ops (install/verify), request policyd decision; emit audit.
   - execd: keep existing policyd exec-check path; add audit around allow/deny outcomes without duplicating policy logic.

6. **Selftest**
   - Prove a deny path and allow path with clear markers.
   - Prove device key API is live (and policy-gated signing where applicable).

## Follow-ups

Follow-up task tracking lives in the YAML header `follow-up-tasks:` to avoid drift.

## Acceptance criteria (behavioral)

- Policy decisions are based on channel-bound identity and an explicit ruleset (no hidden allowlists in random services).
- Audit events exist for allow/deny and are queryable (logd) or at least visible deterministically (UART) until logd is live.
- Device key API exists and is policy-gated; insecure bring-up choices (if any) are explicitly labeled.

## RFC seeds (for later, once green)

- Decisions made:
  - policy schema and subject identity model (`service_id` vs name)
  - audit sink and fallback behavior
  - device key entropy source and rotation model
