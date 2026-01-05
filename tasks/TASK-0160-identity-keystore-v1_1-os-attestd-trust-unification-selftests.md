---
title: TASK-0160 Identity/Keystore v1.1 (OS/QEMU): attestd stub + trust store unification + policy caps + quotas + selftests/postflight/docs
status: Draft
owner: @security
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Keystore v1.1 host slice: tasks/TASK-0159-identity-keystore-v1_1-host-keystored-lifecycle-nonexportable.md
  - Device keys / entropy red flags: tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Packages trust integration: tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Updates trust integration: tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Policy caps (capability matrix): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With keystored v1.1 core behavior proven host-first (`TASK-0159`), we need OS wiring:

- an attestation stub service (`attestd`) that produces verifiable statements,
- unified trust store consumption for bundle verification and update verification,
- policyd capability gates and quotas,
- OS selftests and docs.

## Goal

Deliver:

1. `attestd` attestation stub:
   - signs a compact canonical statement (device/app/key/nonce/claims)
   - uses a device attestation key managed by `keystored` under `appId=system/purpose=device-attest`
   - markers:
     - `attestd: ready`
     - `attest: ok app=<id> key=<kid>`
2. Trust store unification:
   - define canonical trust roots:
     - `pkg://trust/` (system roots)
     - `state:/trust/installed/` (installed roots)
   - `bundlemgrd` and `updated` consume the same trust store logic (Ed25519)
   - provide a CLI for trust root management (`nx key trust add/list/rm`) if `nx` scaffolding exists; otherwise a minimal host tool
3. Policy caps + quotas:
   - caps:
     - `identity.key.create`, `identity.key.rotate`, `identity.key.revoke`
     - `identity.key.sign`, `identity.key.decrypt`
     - `identity.attest`
     - `identity.trust.write` (system only)
   - quota enforcement for keystore storage (soft/hard) and clear errors on exceed
4. OS selftests (bounded, QEMU-safe):
   - wait for `keystored: ready` and `attestd: ready`
   - create/sign/rotate/seal/attest flows (or use pre-seeded system keys if create is gated)
   - markers:
     - `SELFTEST: keystore v1.1 sign ok`
     - `SELFTEST: keystore v1.1 rotate ok`
     - `SELFTEST: keystore v1.1 seal ok`
     - `SELFTEST: keystore v1.1 attest ok`
5. Docs:
   - `docs/identity/keystore.md`
   - `docs/identity/attestation.md`
   - `docs/trust/overview.md`
   - `docs/tools/nx-key.md`

## Non-Goals

- Kernel changes.
- Real hardware attestation / TEE integration (stub only).
- Claiming secure RNG if entropy story is not solved (must be explicit).

## Constraints / invariants (hard requirements)

- No fake security or fake success markers.
- `/state` gating:
  - trust store installed roots and keystore persistence are only real if `/state` exists (`TASK-0009`).
  - if `/state` is unavailable, must be explicit `stub/placeholder` behavior and selftests must not claim persistence.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (entropy / production readiness)**:
  - Without a real entropy source, OS key generation is not secure. Keep deterministic seeds strictly test-only.
  - Attestation is a stub: it proves plumbing, not hardware trust.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p keystore_v1_1_host -- --nocapture`

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers (to be added to `scripts/qemu-test.sh` expected list):
    - `keystored: ready`
    - `attestd: ready`
    - `SELFTEST: keystore v1.1 sign ok`
    - `SELFTEST: keystore v1.1 rotate ok`
    - `SELFTEST: keystore v1.1 seal ok`
    - `SELFTEST: keystore v1.1 attest ok`

- **Docs gate (keep architecture entrypoints in sync)**:
  - If identity/keystore authority boundaries, trust-store locations, or attestation semantics change, update (or create):
    - `docs/architecture/13-identity-and-keystore.md`
    - `docs/architecture/11-policyd-and-policy-flow.md` (capability gates + trust policy)
    - `docs/architecture/09-nexus-init.md` (boot-time orchestration/gating where applicable)
    - and the index `docs/architecture/README.md`

## Touched paths (allowlist)

- `source/services/keystored/`
- `source/services/attestd/` (new)
- `source/services/bundlemgrd/` (trust store consumption)
- `source/services/updated/` (trust store consumption; if service exists, otherwise defer to the task that introduces it)
- `tools/nx-key/` (new)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `tools/postflight-keystore-v1_1.sh` (delegates)
- `docs/identity/` + `docs/trust/` + `docs/tools/`

## Plan (small PRs)

1. attestd stub + keystored device-attest key wiring
2. trust store unification library + integrate bundlemgrd/updated
3. policy caps + quota enforcement
4. selftests + marker contract + docs + postflight

## Acceptance criteria (behavioral)

- OS/QEMU selftests prove sign/rotate/seal/attest markers deterministically.
- Trust store consumption is unified (one code path), and persistence is gated explicitly on `/state`.
