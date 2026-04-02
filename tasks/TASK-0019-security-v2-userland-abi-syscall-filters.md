---
title: TASK-0019 Security v2 (OS): userland ABI syscall guardrails (filter chain + profiles + audit)
status: Done
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md
  - RFC (kernel IPC/cap baseline): docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - RFC (policy authority): docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (policy model): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Depends-on (statefs contract for selftest paths): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing methodology: docs/testing/index.md
  - Testing contract: scripts/qemu-test.sh
follow-up-tasks:
  - TASK-0028: ABI filters v2 (`learn`/`enforce`, argument matchers, generator)
  - TASK-0188: kernel-level syscall enforcement boundary (seccomp v3)
---

## Context

We want a “seccomp-like” syscall policy, but **kernel remains unchanged**. The best achievable v2 in
that constraint is a **userland guardrail**:

- centralize syscall entry through `nexus-abi` wrappers (phased rollout to all shipped OS components),
- apply deterministic allow/deny checks before issuing an ecall,
- emit audit events (deny-by-default profiles) and stable errors.

This complements (not replaces) capability enforcement + policyd decisions.

## Goal

In QEMU, prove:

- OS services fetch/apply a syscall profile at startup,
- denied calls fail deterministically with stable errors,
- deny decisions produce audit events via logd,
- selftest demonstrates allow+deny for statefs and a network bind attempt.

## Status at a Glance (2026-03-27)

- ✅ Task status advanced to `Done` after green host/OS/QEMU proof closure.
- ✅ Phase A: bounded filter profile + matcher implemented in `nexus-abi` (`deny-by-default`, `first-match-wins`).
- ✅ Phase B/C: authenticated profile distribution implemented in `policyd` (`sender_service_id` authority + subject binding).
- ✅ Phase D/F: QEMU markers integrated and green (`abi-profile`, `abi-filter deny`, `SELFTEST: abi * ok`).
- ✅ Lifecycle boundary preserved: static startup apply only (no runtime learn/enforce/hot reload).

## Target-state alignment (post TASK-0018 closeout)

- `TASK-0018` (crashdump v1) is done; this task must stay focused on ABI syscall guardrails and
  must not absorb crashdump v2 follow-on scope.
- Required baseline contracts already exist:
  - `TASK-0006` (`logd` audit/event sink),
  - `TASK-0008` (policy authority + audit model),
  - `TASK-0009` (`/state` persistence semantics used by statefs allow/deny proofs).
- Queue alignment: `TASK-0019` is the next security slice before transport/statefs v2 follow-ons.
- Source-of-truth discipline: profile authority remains single-owner (`policyd`/policy model); avoid
  parallel unsynced profile trees.

## Non-Goals

- A true sandbox against malicious code issuing raw `ecall` instructions.
- Kernel-level syscall filters (future v3).

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **Single syscall entry path (for compliant code)**:
  - rollout is phased,
  - end-state is all shipped OS components on `nexus-abi` wrappers,
  - each phase must ship with explicit coverage/proof of migrated call sites.
- **Determinism**: profiles are deterministic; matching bounded; errors stable; tests bounded.
- **Performance**: do not “audit everything always”. Audit denies and optionally sampled/aggregated allows.

## Rollout phases (explicit execution shape)

1. **Phase A — filter chain base + bounded deny/audit path**
   - Introduce bounded matcher + deny/audit behavior in `nexus-abi` for a minimal syscall subset.
   - Prove deterministic host rejects before broad service rollout.
2. **Phase B — staged service rollout (toward all shipped OS components)**
   - Migrate selected critical services first (deterministic/low-risk set), then expand.
   - Track migrated wrappers/call sites explicitly; no implicit "all migrated" claim.
3. **Phase C — profile distribution (separate phase)**
   - Add authenticated profile delivery path (`sender_service_id`) with subject binding + bounded decode.
   - Keep profile lifecycle static for TASK-0019 (boot-time fetch/apply; no runtime mode switching).
4. **Phase D — QEMU proof and marker closure**
   - Lock marker contract and selftest proofs for deny/allow/netbind deny.
   - Close only when phased rollout + distribution proofs are green.

## Red flags / decision points

- **RED (must document explicitly)**:
  - **Not a security boundary**: without kernel enforcement, a process can bypass userland filters by executing `ecall` directly (inline asm).
    This v2 is a **defense-in-depth guardrail for compliant binaries** and a strong hygiene tool, not a sandbox.
    If we need a true sandbox, we must add a kernel task later (“seccomp v3”).
