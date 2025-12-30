---
title: TASK-0239 Installer v1.1b (OS/QEMU): pkgr service (atomic A/B per app) + bundlemgr registry extension + licensed entitlement gates + SystemUI installer + nx-pkg CLI + selftests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Installer core (host-first): tasks/TASK-0238-installer-v1_1a-host-nab-semver-migrations-policy-deterministic.md
  - bundlemgrd install baseline: tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Installer UI baseline: tasks/TASK-0131-packages-v1c-installer-ui-openwith-launcher-integration.md
  - Licensed entitlement enforcement: tasks/TASK-0222-store-v2_2b-os-purchase-flow-entitlements-guard.md
  - State persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU wiring for Installer v1.1:

- `pkgr` service for atomic A/B activation per app (not system-wide),
- `bundlemgr` registry extension (queries, abilityd guards),
- licensed entitlement gates for paid SKUs,
- SystemUI installer flow + Apps page,
- `nx pkg` CLI.

The prompt proposes `pkgr` as a new service, but `bundlemgrd` already exists (`TASK-0130`). This task extends `bundlemgrd` with atomic A/B activation and treats "pkgr" as an internal service name or API surface, not a duplicate authority.

## Goal

On OS/QEMU:

1. **pkgr service** (extends `bundlemgrd`):
   - atomic A/B activation per app:
     - layout: `state:/apps/<appId>/versions/<ver>/` and `current → versions/<ver>` symlink
     - extract to staging, run preflight checks, run migrations, atomically switch `current` symlink
     - on failure → rollback to previous `current`
   - API (`pkgr.capnp`): `verify`, `install`, `upgrade`, `uninstall`, `list`, `info`, `rollback`
   - hooks: notify `bundlemgr` & `abilityd` on activation; warm icon cache
   - markers: `pkgr: ready`, `pkgr: install app=… v=…`, `pkgr: activate current=…`, `pkgr: rollback to=…`, `pkgr: deny entitlement`
2. **bundlemgr registry extension**:
   - stores registry in `state:/pkg/registry.db` (libSQL)
   - source of truth for `abilityd` guards and Settings/Apps list
   - API (`bundle.capnp`): `register`, `unregister`, `query`, `list`
3. **Licensed entitlement gates**:
   - if `sku` present in manifest → query `licensed` for active entitlement
   - deny install/upgrade if entitlement missing/expired/revoked (stable error)
   - markers: `pkgr: deny entitlement app=… sku=…`
4. **SystemUI installer & Apps page**:
   - sideload (developer mode): open `.nxb` via Files → preview manifest, permission diff, abilities, SKU; install progress
   - Apps & Features settings page: list installed apps, version, size; Uninstall, Open Settings, Clear App Data
   - markers: `ui: installer open nxb=…`, `ui: installer install ok`, `ui: app uninstall app=…`
5. **nx pkg CLI** (subcommand of `nx`):
   - `verify`, `install`, `upgrade`, `list`, `info`, `uninstall`, `rollback`
6. **Store/licensing integration**:
   - Store app: after successful purchase → offer Install (calls `pkgr.install`)
   - licensed: on revocation event → `pkgr` marks app inactive (block launch) until entitlement restored
7. **OS selftests + postflight**.

## Non-Goals

- New bundle format (use NXB only; see v1.1a).
- System-wide A/B slots (this is per-app atomic activation only).
- Real network/Store backend (offline fixtures only).

## Constraints / invariants (hard requirements)

- **No duplicate install authority**: `pkgr` extends `bundlemgrd` (`TASK-0130`), not a parallel service. If the prompt suggests a separate `pkgr` service, treat it as an internal API surface or refactoring of `bundlemgrd`.
- **Atomic activation**: stage → verify → migrate → commit (no partial installs).
- **Determinism**: install/upgrade/rollback must be stable given the same inputs.
- **Bounded resources**: migrations are timeout-bounded; quota checks enforce limits.
- **`/state` gating**: persistence is only real when `TASK-0009` exists.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (install authority drift)**:
  - Do not create a parallel `pkgr` service that duplicates `bundlemgrd` install logic. Extend `bundlemgrd` with atomic A/B activation and treat "pkgr" as an internal name or API surface.
- **RED (missing entitlement API)**:
  - If `licensed` cannot provide entitlement verification, this task must first create that API (separate subtask or gate on `TASK-0222`).
- **YELLOW (migration sandboxing)**:
  - Migration scripts run in a sandbox (timeout, resource limits). Document security caveats explicitly.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Installer core: `TASK-0238`
- bundlemgrd baseline: `TASK-0130`
- Licensed enforcement: `TASK-0222`

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `pkgr: ready`
- `pkgr: install app=… v=…`
- `pkgr: activate current=…`
- `pkgr: rollback to=…`
- `pkgr: deny entitlement app=… sku=…`
- `SELFTEST: pkg install notes v1 ok`
- `SELFTEST: pkg upgrade notes v1.1 ok`
- `SELFTEST: pkg paid entitlement ok`
- `SELFTEST: pkg rollback ok`

## Touched paths (allowlist)

- `source/services/bundlemgrd/` (extend: atomic A/B activation, pkgr API surface)
- `source/services/bundlemgr/` (new or extend: registry DB, queries)
- `source/services/licensed/` (extend: entitlement verification hooks)
- `source/services/storemgrd/` (extend: install-after-purchase flow)
- SystemUI (installer flow + Apps page)
- `tools/nx/` (extend: `nx pkg ...` subcommands)
- `source/apps/selftest-client/` (markers)
- `pkg://fixtures/apps/` (sample NXB bundles + dev keys)
- `docs/pkg/migrations.md` (new)
- `docs/tools/nx-pkg.md` (new)
- `tools/postflight-installer-v1_1.sh` (new)

## Plan (small PRs)

1. **pkgr service (bundlemgrd extension)**
   - atomic A/B activation per app
   - preflight checks + migrations + rollback
   - markers

2. **bundlemgr registry**
   - registry DB (libSQL)
   - queries for abilityd/Settings
   - markers

3. **Licensed entitlement gates**
   - entitlement verification hooks
   - deny install/upgrade if missing/expired/revoked
   - markers

4. **SystemUI + integrations + CLI + selftests**
   - installer flow + Apps page
   - Store install-after-purchase
   - nx pkg CLI
   - OS selftests + postflight

## Acceptance criteria (behavioral)

- `pkgr` manages atomic A/B activation correctly.
- `bundlemgr` registry provides queries for abilityd/Settings.
- Licensed entitlement gates deny install/upgrade correctly.
- SystemUI installer flow works.
- All four OS selftest markers are emitted.
