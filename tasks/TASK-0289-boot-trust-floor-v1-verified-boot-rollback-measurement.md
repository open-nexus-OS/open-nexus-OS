---
title: TASK-0289 Boot trust floor v1: verified boot anchors + rollback indices + measured boot handoff
status: Draft
owner: @security @runtime @updates
created: 2026-04-13
depends-on:
  - TASK-0007
  - TASK-0009
  - TASK-0029
  - TASK-0198
  - TASK-0260
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Updates/packaging v1.0: docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md
  - Supply-chain v1: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Supply-chain v2b OS enforcement: tasks/TASK-0198-supply-chain-v2b-os-enforcement-store-updater-bundlemgrd.md
  - Provisioning/recovery: tasks/TASK-0260-provisioning-recovery-v1_0a-host-image-builder-flasher-protocol-deterministic.md
  - Keystore/attestation OS wiring: tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The current security root direction explicitly calls for verified boot plus signed packages, but the
repo still lacks a concrete production-floor task that closes:

- boot-chain verification anchors,
- anti-rollback state tied to the boot path,
- and measured-boot handoff to higher-level services.

Without this, "production-grade" remains incomplete even if runtime/services are otherwise solid.

## Goal

Deliver a boot trust floor that makes the QEMU/bringup path honest and upgradeable:

- verified boot checks are real,
- rollback is deterministically rejected using a monotonic boot-chain-backed index,
- and a measured-boot event log is handed to canonical userland consumers without inflating kernel policy scope.

## Non-Goals

- Full hardware TEE / secure-element custody.
- Remote attestation protocol.
- Device-specific ROM vendor flows beyond the repo's portable contract.
- Claiming hardware-rooted security where only bring-up anchors exist.

## Constraints / invariants (hard requirements)

- **No security theater**: if a root is software/QEMU-bound, label it clearly.
- **Boot chain first**: anti-rollback must be anchored in the verified boot path, not bolted on later in userspace.
- **Measured boot is additive**: measurement handoff must not become a second policy engine.
- **Deterministic denial**: rollback/signature failures produce stable reasons and no fake-ready markers.

## Red flags / decision points (track explicitly)

- **RED (anchor choice)**:
  - define the minimal portable trust anchor for QEMU/bringup and its upgrade path to real hardware.
- **RED (rollback storage)**:
  - monotonic rollback state must not depend on mutable ordinary userspace files alone.
- **YELLOW (measurement scope)**:
  - keep the measured log minimal and stable enough for later attestation, but do not over-design the format now.

## Security considerations

### Threat model
- Downgrade to vulnerable images/packages.
- Boot image tampering before userland policy starts.
- Measurement spoofing or omission.

### Security invariants (MUST hold)
- Boot verification happens before the system claims a trusted boot state.
- Rollback index checks are monotonic and tamper-evident within the declared bring-up trust model.
- Measurement records are append-only for a boot and handed off intact.

### DON'T DO (explicit prohibitions)
- DON'T implement "userspace-only anti-rollback" and call it production-grade.
- DON'T emit `ready/ok` markers before verification is complete.
- DON'T conflate measured boot with remote attestation.

## Contract sources (single source of truth)

- Verified boot / updates skeleton: `RFC-0012`
- Supply-chain enforcement: `TASK-0029`, `TASK-0198`
- Provisioning/recovery flows: `TASK-0260`

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - tests prove:
    - signature/manifest verification gates boot input as documented,
    - rollback index comparison rejects downgraded artifacts,
    - measured-boot record layout is stable.
- **Proof (OS/QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh`
  - required markers:
    - `boot: verified ok`
    - `boot: rollback reject ok`
    - `SELFTEST: measured boot log ok`

## Touched paths (allowlist)

- `source/kernel/neuron/src/arch/riscv/`
- `source/kernel/neuron/src/boot/`
- `source/init/nexus-init/`
- `source/services/updated/`
- `source/services/bootctld/`
- `source/services/keystored/`
- `source/services/attestd/`
- `source/libs/nexus-abi/`
- `docs/security/`
- `docs/architecture/`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Define the portable verified-boot / rollback floor for QEMU.
2. Add rollback-index persistence and rejection semantics.
3. Add measured-boot event log handoff to canonical consumers.
4. Prove verification, rollback rejection, and measurement markers in QEMU.

## Acceptance criteria (behavioral)

- Boot trust is no longer implied only by package signing; the boot path itself proves trust decisions.
- Downgraded or invalid artifacts are rejected with stable reasons.
- Later attestation work can build on a real measured-boot handoff instead of a placeholder.

## Evidence (to paste into PR)

- QEMU: verification / rollback / measured-boot markers.
- Tests: exact verification and rollback test summaries.