- **YELLOW (risky / needs careful design)**:
  - **Profile source of truth**: avoid introducing a parallel policy tree (`recipes/abi/*`) if we already have `recipes/policy/*`.
    ✅ Resolved in this slice: syscall profiles are served from `recipes/policy/*` through `policyd` (policyd-only).
  - **Authenticated profile distribution**: profile payloads must be accepted only from the policy authority
    (`sender_service_id`), with deterministic reject labels for unauthenticated/mismatched subjects.
  - **Subject identity**: do not trust payload strings. Use kernel-derived identity:
    - `BootstrapInfo.service_id` for “who am I” (available without kernel changes),
    - optional display name from the RO meta name page for logging only.
- **GREEN (confirmed assumptions)**:
  - Kernel already publishes `BootstrapInfo` with stable `service_id` and name pointer/len (provenance-safe).
  - Profile authority choice for TASK-0019 is locked to `policyd` (no `abi-filterd` path in this task).

## Security considerations

### Threat model

- **Bypass via raw ecall**: Malicious code executes `ecall` directly, bypassing `nexus-abi` wrappers
- **Profile tampering**: Attacker modifies syscall profile to grant unauthorized access
- **Audit evasion**: Sensitive operations performed without audit trail
- **DoS via audit flooding**: Service floods audit log with deny events
- **Policy injection**: Attacker injects fake profile rules

### Security invariants (MUST hold)

- ALL OS services we ship MUST use `nexus-abi` wrappers (single syscall entry path)
- Profiles MUST be deterministic and bounded (no unbounded parsing)
- Deny decisions MUST produce audit events
- Profiles MUST be deny-by-default
- Profile distribution MUST use authenticated channels (`sender_service_id`)
- Profile subject binding MUST use kernel-derived service identity, never payload-declared subject strings
- Profile/rule decoding MUST enforce bounded limits (rule count, path length, and argument payload sizes)

### DON'T DO

- DON'T claim this is a security boundary against malicious code
- DON'T skip audit logging for deny decisions
- DON'T accept unbounded profile sizes
- DON'T trust subject identity from payload bytes (use `sender_service_id`)
- DON'T allow runtime profile modification without reboot
- DON'T load or apply profiles received from unauthenticated senders

### Attack surface impact

- **NOT a security boundary**: Without kernel enforcement, this is a guardrail for compliant binaries only
- **Defense-in-depth**: Provides hygiene and audit trail for compliant code
- **True enforcement**: Requires kernel-level syscall filter (TASK-0188)

### Mitigations

- Single syscall entry path via `nexus-abi` wrappers (for compliant code)
- Deny-by-default profiles with explicit allow rules
- Audit trail for all deny decisions (via logd)
- Profile distribution uses authenticated `sender_service_id`
- Bounded profile parsing with size limits

### Security proof

#### Audit tests (negative cases)

- Command(s):
  - `cargo test -p nexus-abi -- reject --nocapture`
- Required tests:
  - `test_reject_unbounded_profile` — oversized profile → rejected
  - `test_reject_unauthenticated_profile_distribution` — non-authority sender → rejected
  - `test_reject_subject_spoofed_profile_identity` — payload subject mismatch → rejected
  - `test_reject_profile_rule_count_overflow` — excessive rules → rejected deterministically
- Additional deterministic hardening tests:
  - `test_reject_first_match_precedence_conflict_is_deterministic` — first-match-wins remains stable under conflicting rules
  - `test_reject_trailing_profile_bytes_as_malformed` — trailing garbage is fail-closed
  - `test_reject_statefs_put_oversized_payload_fail_closed` — oversize payload checks deny deterministically
  - `test_reject_typed_distribution_subject_mismatch` — typed identity wrappers preserve subject-binding reject semantics
  - `test_abi_profile_get_v2_malformed_frame_is_fail_closed` — malformed v2 fetch frames reject deterministically

#### Hardening markers (QEMU)

- `abi-profile: ready (server=policyd|abi-filterd)` — profile server path initialized deterministically
- `abi-filter: deny (subject=<svc> syscall=<op>)` — deny-by-default works
- `SELFTEST: abi filter deny ok` — deny path verified
- `SELFTEST: abi filter allow ok` — allow path verified
- `SELFTEST: abi netbind deny ok` — privileged net bind reject verified

## Contract sources (single source of truth)

- Identity token: `source/kernel/neuron/src/bootstrap.rs` (`BootstrapInfo.service_id`)
- Existing policy baseline: `recipes/policy/base.toml`
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host)

- Deterministic tests for:
  - profile parsing + matching precedence,
  - stable error mapping,
  - audit event formatting to a mock sink,
  - authenticated profile ingestion (`sender_service_id`) and subject-binding rejects.

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend expected markers with:
    - `abi-profile: ready (server=policyd|abi-filterd)`
    - `abi-filter: deny (subject=<svc> syscall=<op>)`
    - `SELFTEST: abi filter deny ok`
    - `SELFTEST: abi filter allow ok`
    - `SELFTEST: abi netbind deny ok`

