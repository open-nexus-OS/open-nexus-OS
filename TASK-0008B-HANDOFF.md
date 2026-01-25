# TASK-0008B Handoff: Device identity keys v1 (virtio-rng + rngd + keystored keygen)

**Date**: 2026-01-23  
**Status**: Ready for implementation (scoped + dependency rewiring complete)  
**Scope note**: This task exists because OS builds cannot use `getrandom`, but we still need **real entropy** for real device keys.

---

## Executive Summary

You are implementing **TASK-0008B: real device identity keys on OS/QEMU**:

- Implement a minimal **virtio-rng** userspace frontend (QEMU `virt` MMIO device).
- Add `rngd` as the **single entropy authority** service.
- Wire `keystored` to request bounded entropy from `rngd` and generate a device identity keypair.
- Expose **only** the public key; forbid any private key export; audit all decisions via logd.

This unblocks crypto-heavy follow-ups (statefs encryption, signed recovery actions, attestd, keymintd).

---

## Must-Read Files (in order)

### 1) Repo standards / rules

1. `docs/agents/PLAYBOOK.md`
2. `docs/agents/VISION.md`
3. `docs/standards/SECURITY_STANDARDS.md`
4. `docs/standards/BUILD_STANDARDS.md` (OS dep hygiene; `dep-gate` rules)
5. `docs/standards/RUST_STANDARDS.md`

### 2) Task definitions

- Primary: `tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md`
- Prereqs:
  - `tasks/TASK-0010-device-mmio-access-model.md` (MMIO mapping primitive exists)
  - `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md` (policy baseline)
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md` (logd audit sink; Done)

### 3) Testing contract

- `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (marker contract; use `RUN_PHASE=...` for triage)

---

## Decisions locked (anti-drift)

From `TASK-0008B`:

- **Entropy authority**: `rngd` is the single authority. Other services must not talk to virtio-rng directly.
- **MMIO cap distribution**: virtio-rng `DeviceMmio` cap is granted to `rngd`, not to selftests.
- **Driver placement**: virtio-rng frontend library lives at `source/drivers/rng/virtio-rng/`.

---

## What “Done” means (stop conditions, summarized)

### QEMU proofs (smoke)

- Canonical: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- Triage:
  - `RUN_PHASE=bring-up RUN_TIMEOUT=90s just test-os`
  - `RUN_PHASE=policy RUN_TIMEOUT=190s just test-os`

Stable markers to add (no secrets printed):

- `rngd: ready`
- `SELFTEST: rng entropy ok`
- `SELFTEST: device key pubkey ok`

### Security proofs (required)

Add negative tests proving:

- bounds enforcement on entropy requests (`test_reject_entropy_request_oversized`)
- no private key export (`test_reject_device_key_private_export`)
- deny-by-default for unprivileged callers (policy-gated)

---

## Guardrails / “Don’t accidentally do this”

- Do **not** add `getrandom` to OS graphs.
- Do **not** log entropy bytes, seeds, private keys, or derived secrets.
- Do **not** let selftests be the “owner” of the MMIO cap; `rngd` must own it.
- Keep requests bounded and deterministic in failure modes (timeouts, size caps).

---

## Suggested implementation slices (PR order)

1. **virtio-rng frontend library**
   - minimal bounded read primitive (polling is fine)
2. **`rngd` service**
   - IPC API: `GET_ENTROPY { n } -> bytes`
   - policy gate + audit
3. **keystored keygen**
   - fetch entropy via `rngd`, generate device keypair
   - pubkey API only; forbid private export
4. **selftest + harness**
   - add stable markers; extend `scripts/qemu-test.sh` expectations

---

## RFC seed contract rule (required for every task)

For each task (including TASK-0008 and TASK-0008B), we create a **specific RFC seed contract** that states exactly what is being built (interfaces, invariants, proofs), using:

- `docs/rfcs/README.md` (process + authority model + “contract seed” rule)
- `docs/rfcs/RFC-TEMPLATE.md` (required structure)

**Important**: `docs/rfcs/` is a protected zone in this repo; get explicit approval before adding/updating RFC files.
