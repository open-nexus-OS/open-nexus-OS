# RFC-0015: Policy Authority & Audit Baseline v1

- Status: Complete
- Owners: @runtime
- Created: 2026-01-23
- Last Updated: 2026-01-25
- Links:
  - Tasks: `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md` (execution + proof)
  - Related RFCs: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (capability model foundation)
  - Related RFCs: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (audit sink via logd)

## Status at a Glance

- **Phase 0 (Policy evaluation library)**: ✅ Complete (host proof green)
- **Phase 1 (Audit emission baseline)**: ✅ Complete (QEMU proof green)
- **Phase 2 (Policy-gated operations)**: ✅ Complete (host + QEMU proof green)
- **Phase 3 (Selftest markers + proofs)**: ✅ Complete (`RUN_PHASE=policy` proof green)

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in TASK-0008.

- **This RFC owns**:
  - Policy evaluation model (capability-based, service-id bound)
  - Audit record contract (structure, emission, sink)
  - Policy-gated operation semantics (deny-by-default)
  - Single authority invariant (`policyd` is the decision service)

- **This RFC does NOT own**:
  - Device key entropy / generation (→ TASK-0008B, future RFC)
  - Persistent policy storage (→ future RFC after statefs)
  - SELinux-style labels, MLS, or TE complexity (explicitly out of scope)
  - Kernel changes (kernel remains untouched)

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC links to TASK-0008 which implements and proves each phase.

## Context

The OS follows a seL4/Fuchsia-style security posture:

- Kernel enforces capability rights (RFC-0005)
- Userland services use channel-bound identity (`sender_service_id`)
- Policy decisions live in `policyd` (currently: bring-up hardcoded allow/deny)

**Problem**: The current `policyd` implementation has hardcoded rules and no audit trail.
Sensitive operations lack deny-by-default enforcement and audit visibility.

**Why now**: Security hardening v1 is required before downstream tasks (device keys, distributed
operations, sandboxing) can build on a stable policy foundation.

## Goals

- Move from hardcoded allow/deny stubs to a small, enforceable policy engine
- Produce an auditable, queryable trail of all policy decisions
- Enforce deny-by-default for sensitive operations (signing, bundle install, exec)
- Bind all policy decisions to `sender_service_id` (unforgeable kernel identity)

## Non-Goals

- SELinux clone (labels, MLS, TE complexity) — keep a small "bring-up policy DSL"
- Kernel policy enforcement changes — no new syscalls or kernel hooks
- Full persistence backend — interface may be staged, persistence is TASK-0009
- Device key entropy/generation — explicitly TASK-0008B

## Constraints / invariants (hard requirements)

- **Kernel untouched**: No kernel modifications for v1.
- **Channel-bound identity**: Policy decisions MUST bind to `sender_service_id` from kernel IPC, never trusting subject strings in payloads.
- **Single authority**: `policyd` is the sole decision service. No policy logic duplication in other services.
- **Determinism**: Decisions and markers are stable; bounded parsing; no unbounded allocations from untrusted inputs.
- **No fake success**: Audit markers/logs only emitted when decisions actually occurred.
- **Rust hygiene**: No new `unwrap`/`expect` in OS daemons; no blanket `allow(dead_code)`.
- **Stubs policy**: Any stub must be explicitly labeled, non-authoritative, and must not claim success.

## Proposed design

### Contract / interface (normative)

#### Policy Evaluation Model

Policy is capability-based with service-id binding:

```text
service_id (u64) + action (str) + target (optional str) → Decision (Allow | Deny)
```

Policy rules are loaded from `recipes/policy/base.toml` at boot. The schema:

```toml
[allow]
<service_name> = ["<capability>", ...]

# Example:
keystored = ["crypto.sign", "crypto.verify", "ipc.core"]
```

**Identity binding**: `service_id` is derived from the kernel-provided `sender_service_id` on IPC.
Requester name in payloads is display-only and MUST NOT grant authority.

