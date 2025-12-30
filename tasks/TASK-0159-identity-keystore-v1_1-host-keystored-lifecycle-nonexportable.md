---
title: TASK-0159 Identity/Keystore v1.1 (host-first): keystored key lifecycle + non-exportable ops + typed client + deterministic tests
status: Draft
owner: @security
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Existing keystored service: source/services/keystored/
  - Device keys baseline: tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Keymint/keychain (per-user vault): tasks/TASK-0108-ui-v18b-keymintd-keystore-keychain.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Quotas v1: tasks/TASK-0133-statefs-quotas-v1-accounting-enforcement.md
  - Supply-chain/trust direction: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The repo already contains `keystored` with:

- a Cap’n Proto service focused on **trust anchor distribution + signature verification** (host/OS builds), and
- an OS-lite `KS v1` bring-up shim implementing a tiny in-memory KV store scoped by `sender_service_id`.

This prompt proposes “Identity/Keystore v1.1” which is a different capability set:

- key lifecycle (create/rotate/revoke),
- non-exportable private key usage (sign/seal/unseal),
- per-key policies + audit signals.

To avoid drift, this task defines keystored v1.1 as an **extension** of the keystored trust-root role,
not a parallel service with unrelated semantics.

OS/QEMU integration, attestation stub, trust store unification, and selftests land in `TASK-0160`.

## Goal

Deliver, host-first:

1. `keystored` v1.1 key lifecycle API:
   - create / rotate / revoke
   - metadata includes: appId, purpose, algo, uses, state, version, timestamps (injected clock for tests)
   - stable key IDs (deterministic format)
2. Non-exportable operations:
   - Ed25519 signing (`sign_detached`) where `KeyUse::sign` and state is `active`
   - AEAD sealing/unsealing (Chacha20Poly1305 or XChaCha20Poly1305) where `KeyUse::decrypt` and state is `active`
   - private key bytes are never returned
3. Deterministic test mode:
   - seeded key generation and KEK derivation for host tests only
   - explicit “insecure test-only” guardrails in code and docs (never used as OS proof of security)
4. Storage layout contract (host-first):
   - define the on-disk layout under `state:/keystore/<appId>/<purpose>/...` but do not require `/state` to exist yet
   - host tests use a temp dir adapter; OS persistence is gated
5. Typed client library:
   - `userspace/libs/nexus-keys` (or existing suitable location) wraps keystored RPC with typed errors (`EPERM`, `ENOENT`, `EALREADY`, `EKEYREVOKED`)
6. Deterministic host tests:
   - create/sign/verify
   - rotate semantics (old pub verifies historical signatures; new key used for new signatures)
   - revoke semantics (use fails deterministically with `EKEYREVOKED`)
   - seal/unseal with AAD; tamper fails

## Non-Goals

- Kernel changes.
- Hardware-backed key storage.
- A full per-user encrypted vault / keychain UX (covered by `TASK-0108`).
- Attestation service (covered by `TASK-0160`).

## Constraints / invariants (hard requirements)

- Determinism: tests must not depend on OS RNG or wall clock.
- Non-exportability: no API returns private key bytes.
- Bounded inputs: cap message sizes for sign/seal/unseal and metadata string lengths.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake security: if deterministic seeds are used, it must be explicit and confined to tests/dev mode.

## Red flags / decision points (track explicitly)

- **RED (entropy / device identity security)**:
  - Secure keygen requires an entropy source in OS builds. Until we have a real RNG story, OS keys must not claim “secure”.
  - This task stays host-first; OS work must gate on a clear entropy decision (see `TASK-0008` red flag).

- **YELLOW (service scope drift)**:
  - Existing keystored does “verify anchors” today; v1.1 adds “manage private keys”.
  - We must keep the ABI/IDL versioned and avoid breaking existing bundle verification flows.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p keystore_v1_1_host -- --nocapture` (or equivalent crate)
  - Required tests:
    - lifecycle (create/rotate/revoke)
    - sign/verify
    - seal/unseal
    - deterministic outputs under fixed seed

## Touched paths (allowlist)

- `source/services/keystored/` (extend v1.1 API while preserving existing verify/anchors behavior)
- `tools/nexus-idl/schemas/` (add/extend keystore capnp schema)
- `userspace/libs/nexus-keys/` (new)
- `tests/keystore_v1_1_host/` (new)

## Plan (small PRs)

1. Add/extend schema and implement key lifecycle state machine (host-first, tempdir storage)
2. Implement sign/seal/unseal non-exportable ops with bounds + tests
3. Add typed client library + docs stub notes

## Acceptance criteria (behavioral)

- Host tests deterministically prove lifecycle + non-exportable use.
- No OS/QEMU markers are claimed in this task.