### Latest evidence (2026-03-27)

- Host:
  - `cargo test -p nexus-abi -- reject --nocapture` ✅
  - required `test_reject_*` set green ✅
  - additional deterministic hardening tests green:
    - `test_reject_first_match_precedence_conflict_is_deterministic` ✅
    - `test_reject_trailing_profile_bytes_as_malformed` ✅
    - `test_reject_statefs_put_oversized_payload_fail_closed` ✅
    - `test_reject_typed_distribution_subject_mismatch` ✅
  - policyd host integration tests:
    - `cargo test -p policyd abi_profile_get_v2 -- --nocapture` ✅
    - includes `test_abi_profile_get_v2_malformed_frame_is_fail_closed` ✅
- OS:
  - `just dep-gate` ✅
  - `just diag-os` ✅
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` ✅
- Observed markers:
  - `abi-profile: ready (server=policyd|abi-filterd)` ✅
  - `abi-filter: deny (subject=selftest-client syscall=statefs.put)` ✅
  - `SELFTEST: abi filter deny ok` ✅
  - `SELFTEST: abi filter allow ok` ✅
  - `abi-filter: deny (subject=selftest-client syscall=net.bind)` ✅
  - `SELFTEST: abi netbind deny ok` ✅

### Lifecycle stop condition (must hold for TASK-0019 closeout)

- TASK-0019 remains boot-time/static lifecycle only:
  - no runtime `learn/enforce` switching,
  - no runtime hot profile reload path,
  - lifecycle transition logic is explicitly deferred to `TASK-0028`.

## Touched paths (allowlist)

- `source/libs/nexus-abi/` (central dispatcher + filter chain; route wrappers through it)
- `source/services/policyd/` (serve profiles; policyd-only in TASK-0019)
- `recipes/policy/` (extend schema for syscall profile rules)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `docs/security/abi-filters.md`

## Plan (small PRs)

1. **Phase A — central filter path in `nexus-abi`**
   - Add a bounded pre-ecall filter path for selected syscall wrappers first.
   - Return stable `AbiError`/`errno` on deny; emit bounded audit deny labels.

2. **Phase B — profile format + matcher**
   - TOML or binary profile, but bounded and deterministic.
   - Default deny; first-match-wins rules for:
     - syscall kind
     - path prefix (statefs)
     - ports (net bind)
     - size bounds.

3. **Phase C — staged service rollout to all shipped OS components**
   - Start with critical service set; expand phase-by-phase until all shipped OS components use wrappers.
   - Record rollout evidence per phase (migrated call-site set + proof command list).

4. **Phase D — profile distribution (explicit separate phase)**
   - `policyd` serves syscall profiles (single policy authority) and emits audit.
   - `abi-filterd` remains out-of-scope for TASK-0019.
   - Enforce authenticated profile source and subject binding (`sender_service_id` + bounded payload).
   - Lifecycle rule for TASK-0019: boot-time fetch/apply only; runtime mode switches and hot reload are follow-up (`TASK-0028`).

5. **Phase E — audit**
   - Denies emit audit events via logd (TASK-0006).
   - Allows are sampled or counted, not line-by-line spam.

6. **Phase F — selftest + marker closure**
   - Denied: `statefs.put("/state/forbidden", ...)` (or another clearly forbidden op).
   - Allowed: `statefs.put("/state/app/selftest/token", ...)` (or allowed prefix).
   - Denied: net bind without permission.
   - Emit markers listed in Stop conditions.

## Policy lifecycle boundary (TASK-0019 vs follow-up)

- TASK-0019 lifecycle is intentionally conservative:
  - profile is fetched/applied at startup (or boot-time deterministic initialization),
  - no runtime `learn/enforce` mode switching,
  - no hot reload loops in this task.
- Runtime lifecycle evolution (`learn`, mode switches, controlled reload epochs) is follow-up scope in `TASK-0028`.

## Acceptance criteria (behavioral)

- Profiles are deny-by-default, deterministic, and bounded.
- Denied calls return stable errors and are audited.
- Documentation clearly states this is not a sandbox without kernel enforcement.
- Rollout proof is phase-based and explicit; "all shipped OS components" is only claimed after final rollout phase evidence.

## Future direction (not in v2): kernel-level seccomp (v3)

If/when we need true enforcement against malicious code:

- add a kernel syscall filter hook keyed by `service_id` and/or a domain token,
- keep the userspace profiles as the authoring format and compile them to a kernel-checked form,
- prove it via negative tests (raw ecall denied even if bypassing `nexus-abi`).

This “true enforcement” work is tracked as: `TASK-0188`.
