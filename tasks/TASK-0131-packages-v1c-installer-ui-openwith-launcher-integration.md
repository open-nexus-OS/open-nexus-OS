---
title: TASK-0131 Packages v1c: Installer UI + Files/Open-With integration + Launcher refresh + OS proofs + docs/postflight
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - bundlemgrd install pipeline: tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Installer v1.1 (atomic A/B, migrations, entitlements): tasks/TASK-0238-installer-v1_1a-host-nab-semver-migrations-policy-deterministic.md, tasks/TASK-0239-installer-v1_1b-os-pkgr-atomic-ab-bundlemgr-registry-licensed-selftests.md
  - MIME/Open-With + picker: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Files app integration: tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - SystemUI→DSL migration: tasks/TASK-0119-systemui-dsl-migration-phase1a-launcher-qs-host.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Packages v1 is user-facing only if:

- users can open `.nxb` bundles from Files/Picker,
- see a clear installer confirmation UI (manifest + signer),
- and the installed app appears in Launcher and can be launched.

## Goal

Deliver:

1. Installer UI app (`userspace/apps/installer`):
   - launched with a `content://` (or `pkg://selftest`) URI pointing to a bundle directory
   - shows manifest summary (name, appId, version), signer identity (fingerprint), and declared capabilities (record-only v1)
   - user confirms “Install” → calls `bundlemgrd.install`
   - on success: “Open app” and “Done”
   - markers:
     - `installer: open uri=...`
     - `installer: install ok <appId>@<ver>`
2. Open-With integration:
   - `mimed` maps `application/x-nxbundle` to Installer by default
   - Files context menu “Install package” and double-click open
   - markers:
     - `files: install action uri=...`
3. Launcher refresh:
   - Launcher refreshes app list after install/uninstall events (direct query or notification)
   - marker: `launcher: apps refreshed n=...`
4. OS selftests + postflight:
   - install a signed selftest bundle, launch it, uninstall it (markers)
   - `tools/postflight-pkg-v1.sh` delegates to host tests + QEMU marker run
5. Docs:
   - `docs/packages/installer.md`
   - update `docs/dev/ui/testing.md` with package v1 marker list

## Non-Goals

- Kernel changes.
- Real permission enforcement (v1 shows and records declared capabilities; enforcement follows sandboxing/policy tasks).

## Constraints / invariants (hard requirements)

- Deterministic UI text for test fixtures.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `installer: install ok`
- `SELFTEST: pkg v1 install ok`
- `SELFTEST: pkg v1 launch ok`
- `SELFTEST: pkg v1 uninstall ok`

## Touched paths (allowlist)

- `userspace/apps/installer/` (new)
- `userspace/apps/files/` (integration)
- `userspace/apps/launcher/` (refresh wiring)
- `source/apps/selftest-client/`
- `tools/postflight-pkg-v1.sh`
- `docs/packages/`

## Plan (small PRs)

1. installer UI + markers
2. files/open-with integration + launcher refresh + markers
3. selftests + postflight + docs
