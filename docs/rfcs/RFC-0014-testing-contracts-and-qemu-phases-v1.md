<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0014: Testing contracts v1 — host-first service contract tests + phased QEMU smoke gates

- Status: Accepted (Phase 0 complete; Phase 1/2 pending)
- Owners: @runtime, @tools-team
- Created: 2026-01-22
- Last Updated: 2026-01-22
- Links:
  - Tasks: TBD (post-RFC acceptance; keep tasks as the single execution/proof truth)
  - Related RFCs:
    - `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` (readiness vs up; determinism; bounded waits)
    - `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` (OTA/update proof markers; `updated` contract surface)
    - `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (logd contract + E2E expectations)
    - `docs/rfcs/RFC-0003-unified-logging.md` (no fake success; marker discipline)
    - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (IPC semantics; capability move model)
  - Testing methodology: `docs/testing/index.md`
  - Security standards: `docs/standards/SECURITY_STANDARDS.md`

## Status at a Glance

Implementation status (what exists in the repo today):

- **Phase 0 (Current host E2E tests + existing QEMU smoke markers)**: ✅
- **Phase 1 (Contract tests for critical service breakpoints)**: ✅
- **Phase 2 (Phased QEMU smoke helpers + debug-first guidance)**: ✅

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".
- **Accepted** means: the contract is accepted and at least one phase is implemented. Remaining phases are pending follow-up tasks.
- This RFC is the **source of truth for the contract**; tasks are the execution truth for proofs and stop conditions.
- Phase "done" criteria are listed in **Rollout / adoption plan** below.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - What “service contract tests” mean in Open Nexus OS (normative definitions + minimum coverage set).
  - A repo-wide approach to prevent “days of manual QEMU debugging” after non-trivial changes by shifting left:
    - host-first protocol/contract tests, and
    - short, phased QEMU smoke gates for integration.
  - Deterministic failure reporting requirements for tests and UART markers.
- **This RFC does NOT own**:
  - Changes to the kernel scheduler / fairness policy (unless required to satisfy bounded syscall semantics).
  - Kernel IPC semantics changes (owned by RFC-0005 + kernel tasks).
  - Broad harness rewrites; the goal is incremental gates, not replacing `scripts/qemu-test.sh`.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC defines contracts; follow-up tasks implement the proofs and wire them into CI.

## Context

The current QEMU smoke gate (`just test-os` → `scripts/qemu-test.sh`) is an authoritative end-to-end signal, but it is a **late, coarse signal**:

- A missing UART marker (e.g. `SELFTEST: ota stage ok`) tells us the run failed, but not *why* (routing, inbox mixing, bounded deadline behavior, service contract regression, etc.).
- Because OS bring-up uses cooperative scheduling and IPC queues, “deterministic spawn” does not imply deterministic *interaction*.
- The result is a recurring workflow failure mode: after a larger change, developers end up debugging services one-by-one in QEMU, for days.

This RFC defines a testing strategy that keeps QEMU authoritative, but stops using it as the *first* and *only* feedback mechanism.

## Goals

- Make most regressions fail **early and locally** (host-first) via contract tests on the actual IPC handlers and userspace domain libraries.
- Keep QEMU smoke authoritative, but split it into **phases** that isolate where failures occur (bring-up, routing, exec, policy, vfs, logd, ota, …).
- Ensure tests and markers are:
  - deterministic,
  - bounded (no unbounded waiting/poll loops),
  - actionable (failures include stable reason classification).

## Non-Goals

- Replacing QEMU smoke with host-only tests.
- Adding “fake green” readiness/ok markers or weakening the marker contract.
- Building a full distributed/fuzzing test farm; this is v1, minimal set with high ROI.

## Constraints / invariants (hard requirements)

- **Determinism**: proofs and markers are deterministic; no timing-fluke “usually ok”.
- **No fake success**: never emit `ok` / `ready` unless the real behavior occurred.
- **Bounded resources**: explicit limits for buffers, loops, deadlines, queue depth, and pagination windows.
- **Security floor**:
  - Deny-by-default behavior remains enforced even under bring-up.
  - No secrets in logs/UART; negative tests exist for security-relevant contracts.
- **Stubs policy**: any stub must be explicitly labeled (`stub`/`placeholder`) and must not claim success.

## Proposed design

### 1) “Contract tests” definition (normative)

A **contract test** is a host-executable test that exercises a stable contract surface with:

- **Real framing** (Cap’n Proto where applicable; real request/response structs),
- **Real handler** code (service request handler or domain library), and
- **Real error model** expectations (deny-by-default, bounds rejection, reason taxonomy).

Contract tests must include:

- Positive path tests (“happy path”), and
- Negative tests for reject/deny/invalid inputs (`test_reject_*`) when security-relevant.

### 1b) Contract stability / versioning

This RFC does **not** claim a new stable ABI or on-wire format. If any IPC contract is promoted to “stable” as part of the rollout, tasks must include:

- golden vectors / layout tests, and
- explicit compatibility checks for versioned frames.

### 2) Minimum critical contract set (v1)

This RFC defines the minimum set that must have host-first contract coverage because failures there commonly cascade into opaque QEMU marker misses:

- **Init-lite routing v1**:
  - `ROUTE_GET` correctness under retries, stale replies, and mixed inbox traffic.
  - Deterministic bounded retry/yield strategy (no infinite waits).
- **Kernel IPC v1 usage contracts in userspace (`nexus-ipc` / clients)**:
  - Deadline semantics (timeout is deterministic).
  - Reply routing: CAP_MOVE/@reply handling must be robust against “shared inbox” mixing (filtering by opcode / conversation id where available).
- **logd**:
  - APPEND → QUERY → STATS roundtrip, including paging and bounded buffers.
  - Query correctness when records contain structured fields (scope/fields/msg).
- **policyd**:
  - Allow/deny decision is enforced and audited (deny-by-default).
  - Reject spoof attempts (identity from kernel IPC, not payload strings).
- **bundlemgrd**:
  - Slot-aware publication/visibility (active slot vs standby).
  - Route-status / capability requirements surfaced deterministically.
- **updated (OTA contract)**:
  - `stage → switch → health/rollback` semantics.
  - Explicit, bounded failure reasons that allow selftest to emit deterministic success/fail markers.

Notes:

- This set is intentionally small and is chosen for debugging ROI, not feature completeness.
- Individual services may add additional contract tests, but v1 focuses on the shared “breakpoints”.

### 3) Phased QEMU smoke gates (normative)

QEMU remains authoritative, but the smoke flow must be structured to answer: **“which phase regressed?”**.

Principle:

- A QEMU run must be able to stop early once a phase is proven (`RUN_UNTIL_MARKER=1` style), and failures must point to a named phase, not a generic marker miss.

Proposed phases (names illustrative; final marker strings must be stable and documented in `docs/testing/index.md`):

- **Phase: bring-up**: banner → `init: ready` plus required `<svc>: ready` markers (RFC-0013 readiness semantics).
- **Phase: routing**: selftest proves routing v1 deterministically (`SELFTEST: ipc routing ok`).
- **Phase: exec**: loader + lifecycle selftest markers.
- **Phase: policy**: allow/deny markers.
- **Phase: vfs**: stat/read/ebadf markers.
- **Phase: logd**: query/stats markers.
- **Phase: ota**: `SELFTEST: ota stage ok` and subsequent gates as defined by RFC-0012.

The harness must treat missing “phase ok” markers as a hard failure and print the name of the failed phase (not just “missing marker X”).

### 4) Deterministic failure reporting (normative)

Where a phase depends on IPC interactions, failures must carry stable reason classification to be actionable:

- Examples: `TimedOut`, `RouteNotFound`, `DeniedByPolicy`, `InvalidFrame`, `OversizedInput`, `BadSignature`, `SlotMismatch`, `UnexpectedReplyOpcode`.
- Do not leak secrets. Do not dump raw key material. Keep reasons classification-only on UART.

This RFC does not mandate the exact encoding yet (enum value vs string); it mandates stability and boundedness.

## Rollout / adoption plan (non-normative, v1 minimal slice)

This section exists to keep the initial rollout small and high-ROI. It is **not** a replacement for tasks; tasks will carry concrete stop conditions and proof commands.

### Guiding principle

- Prefer contract tests that protect **shared breakpoints** (routing, inbox/reply matching, paging) over “feature completeness”.
- Prefer phases that fail **early** and **locally**, so regressions do not require multi-day per-service QEMU debugging.

### Phase 0 (current tests we already have)

Phase 0 names the **existing** tests that already provide signal today, so we can build on them without pretending they do not exist:

- Host E2E suites in `docs/testing/index.md`:
  - `nexus-e2e` (`just test-e2e`)
  - `logd-e2e`
  - `e2e_policy`
  - `vfs-e2e`
  - `remote_e2e`
- QEMU smoke (`RUN_UNTIL_MARKER=1 just test-os`) with the current marker sequence.

Phase 0 outcome: we acknowledge the current baseline and map failures to the phase ladder defined below.

Done when:

- existing host E2E suites are listed (so they can be used as baseline proofs),
- the QEMU smoke marker sequence is explicitly referenced,
- the phase ladder below is the mapping for failures.

### Phase 1 (contract tests for shared breakpoints)

Add host-first contract tests for the highest-ROI breakpoints (routing, logd paging, policy, updated OTA framing, bundlemgrd slot publication). Each test must be:

- bounded (deadline/timeouts),
- deterministic (no probabilistic success),
- explicit about failure reasons.

Phase 1 outcome: regressions in these breakpoints fail **before** QEMU.

Done when:

- contract tests exist for the adopt-first list below,
- each test is bounded and deterministic,
- host proof commands are documented.

### Phase 2 (phased QEMU helpers + debug-first guidance)

Add small helpers that make QEMU smoke faster to triage without expanding scope:

- phase naming in harness output (first failed phase, not only “missing marker”),
- optional early-exit by phase (e.g., `RUN_PHASE=policy` or equivalent marker-based stop),
- short, bounded log excerpts for the failed phase.

Phase 2 outcome: when QEMU fails, the failure is localized to a phase with actionable context.

Done when:

- QEMU harness output includes the **first failed phase** name,
- optional early-exit by phase is supported (`RUN_PHASE=<name>`),
- logs are bounded and scoped to the failed phase.

### Adopt-first contract list (order matters)

1. **Init-lite routing v1 contract tests** (highest leverage)
   - Why first: routing failures cascade into “missing marker” symptoms across multiple services.
   - Minimum coverage:
     - retry + bounded yield,
     - stale replies in shared inbox,
     - mixed inbox traffic (non-route replies present).

2. **logd contract tests (APPEND/QUERY/STATS + paging)** (high leverage)
   - Why next: many readiness/phase checks use logd as a stopgap; paging bugs make failures opaque.
   - Minimum coverage:
     - paging over multiple records,
     - structured fields (`scope`, `fields`, `msg`) query hit behavior,
     - bounded buffers and deterministic truncation/drop policy.

3. **policyd allow/deny + spoof rejection tests** (security-critical)
   - Why: deny-by-default is a repo invariant; regressions must be caught before QEMU.
   - Minimum coverage:
     - allow/deny classification,
     - reject spoofed identity (payload strings are non-authoritative),
     - at least one `test_reject_*` input-bounds case.

4. **updated OTA contract tests (stage/switch/health/rollback framing)** (Task 7 anchor)
   - Why: protects the `SELFTEST: ota stage ok` marker from becoming a long debug loop.
   - Minimum coverage:
     - stage rejects bad signature / oversized inputs deterministically,
     - stage success emits stable “ok” condition (no partial success),
     - failure reasons stable and non-secret.

5. **bundlemgrd slot-aware publication + route-status surface** (integration glue)
   - Why: ties together update slot state and service execution/visibility; frequent “works on host, fails in QEMU” edge.

### Minimal phased QEMU ladder (start small)

The goal is not to add many new markers, but to validate the existing ones in a **short, phased** manner. The initial ladder should be:

- **Phase bring-up**: reach `init: ready` + core `<svc>: ready` markers (RFC-0013 semantics).
- **Phase routing**: `SELFTEST: ipc routing ok`
- **Phase logd**: `SELFTEST: log query ok`
- **Phase policy**: `SELFTEST: policy allow ok` / `SELFTEST: policy deny ok`
- **Phase vfs**: `SELFTEST: vfs stat ok` / `SELFTEST: vfs read ok`
- **Phase ota**: `SELFTEST: ota stage ok`

Stop condition for the initial adoption: failures must name the first failing phase and must not require “debug every service” to determine where the regression lives.

### Phase checklist (current state)

- **Phase 0**
  - [x] Existing host E2E suites listed as baseline proofs.
  - [x] QEMU smoke marker sequence referenced.
  - [x] Phase ladder defined for failure mapping.
- **Phase 1**
  - [x] Contract tests exist for routing/logd/policy/updated/bundlemgrd breakpoints.
  - [x] Tests are bounded + deterministic.
  - [x] Phase 1 proof commands listed.
- **Phase 2**
  - [x] QEMU harness names the first failed phase.
  - [x] Optional early-exit by phase is available (`RUN_PHASE=<name>`).
  - [x] Logs are bounded and scoped to the failed phase.

## Security considerations

- **Threat model**:
  - Confused-deputy (service accepts spoofed identity).
  - Input-driven DoS (unbounded buffers, unbounded waits).
  - Secret exposure via UART markers or verbose logs.
- **Mitigations**:
  - Contract tests require negative-path coverage (`test_reject_*`) for policy/auth/crypto boundaries.
  - Bounded waits for IPC (deadline semantics tested).
  - Marker discipline: reason classifications only; no sensitive data.
- **Open risks**:
  - If reply routing lacks a stable conversation id, “shared inbox” mixing remains a source of flakiness unless client code filters by opcode and drains stale replies deterministically.

## Failure model (normative)

- Contract tests must fail with actionable reason classification.
- QEMU phases must fail with:
  - a stable phase name,
  - the first failing marker expectation, and
  - a bounded excerpt of relevant logs (consistent with `docs/testing/index.md` log trimming guidance).

## Proof / validation strategy (required)

Canonical proofs (to be implemented by follow-up tasks):

- Proofs must be **honest green**: no log-grep optimism; markers must reflect real behavior.

### Phase proofs (explicit)

#### Phase 0 proofs (current baseline)

```bash
cd /home/jenning/open-nexus-OS && just test-e2e
cd /home/jenning/open-nexus-OS && cargo test -p logd-e2e
cd /home/jenning/open-nexus-OS && cargo test -p e2e_policy
cd /home/jenning/open-nexus-OS && cargo test -p vfs-e2e
cd /home/jenning/open-nexus-OS && cargo test -p remote_e2e
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

