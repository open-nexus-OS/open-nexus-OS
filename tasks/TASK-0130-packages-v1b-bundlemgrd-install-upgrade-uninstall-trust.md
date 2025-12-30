---
title: TASK-0130 Packages v1b: bundlemgrd install/upgrade/uninstall for third-party apps + trust policy + registry wiring + tests/markers
status: Draft
owner: @runtime
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NXB format/tooling: tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md
  - State persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Supply-chain sign policy: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - MIME registry: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - App lifecycle/launch: tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`bundlemgrd` already exists in the repo and is part of OS-lite execution flows. Packages v1 extends
it into a user-facing install/upgrade/uninstall path for third-party apps, with honest verification
and deterministic proof.

We must not reintroduce `manifest.json` drift; install operates on `manifest.nxb`.

## Goal

Deliver:

1. `bundlemgrd` install pipeline:
   - `verify(bundleUri)` → stable ok/reason
   - `install(bundleUri, opts)`:
     - signature verification (policy-gated; aligned with `TASK-0029`)
     - manifest validation (schema/version)
     - version rule (SemVer upgrade only; downgrade gated by dev mode)
     - extraction/staging into `/state/apps/<appId>/...` with atomic commit
   - `uninstall(appId)`:
     - stop running instances via `appmgrd`
     - remove app directory and unregister handlers
   - `list/query` for installed apps
2. Storage layout (v1):
   - `/state/apps/<appId>/bundle.nxb/` (canonical bundle dir)
   - optional derived cache (icons) stored alongside and reproducible
3. Trust policy:
   - **single authority** for allowed signing keys/publishers (prefer `keystored` + policy config as per `TASK-0029`)
   - `dev_mode` gate for user-installed dev keys (if supported) and downgrade allowance
4. Registry wiring:
   - register installed apps with `appmgrd` (launch entry)
   - register MIME handlers with `mimed` based on manifest metadata
5. Markers:
   - `bundlemgrd: ready`
   - `bundle: verify ok app=<id>`
   - `bundle: install ok <id>@<ver>`
   - `bundle: uninstall ok <id>`
   - `policy: packages enforce on`

## Non-Goals

- Kernel changes.
- Capability enforcement beyond recording/registration (sandboxing/enforcement is separate work).
- Full multi-asset bundles unless/until NXB contract expands beyond `manifest.nxb` + `payload.elf`.
- Atomic A/B activation per app, SemVer migrations, or licensed entitlement gates (handled by `TASK-0238`/`TASK-0239`).

## Constraints / invariants (hard requirements)

- Atomic install: stage → verify → commit (no partial installs).
- Deterministic error reasons and deterministic markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/pkg_v1_host/`:

- install v1.0.0, upgrade to v1.1.0 succeeds deterministically
- downgrade is rejected unless dev_mode + allowDowngrade are enabled (stable error)
- trust policy allow/deny enforced deterministically
- `mimed` registry reflects installed mime handlers deterministically

### Proof (OS/QEMU) — gated

UART markers:

- `bundlemgrd: ready`
- `SELFTEST: pkg v1 install ok`
- `SELFTEST: pkg v1 launch ok`
- `SELFTEST: pkg v1 uninstall ok`

## Touched paths (allowlist)

- `source/services/bundlemgrd/` (extend)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh` (marker additions)
- `tests/` (host tests)
- `docs/packages/` (in follow-up task or here if minimal)

## Plan (small PRs)

1. bundlemgrd: verify/install/uninstall/list/query + markers
2. trust policy integration (align with `TASK-0029`)
3. host tests + OS selftest markers (gated)
