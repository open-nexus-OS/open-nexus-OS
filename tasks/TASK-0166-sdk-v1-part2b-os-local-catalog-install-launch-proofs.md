---
title: TASK-0166 SDK v1 Part 2b (OS/QEMU): local catalog + install/launch proofs (offline) + selftests/docs
status: Draft
owner: @devx
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - SDK devtools workflow: tasks/TASK-0165-sdk-v1-part2a-devtools-lints-pack-sign-ci.md
  - Packages install/launch (bundlemgrd/installer UI): tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Installer UI integration: tasks/TASK-0131-packages-v1c-installer-ui-openwith-launcher-integration.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Identity/Keystore v1.1 trust/signing: tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

SDK v1 Part 2a is host-first tooling. OS/QEMU proofs cannot run host CLIs inside QEMU.
So “install/list/launch” must be proven by **OS services**:

- a local catalog/index under `/state`,
- install of a signed `.nxb` from a deterministic URI (e.g., `pkg://fixtures/...`),
- launch via bundle manager / app manager paths,
- bounded selftests and markers.

This task is OS-gated on real `/state` and packages install support.

## Goal

Deliver:

1. Local catalog surface (implementation choice):
   - Prefer reusing `bundlemgrd` install database as the “catalog” if it exists.
   - Only introduce a dedicated `catalogd` service if required; avoid duplicating authority/format.
2. Deterministic fixture install:
   - ship a prebuilt, signed fixture bundle in `pkg://fixtures/sdk/hello.nxb/` (or equivalent)
   - OS selftest requests install via bundle manager/catalog API
3. Launch proof:
   - after install, launch the installed app deterministically and observe a marker from the app
4. Markers:
   - `catalog: ready` (or `bundlemgrd: catalog ready` depending on design)
   - `SELFTEST: sdk v1 install ok`
   - `SELFTEST: sdk v1 launch ok`
5. Docs:
   - document the offline local install flow and how SDK artifacts map to installed bundles

## Non-Goals

- Kernel changes.
- Running `nx` inside QEMU.
- Full app store / remote catalog.

## Constraints / invariants (hard requirements)

- Determinism: fixture bundle bytes are fixed; install database ordering stable; markers stable.
- No fake success: selftest markers only after actual install + successful launch.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (requires `/state`)**:
  - install database and catalog persistence require `TASK-0009`.

- **RED (requires packages install path)**:
  - without `bundlemgrd` install/launch support (`TASK-0130/0131`), this task cannot be proven in QEMU.

## Stop conditions (Definition of Done)

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: sdk v1 install ok`
    - `SELFTEST: sdk v1 launch ok`

## Touched paths (allowlist)

- `source/apps/selftest-client/`
- `source/services/bundlemgrd/` (install path and/or catalog listing)
- `userspace/apps/<fixture-app>/` (fixture app marker)
- `scripts/qemu-test.sh` (marker contract update)
- `docs/sdk/packaging.md` (OS mapping section)

## Plan (small PRs)

1. Decide whether `catalogd` is needed vs reuse bundlemgrd database; document the decision
2. Add deterministic fixture bundle + trust roots + install API path
3. Add selftest install/launch proof + marker contract + docs

## Acceptance criteria (behavioral)

- In QEMU (when unblocked), selftest proves install + launch of a signed local bundle without any host tooling.
