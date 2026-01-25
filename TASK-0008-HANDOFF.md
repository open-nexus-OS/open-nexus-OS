# TASK-0008 Handoff: Security hardening v1 (policy authority + audit baseline)

**Date**: 2026-01-23  
**Status**: Ready for implementation (scoped + drift resolved)  
**Scope note**: TASK-0008 is the **policy/audit baseline**. **Device key entropy + keygen** is split into **TASK-0008B**.

---

## Executive Summary

You are implementing **TASK-0008: Security hardening v1 (OS)**:

- `policyd` becomes a small, enforceable policy engine (library-backed) with **channel-bound identity**.
- Sensitive operations are **deny-by-default** and enforced via `policyd` as the single authority.
- **Audit trail** exists for allow/deny decisions (via logd; UART is a bounded fallback only).
- **No entropy / device keygen** is done here. That is explicitly **TASK-0008B**.

This task is security-critical but intentionally scoped to be implementable without changing the kernel and without requiring OS entropy.

---

## Must-Read Files (in order)

### 1) Repo standards / rules

1. `docs/agents/PLAYBOOK.md`
2. `docs/agents/VISION.md`
3. `docs/standards/SECURITY_STANDARDS.md`
4. `docs/standards/BUILD_STANDARDS.md`
5. `docs/standards/RUST_STANDARDS.md`
6. `docs/standards/DOCUMENTATION_STANDARDS.md`

### 2) Task definitions

- Primary: `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
- Related:
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md` (DONE; audit sink via logd exists)
  - `tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md` (entropy + real device keys)

### 3) Testing contract

- `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (marker contract; use `RUN_PHASE=...` for triage)

---

## Decisions locked (anti-drift)

From `TASK-0008`:

- **Policy source of truth**: `recipes/policy/` (evolve `recipes/policy/base.toml`; no parallel `recipes/sel/*`).
- **Identity**: decisions/audits bind to **`sender_service_id`**. Subject strings are display-only.

---

## What “Done” means (stop conditions, summarized)

### Host proofs (fast path)

- `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p policyd --no-default-features --features os-lite -- --nocapture`
- `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p keystored --no-default-features --features os-lite -- --nocapture`
- `cargo test -p e2e_policy -- --nocapture`

Minimum required negative tests (`test_reject_*`) must exist for spoofing/bounds/authz.

### QEMU proofs (smoke)

- Canonical: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- Triage (Phase 2 helpers): `RUN_PHASE=policy RUN_TIMEOUT=190s just test-os`

Markers must be stable, deterministic, and must not leak policy config details:

- `SELFTEST: policy deny audit ok`
- `SELFTEST: policy allow audit ok`
- `SELFTEST: keystored sign denied ok`

---

## Guardrails / “Don’t accidentally do this”

- Do **not** trust identity strings from payload bytes (must bind to `sender_service_id`).
- Do **not** duplicate policy logic inside bundlemgrd/execd/keystored (single authority: `policyd`).
- Do **not** emit “ok” markers for stubs or partial work.
- Do **not** add OS-forbidden crates (`getrandom`, `parking_lot`, etc.).
- Do **not** leak secrets or policy details in UART/log output.

---

## Implementation slices (suggested PR order)

1. **Policy evaluation library hook-up**
   - `policyd` uses a pure evaluation library (planned `source/libs/nexus-sel/`).
   - Note: `source/libs/**` is protected; get explicit approval before implementing there.

2. **Audit emission contract**
   - Prefer logd sink (TASK-0006 is Done).
   - Ensure every allow/deny produces an audit record; add negative tests.

3. **Enforcement adapters**
   - bundlemgrd/execd/keystored route sensitive operations through policyd checks.
   - Keep audit as a side effect of the decision, not separate duplicated logic.

4. **Selftest markers**
   - Add stable markers that prove allow/deny/audit occurred without leaking secrets.

---

## RFC seed contract rule (required for every task)

For each task (including TASK-0008 and TASK-0008B), we create a **specific RFC seed contract** that states exactly what is being built (interfaces, invariants, proofs), using:

- `docs/rfcs/README.md` (process + authority model + “contract seed” rule)
- `docs/rfcs/RFC-TEMPLATE.md` (required structure)

**Important**: `docs/rfcs/` is a protected zone in this repo; get explicit approval before adding/updating RFC files.
