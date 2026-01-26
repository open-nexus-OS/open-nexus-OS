---
title: TASK-0181 Store v1b (OS/QEMU): Storefront UI + install/update/remove via storemgrd/bundlemgrd + ratings persistence + policy caps + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Store v1a core services/tests: tasks/TASK-0180-store-v1a-host-storefeedd-storemgrd-ratings.md
  - Packages install authority (bundlemgrd): tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Installer UI wiring (optional): tasks/TASK-0131-packages-v1c-installer-ui-openwith-launcher-integration.md
  - Trust store unification (signatures): tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Policy capability matrix (apps): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - L10n/font fallback (UI prerequisites): tasks/TASK-0175-l10n-i18n-v1b-os-locale-switch-settings-cli-selftests.md
  - Search v2 integration (optional): tasks/TASK-0152-search-v2-ui-os-deeplinks-selftests-postflight-docs.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With `storefeedd` + `storemgrd` core behavior proven host-first (`TASK-0180`), we need OS integration:

- a Storefront app UI (browse/search/details/install/update/remove),
- trust/signature surfacing,
- ratings persistence (local-only),
- policy gates,
- and QEMU selftests/markers.

## Goal

Deliver:

1. Storefront app (`userspace/apps/storefront`):
   - Home (categories/featured), Search, Details, Installed, Updates
   - actions: install/update/remove/open
   - shows signer badge (label from trust store)
   - ratings stub UI:
     - write rating (overwrite allowed)
     - display average + count (local-only)
   - a11y roles and i18n string usage where available
   - markers:
     - `storefront: open`
     - `storefront: details app=<id>`
     - `storefront: install click app=<id>`
     - `storefront: rating save app=<id> stars=<n>`
2. OS services wiring:
   - ensure `storefeedd` and `storemgrd` are started in OS graph
   - `storemgrd` drives `bundlemgrd` for actual install/update/remove
3. Policy caps:
   - `store.feed.read`
   - `store.install` / `store.remove` (or reuse `catalog.install/catalog.remove` if already standardized)
   - `store.rate.write`
   - enforce via `policyd.require(...)` (system-only default)
4. Fixtures:
   - deterministic `pkg://store/feed.nxf` (Cap'n Proto; canonical), icons, and signed bundles (or references to existing fixtures)
     - optional authoring/derived view: `pkg://store/feed.json` (not canonical)
5. OS selftests (bounded):
   - wait for readiness: `storefeedd: ready`, `storemgrd: ready`, `bundlemgrd: ready`
   - install app from feed and verify it appears as installed
   - update path (if feed includes a higher SemVer)
   - remove app and verify it is gone
   - ratings write/read-back (only if `/state` exists; otherwise explicit placeholder)
   - required markers:
     - `SELFTEST: store feed ok`
     - `SELFTEST: store install ok`
     - `SELFTEST: store update ok`
     - `SELFTEST: store remove ok`
     - `SELFTEST: store rating ok` (only if `/state` exists; otherwise explicit placeholder)
6. CLI `nx-store`:
   - host tool and/or OS tool depending on existing `nx` strategy
   - NOTE: do not rely on running host CLIs inside QEMU selftests
7. Docs:
   - architecture and offline feed format
   - signature/trust model and policy caps
   - ratings stub limitations
   - testing and marker contract

## Non-Goals

- Kernel changes.
- Online store and networking.
- Cross-device reviews/ratings.

## Constraints / invariants (hard requirements)

- Offline & deterministic: all app metadata and bundles are `pkg://...` fixtures.
- No fake success: install/update/remove markers only after real bundlemgrd outcomes.
- `/state` gating:
  - ratings persistence and installed DB are only “real” once `/state` exists (`TASK-0009`).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (depends on packages)**:
  - Store v1 cannot be proven in QEMU until `bundlemgrd` install/uninstall is real (`TASK-0130`) and `/state` exists (`TASK-0009`).

- **YELLOW (search integration)**:
  - Indexing store apps into search should wait for Search v2 OS integration (`TASK-0152`). Keep it optional in v1b.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p store_v1_host -- --nocapture` (from v1a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=195s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: store feed ok`
    - `SELFTEST: store install ok`
    - `SELFTEST: store update ok`
    - `SELFTEST: store remove ok`
    - `SELFTEST: store rating ok` (only if `/state` exists; otherwise explicit placeholder)

## Touched paths (allowlist)

- `userspace/apps/storefront/` (new)
- `source/services/storefeedd/` + `source/services/storemgrd/`
- `source/services/bundlemgrd/` (integration use only)
- `source/apps/selftest-client/`
- `pkg://store/` fixtures (exact repo path to be chosen)
- `docs/store/` + `docs/tools/nx-store.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Storefront UI skeleton + feed browsing/search/details
2. install/update/remove wiring + progress UI + markers
3. ratings persistence wiring + quotas/policy + markers
4. OS selftests + docs + marker contract update

## Acceptance criteria (behavioral)

- In QEMU (when unblocked), Storefront can install/update/remove a signed local bundle from the offline feed, and ratings stub works when `/state` exists.
