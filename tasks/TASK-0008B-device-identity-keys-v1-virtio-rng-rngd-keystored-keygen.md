---
title: TASK-0008B Device identity keys v1 (OS): virtio-rng entropy + rngd + keystored keygen (enables real keys)
status: Done
owner: @runtime @security
created: 2026-01-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (policy baseline): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Policy contract: docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (MMIO mapping primitive): tasks/TASK-0010-device-mmio-access-model.md
  - Depends-on (persistence, optional): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
follow-up-tasks:
  - TASK-0027: StateFS encryption-at-rest (AEAD) via keystored
  - TASK-0053: Signed recovery actions (.nxra)
  - TASK-0108: keymintd + keychain vault
  - TASK-0160: attestd + trust unification (OS)
---

## Context

TASK-0008 establishes policy authority + audit semantics, but secure device identity keys require
real entropy on OS builds. Host builds can use `OsRng`; OS builds cannot depend on `getrandom`.

On QEMU `virt`, virtio-rng is available as an MMIO device. With TASK-0010’s MMIO mapping primitive,
we can implement a tiny userspace virtio-rng frontend and expose a simple entropy service (`rngd`)
to keystored and other consumers.

## Goal

In QEMU, prove:

- A userspace RNG service provides bounded entropy bytes to authorized callers.
- `keystored` can generate a device identity keypair using that entropy and exposes **only** the public key.
- Device private key bytes are never exposed via any API.
- All operations are policy-gated and audit-logged (via logd).

## Non-Goals

- Claiming production-grade hardware entropy on real devices (QEMU bring-up only).
- Full DRBG design, reseeding policies, or complex entropy mixing (keep it minimal and auditable).
- Kernel changes beyond what already exists in TASK-0010.
- Persistence/rotation of device keys (persistence is TASK-0009; lifecycle/rotation lives in TASK-0159/TASK-0160).

## Constraints / invariants (hard requirements)

- **No fake success**: only emit “ok” markers after real entropy was used and real keys exist.
- **Bounded**: requests must be size-bounded; no unbounded reads; deterministic failure modes.
- **No secrets**: never print entropy bytes or private key material in logs/UART.
- **Policy**: entropy/keygen endpoints are deny-by-default and must bind to `sender_service_id`.
- **OS dependency hygiene**: do not add forbidden crates to OS graphs (notably `getrandom`).
- **IPC robustness**: if you use `CAP_MOVE` + shared inboxes, use nonce-correlated replies and close moved caps on all paths (prevents reply misassociation / cap leaks).

## Decisions (v1) — to prevent drift

- **Entropy authority**: ✅ Add `rngd` as the single entropy authority service. Other services (including keystored) must not talk to virtio-rng directly.
- **MMIO capability distribution**: ✅ The virtio-rng `DeviceMmio` cap is granted to `rngd` (not to selftests) so the proof is not “selftest-only injection”.
- **Driver placement**: ✅ Put the virtio-rng frontend under `source/drivers/rng/virtio-rng/` (aligns with `source/drivers/storage/virtio-blk/`). `rngd` uses it as a library.

## Security considerations

### Threat model

- **Entropy spoofing**: caller or compromised service tries to fake RNG output.
- **Key extraction**: private key material leaks via API, logs, or memory dumps.
- **Unauthorized keygen**: unprivileged service triggers device key generation or reads device keys.
- **DoS**: unbounded entropy reads starve the system.

### Security invariants (MUST hold)

- Entropy bytes MUST NOT be logged.
- Device private keys MUST NEVER be returned (signing happens inside keystored).
- Calls MUST be authorized based on `sender_service_id` (channel identity).
- All allow/deny decisions for entropy/keygen MUST be audit-logged.

## Security proof

### Host tests

- `RUSTFLAGS='--cfg nexus_env="os"' cargo test -p keystored --no-default-features --features os-lite -- reject --nocapture`
  - Add `test_reject_entropy_request_oversized`
  - Add `test_reject_device_key_private_export`

### QEMU markers

- `rngd: ready`
- `SELFTEST: rng entropy ok` (bounded request succeeds; no bytes printed)
- `SELFTEST: device key pubkey ok`
- `SELFTEST: device key private export rejected ok`

## Stop conditions (Definition of Done)

### Proof (OS / QEMU)

- Canonical smoke:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- Faster triage (RFC‑0014 Phase 2):
  - `RUN_PHASE=policy RUN_TIMEOUT=190s just test-os`
  - `RUN_PHASE=bring-up RUN_TIMEOUT=90s just test-os`
- Harness expectations updated to include (stable) markers:
  - `rngd: ready`
  - `SELFTEST: rng entropy ok`
  - `SELFTEST: device key pubkey ok`

### Proof (Security)

- Negative tests exist (`test_reject_*`) proving:
  - bounds enforcement
  - no private key export
  - deny-by-default for unprivileged callers

## Touched paths (allowlist)

- `source/drivers/rng/virtio-rng/` (virtio-rng frontend library)
- `source/services/` (new `rngd` service)
- `source/services/keystored/` (device keygen + pubkey API + policy-gated signing)
- `source/apps/selftest-client/` (entropy + pubkey markers; negative-path checks)
- `scripts/qemu-test.sh`
- `docs/security/`

## Plan (small PRs)

1. **virtio-rng frontend (minimal)**
   - Map the virtio-rng MMIO window via the existing device-mmio cap.
   - Implement bounded “read N bytes” from the virtio queue path (polling is fine).

2. **rngd service**
   - Provide a tiny IPC API: `GET_ENTROPY { n } -> bytes`.
   - Enforce bounds and policyd gates.
   - Use `policyd` OP_CHECK_CAP for capability checks (no payload identity strings).
   - Emit `rngd: ready`.

3. **keystored integration**
   - Use `rngd` for key generation.
   - Expose a device identity **public** key API.
   - Explicitly forbid private key export; add negative tests.
   - Gate: `device.keygen` (generation) and optionally `device.pubkey.read` (pubkey query) via OP_CHECK_CAP.

4. **Selftest + harness**
   - Add `SELFTEST: rng entropy ok` + `SELFTEST: device key pubkey ok`.
   - Add markers to `scripts/qemu-test.sh` expectations.