**Deny-by-default**: Operations without explicit policy allow are rejected.

#### Audit Record Contract

Every policy decision emits an audit record with the following structure:

| Field | Type | Description |
|-------|------|-------------|
| `timestamp_ns` | u64 | Nanoseconds since boot |
| `subject_id` | u64 | `sender_service_id` of requester |
| `action` | str (bounded) | Operation type (e.g., "route", "exec", "sign") |
| `target` | str (bounded, optional) | Target service/resource |
| `decision` | u8 | 0 = Allow, 1 = Deny |
| `reason` | u8 | Reason code (0 = policy, 1 = identity mismatch, 2 = malformed) |

**Sink priority**:
1. logd (RFC-0011) — structured, queryable
2. UART fallback — deterministic markers only, no secrets

**Bounded fields**: `action` ≤ 32 bytes, `target` ≤ 64 bytes.

#### Policy-Gated Operations

The following operations are deny-by-default and require explicit capability:

| Operation | Required Capability | Service |
|-----------|---------------------|---------|
| Sign (Ed25519) | `crypto.sign` | keystored |
| Bundle install | `fs.verify` | bundlemgrd |
| Process exec | `proc.spawn` | execd |
| Route request | `ipc.core` | policyd (via init-lite proxy) |

### Phases / milestones (contract-level)

- **Phase 0 (Policy evaluation library)**: `nexus-sel` supports service-id based lookups and deny-precedence. `policyd` uses library instead of hardcoded logic.
- **Phase 1 (Audit emission baseline)**: Every allow/deny decision emits an audit record via logd. Fallback to UART markers if logd unavailable.
- **Phase 2 (Policy-gated operations)**: keystored `OP_SIGN` is policy-gated (deny-by-default). bundlemgrd/execd route sensitive ops through policyd.
- **Phase 3 (Selftest markers + proofs)**: QEMU markers prove audit emission and policy-gated denial.

## Security considerations

### Threat model

| Threat | Description |
|--------|-------------|
| Policy bypass | Attacker finds path to sensitive operation that skips `policyd` check |
| Privilege escalation | Service obtains capabilities beyond its policy allowance |
| Identity spoofing | Attacker forges `service_id` to impersonate another service |
| Key extraction | Attacker extracts device private keys from keystored |
| Audit evasion | Attacker performs sensitive operations without audit trail |
| Policy injection | Attacker modifies policy rules to grant unauthorized access |
| Side-channel | Timing or error message differences leak policy decisions |

### Security invariants (MUST hold)

1. ALL sensitive operations MUST go through `policyd` (single authority, no bypass)
2. Policy decisions MUST bind to `sender_service_id` from kernel IPC (unforgeable)
3. Device private keys MUST NEVER leave keystored (sign operations return signatures, not keys)
4. ALL policy allow/deny decisions MUST be audit-logged
5. Policy rules MUST be immutable at runtime (loaded at boot from trusted source)
6. Signing operations MUST be policy-gated (deny-by-default)
7. Error messages MUST NOT leak policy configuration details

### DON'T DO

- DON'T trust subject identity from payload bytes (use kernel-provided `sender_service_id`)
- DON'T duplicate policy logic in multiple services (single authority: `policyd`)
- DON'T expose raw private key bytes via any keystored API
- DON'T allow runtime policy modification without reboot
- DON'T use deterministic/insecure device keys in production (bring-up only, labeled)
- DON'T skip audit logging for any policy decision
- DON'T leak secrets or policy details in UART/log output

### Mitigations

- Channel-bound identity via kernel IPC (`sender_service_id` unforgeable)
- Policy rules loaded from immutable `recipes/policy/base.toml` at boot
- Keystored performs signing internally; private keys never exposed
- All policy decisions logged to audit trail (logd or UART)
- Deny-by-default: operations without explicit policy allow are rejected
- Bounded input parsing: reject oversized/malformed policy queries

## Failure model (normative)

