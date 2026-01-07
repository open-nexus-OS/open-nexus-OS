---
title: TASK-0003B DSoftBus OS Noise XK handshake (no_std)
status: Done ✅ (Noise XK handshake complete; identity binding enforcement → TASK-0004)
owner: @runtime
created: 2026-01-01
updated: 2026-01-07
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md
  - RFC: docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md
  - Parent task: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
---

## Context
- Follow-up to `TASK-0003`: the deterministic C/R gate has been replaced with a real Noise XK handshake (no_std) on the OS DSoftBus transport.
- Milestone is complete: true Noise XK handshake using `nexus-noise-xk` library (X25519 + ChaChaPoly + BLAKE2s), deterministic and userspace-only crypto.

## Current Status (2026-01-07)

**What's DONE:**
- ✅ Noise XK handshake implementation (`source/libs/nexus-noise-xk`)
- ✅ QEMU marker `dsoftbusd: auth ok` after real Noise handshake
- ✅ Host regression tests (`cargo test -p nexus-noise-xk`)
- ✅ Test keys parameterized (port-based derivation for dual-node)
- ✅ `SELFTEST: dsoftbus ping ok` over encrypted session

**What's DEFERRED to TASK-0004** (requires dual-node for meaningful validation):
- ⬜ Identity binding enforcement (`device_id <-> noise_static_pub` mapping)
- ⬜ Session rejection on identity mismatch
- ⬜ Marker: `dsoftbusd: identity bound peer=<id>`

**Note**: Test keys are labeled as "bring-up test keys" in code (see `derive_test_secret` usage).

**Proof Gates:**
- `just diag-os` ✅
- `just diag-host` ✅
- `just dep-gate` ✅
- QEMU `dsoftbusd: auth ok` ✅
- QEMU `SELFTEST: dsoftbus ping ok` ✅

## Goal
- OS auth path uses Noise XK (X25519 + AEAD) with identity binding; `dsoftbusd: auth ok` only after handshake success.

## Non-Goals
- Kernel crypto or kernel networking.
- Keystored integration (planned follow-up).

## Constraints / invariants
- no_std + alloc (`riscv64imac-unknown-none-elf`).
- Userspace crypto only; no new kernel syscalls.
- Deterministic proof via netstackd IPC facade (loopback); no external network reliance.
- No fake success markers; stubs must be explicit if any remain.

## Red flags / decision points
- RED: Choice of no_std Noise/X25519/AEAD implementation (vendored minimal shim vs crate).
- YELLOW: Key source policy (test static keys now; keystored later).
- GREEN: Ownership model stays: netstackd owns MMIO; dsoftbusd over IPC facade.

## Security considerations (scope: TASK-0003B only)

> **Scope boundary**: This section covers only the Noise XK handshake implementation.
> Identity binding enforcement and session rejection are **TASK-0004** scope.

### Threat model (for handshake implementation)
- **MITM during handshake**: Attacker intercepts TCP and attempts own handshake
- **Key compromise**: Test keys are deterministic and publicly known
- **Replay of handshake messages**: Old msg1/msg2/msg3 replayed

### Security invariants (MUST hold in TASK-0003B)
- Noise XK handshake MUST complete before `dsoftbusd: auth ok` marker
- Session keys derived from handshake MUST be used for all subsequent traffic
- Test keys MUST be labeled `// SECURITY: bring-up test keys, NOT production custody`
- Private keys MUST NOT appear in logs, UART, or error messages

### DON'T DO (TASK-0003B scope)
- DON'T emit `auth ok` without completed Noise handshake
- DON'T log private keys or session keys
- DON'T use test keys in production builds (enforce via `cfg` in future)

### What is NOT in scope (→ TASK-0004)
- Identity binding enforcement (`device_id <-> noise_static_pub`)
- Session rejection on identity mismatch
- Dual-node/cross-VM validation

### Mitigations (implemented)
- Noise XK provides mutual authentication via static keys
- Noise XK provides forward secrecy via ephemeral keys
- All post-handshake traffic encrypted with AEAD (ChaCha20-Poly1305)
- Test keys derived from port number (enables dual-node in TASK-0004)

### Security proof (TASK-0003B scope)
- **Host tests**: `cargo test -p nexus-noise-xk` — handshake correctness
- **QEMU marker**: `dsoftbusd: auth ok` — only after real Noise handshake
- **QEMU marker**: `SELFTEST: dsoftbus ping ok` — encrypted roundtrip works

## Contract sources (single source of truth)
- QEMU marker contract: `scripts/qemu-test.sh`
- Transport contract: `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md`

## Stop conditions (Definition of Done)
- Proof (QEMU):
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Required markers: `dsoftbusd: auth ok` emitted only after Noise XK handshake success (not C/R gate).
- Proof (tests):
  - If added: `cargo test -p dsoftbus` covering Noise OS handshake path (or dedicated integration test).

## Touched paths (allowlist)
- `source/services/dsoftbusd/**`
- `source/apps/selftest-client/**`
- `userspace/dsoftbus/**`
- `scripts/qemu-test.sh`
- `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md`
- `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`

## Plan (small PRs)
1. Introduce no_std Noise/X25519/AEAD shim (test static keys) + wire into dsoftbusd auth path; selftest loopback proves handshake; markers updated.
2. Harden/cleanup markers and docs; keep deterministic proof; ensure qemu-test green.

## Acceptance criteria (behavioral)
- Noise XK handshake completes on OS path; `dsoftbusd: auth ok` follows real handshake.
- Selftest still emits `SELFTEST: dsoftbus os connect ok` and `SELFTEST: dsoftbus ping ok`.
- No regressions to existing networking markers.

## Evidence (to paste into PR)
- QEMU: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` with `uart.log` tail showing Noise-based `dsoftbusd: auth ok`.
- Tests: any added `cargo test -p dsoftbus ...` summaries (if implemented).

## RFC seeds (for later, when the step is complete)
- Decisions: chosen no_std Noise/crypto shim; key policy (test keys).
- Open questions: keystored integration path/timing; stronger identity attestation.
- Stabilized contracts: marker semantics for Noise auth; pointer to tests/markers enforcing it.