#### Phase 1 proofs (contract tests; to be filled as they land)

Phase 1 is ✅ once these concrete contract tests exist and are listed here. Canonical host proof commands:

```bash
# Routing contract (host, OS-mode in-process router backend)
cd /home/jenning/open-nexus-OS && RUSTFLAGS='--cfg nexus_env="os"' cargo test -p nexus-ipc

# logd contract (paging/bounds + query/stats)
cd /home/jenning/open-nexus-OS && cargo test -p logd-e2e

# policyd contract (allow/deny + requester spoof rejection) — os-lite frame handler tests
cd /home/jenning/open-nexus-OS && RUSTFLAGS='--cfg nexus_env="os"' cargo test -p policyd --no-default-features --features os-lite

# updated OTA/domain contracts (system-set parsing + signature/digest/bounds + BootCtrl stage/switch/health/rollback)
cd /home/jenning/open-nexus-OS && cargo test -p updates_host

# bundlemgrd os-lite contracts (slot-aware image + set_active_slot framing + bounds)
cd /home/jenning/open-nexus-OS && RUSTFLAGS='--cfg nexus_env="os"' cargo test -p bundlemgrd --no-default-features --features os-lite
```

#### Phase 2 proofs (QEMU helpers)

Phase 2 is ✅ once the harness provides phase-first failure output and phase-based early exit.

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os

