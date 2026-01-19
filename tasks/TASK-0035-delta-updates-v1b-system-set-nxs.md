---
title: TASK-0035 Delta updates v1b (system sets): nxs delta container + updated orchestration (blocked)
status: Blocked
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Depends-on (bundle deltas): tasks/TASK-0034-delta-updates-v1-bundle-nxdelta.md
  - Depends-on (updates service): tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Depends-on (supply-chain policy): tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Signing policy: docs/security/signing-and-policy.md
---

## Context

We eventually want system-set (`.nxs`) delta updates that apply a set of bundle deltas and stage an A/B update.
However, repo reality today:

- `.nxs` tooling exists (`tools/nxs-pack`) and the system-set contract is defined (RFC-0012).
- `updated` service exists (v1.0 non-persistent skeleton).
- Real “booted slot” proof still requires boot-chain support (tracked separately as `TASK-0037`).

So this task is explicitly **blocked** until those prerequisites exist.

## Goal

Once unblocked, deliver:

- an aggregate delta container for system sets (list of per-bundle patches + integrity index),
- updated-side orchestration:
  - apply per-bundle deltas via bundlemgrd,
  - verify supply-chain policy for all bundles,
  - stage atomically to the target slot,
  - persist checkpoints for resume.

## Stop conditions (Definition of Done)

- Host tests: system delta container make/apply matches expected system set digest.
- OS/QEMU: markers for system delta start/verify/staged and selftest proofs.

## Red flags / decision points

- **RED**: blocked until bundle deltas (`TASK-0034`) and supply-chain policy (`TASK-0029`) are proven.