| Condition | Behavior |
|-----------|----------|
| Policy lookup miss | Deny (deny-by-default) |
| Malformed request | Deny with `STATUS_MALFORMED`, audit logged |
| Identity mismatch | Deny with `STATUS_DENY`, audit logged |
| logd unavailable | Fall back to UART audit markers |
| Oversized input | Reject before parsing, `STATUS_TOO_LARGE` |

- "No silent fallback": If a fallback exists (e.g., UART when logd unavailable), it must be explicit and the fallback decision is still audited.

## Proof / validation strategy (required)

### Proof (Host)

```bash
# Policy evaluation tests (os-lite host mode)
RUSTFLAGS='--cfg nexus_env="os"' cargo test -p policyd --no-default-features --features os-lite -- --nocapture

# Keystored policy-gate tests
RUSTFLAGS='--cfg nexus_env="os"' cargo test -p keystored --no-default-features --features os-lite -- --nocapture

# E2E policy tests
cargo test -p e2e_policy -- --nocapture
```

### Proof (OS/QEMU)

```bash
# Canonical smoke
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os

# Triage for policy phase (RFC-0014 Phase 2)
RUN_PHASE=policy RUN_TIMEOUT=190s just test-os
```

### Deterministic markers (required for TASK-0008)

| Marker | Proves |
|--------|--------|
| `SELFTEST: policy deny audit ok` | A deny decision occurred AND audit record was emitted |
| `SELFTEST: policy allow audit ok` | An allow decision occurred AND audit record was emitted |
| `SELFTEST: keystored sign denied ok` | Policy-gated signing denied without required capability |

### Required negative tests

| Test | Proves |
|------|--------|
| `test_reject_forged_service_id` | Payload identity ignored, kernel ID used |
| `test_reject_unpolicied_operation` | No policy rule → denied |
| `test_reject_key_extraction` | No API path returns raw private key |
| `test_audit_all_decisions` | Every allow/deny produces audit record |
| `test_reject_oversized_policy_query` | Bounded input enforced |

## Alternatives considered

- **SELinux-style labels**: Rejected — too complex for bring-up; our policy model is simpler and sufficient for v1.
- **In-kernel policy enforcement**: Rejected — violates "kernel untouched" constraint; userspace enforcement via policyd is sufficient.
- **Per-service inline policy checks**: Rejected — violates single authority invariant; would lead to drift and bypass risks.

## Open questions

- **Policy schema evolution**: How do we version the `recipes/policy/base.toml` schema for future extensions? (Owner: @runtime, decision: before v1.1)
- **Audit retention**: How long do audit records persist in logd? (Depends on TASK-0006 journal semantics)

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- ✅ Scope boundaries are explicit; cross-RFC ownership is linked.
- ✅ Determinism + bounded resources are specified in Constraints section.
- ✅ Security invariants are stated (threat model, mitigations, DON'T DO).
- ✅ Proof strategy is concrete (not "we will test this later").
- ⬜ If claiming stability: define ABI/on-wire format + versioning strategy. (Audit record format is v1, versioning TBD)
- ✅ Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: Policy evaluation library hook-up — proof: `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p policyd --no-default-features --features os-lite -- --nocapture`
- [x] **Phase 1**: Audit emission baseline — proof: `SELFTEST: policy deny audit ok` + `SELFTEST: policy allow audit ok` in QEMU
- [x] **Phase 2**: Policy-gated operations — proof: `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p keystored --no-default-features --features os-lite -- reject --nocapture` and QEMU markers (`SELFTEST: keystored sign denied ok`)
- [x] **Phase 3**: Selftest markers pass — proof: `RUN_PHASE=policy RUN_TIMEOUT=190s just test-os`
- [x] Host proof: `cargo test -p e2e_policy -- --nocapture`
- [x] Task linked with stop conditions + proof commands.
- [x] QEMU markers appear in `scripts/qemu-test.sh` and pass (at least through routing + policy phases).
- [x] Security-relevant negative tests exist (`test_reject_*`).