# Stop after specific phases (triage helper)
cd /home/jenning/open-nexus-OS && RUN_PHASE=bring-up RUN_TIMEOUT=90s just test-os
cd /home/jenning/open-nexus-OS && RUN_PHASE=policy RUN_TIMEOUT=190s just test-os
cd /home/jenning/open-nexus-OS && RUN_PHASE=vfs RUN_TIMEOUT=190s just test-os
```

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test --workspace
```

### Proof (Security)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p e2e_policy
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers (initial set; must remain stable once adopted)

- `SELFTEST: ipc routing ok`
- `SELFTEST: log query ok`
- `SELFTEST: policy allow ok`
- `SELFTEST: policy deny ok`
- `SELFTEST: vfs stat ok`
- `SELFTEST: vfs read ok`
- `SELFTEST: ota stage ok`

## Alternatives considered

- **Keep only E2E QEMU smoke**: rejected; too late/coarse, causes multi-day manual debugging.
- **Move all testing to QEMU**: rejected; violates host-first principle and is too slow.
- **Add kernel-only diagnostics**: rejected for v1; we keep kernel minimal and shift left in userspace where possible.

## Open questions

- Do we standardize a conversation id for request/response matching in userspace IPC contracts (beyond opcode filtering), and where is it owned (IDL vs `nexus-ipc`)?
- (Resolved in v1) We added a first-class phase knob `RUN_PHASE=<name>` to the QEMU harness for triage; keep the phase names/markers stable once adopted.

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

- [x] **Phase 0**: Existing host E2E + QEMU smoke baseline.
  - Proof: `just test-e2e`, `cargo test -p logd-e2e`, `cargo test -p e2e_policy`, `just test-os`
- [x] **Phase 1**: Contract tests for routing/logd/policy/updated/bundlemgrd breakpoints.
  - Proof:
    - `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p nexus-ipc`
    - `cargo test -p logd-e2e`
    - `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p policyd --no-default-features --features os-lite`
    - `cargo test -p updates_host`
    - `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p bundlemgrd --no-default-features --features os-lite`
- [x] **Phase 2**: QEMU harness phase-first failure output + early-exit by phase.
  - Proof:
    - `RUN_PHASE=bring-up RUN_TIMEOUT=90s just test-os`
    - `RUN_PHASE=policy RUN_TIMEOUT=190s just test-os`
    - `RUN_PHASE=vfs RUN_TIMEOUT=190s just test-os`
- [x] Follow-up task(s) created for Phase 2 with stop conditions: `tasks/TASK-0285-rfc0014-phase2-qemu-harness-phases-v1.md`
