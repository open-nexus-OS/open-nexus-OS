---
title: TASK-0003B DSoftBus OS Noise XK identity binding (no_std)
status: Draft
owner: @runtime
created: 2026-01-01
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md
  - Parent task: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
---

## Context
- Follow-up to `TASK-0003`: replace the deterministic C/R gate with a real Noise XK handshake (no_std) on the OS DSoftBus transport.
- Current milestone is green with a bounded C/R; this task upgrades to true Noise XK while keeping determinism and userspace-only crypto.

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
