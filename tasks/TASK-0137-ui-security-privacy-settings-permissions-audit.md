---
title: TASK-0137 Security & Privacy UI v1: permissions matrix editor + audit viewer + installer approvals wiring (DSL, host-first, OS-gated)
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - Policy capability matrix: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - SystemUI→DSL Settings baseline: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - SystemUI→DSL OS wiring: tasks/TASK-0122-systemui-dsl-migration-phase2b-os-wiring-postflight-docs.md
  - Installer UI baseline: tasks/TASK-0131-packages-v1c-installer-ui-openwith-launcher-integration.md
  - Runtime perms/privacy indicators: tasks/TASK-0103-ui-v17a-permissions-privacyd.md
---

## Context

Once we have an app capability matrix (`TASK-0136`), users need a first-class UI to:

- review granted capabilities per app,
- toggle grants (persist),
- and inspect recent allow/deny decisions (audit trail).

Installer must seed grants from bundle-declared permissions (record + approve flow) without duplicating policy authority.

Scope note:

- Policy v1.1 adds scoped grants/expiry/runtime prompts and a richer Privacy Dashboard. That work is tracked as `TASK-0168`
  and should extend this UI rather than creating a parallel Settings surface.

## Goal

Deliver:

1. Settings → Security & Privacy (DSL) page:
   - Permissions tab:
     - per-app list of granted capabilities with toggles
     - badges for “foreground-only” caps (informational)
     - “Reset to defaults” (reapply installer baseline)
     - search/filter by app/capability
   - Activity/Audit tab:
     - tail last N audit events with filters
     - clear log action (admin-gated if needed)
   - markers:
     - `settings:security open`
     - `settings:perm toggle app=<id> cap=<cap> on=<bool>`
     - `settings:audit clear`
2. Installer integration:
   - parse bundle-declared permissions (source-of-truth location defined by packaging tasks)
   - present checkboxes in Installer UI
   - on install: call `policyd.grant(appId, cap, persist=true)` for approved caps
   - marker: `installer: caps approved n=<n>`
3. Host tests:
   - deterministic rendering snapshots for Security & Privacy page (light/dark/HC)
   - interaction tests: toggling a cap calls bridge/policyd mock and updates UI state
   - audit list renders stable rows from deterministic mock stream

## Non-Goals

- Kernel changes.
- Full user identity (multi-user) model.
- A sophisticated admin/auth model (v1 can treat “system apps” as admin; must be explicit).

## Constraints / invariants (hard requirements)

- Deterministic snapshots and deterministic mocks.
- A11y labels/roles for all toggles and list rows.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (suggested: `tests/security_privacy_ui_host/`):

- snapshots for Security & Privacy pages match goldens
- toggling a permission calls mocked `policyd` and state reflects roundtrip reads
- audit tail list stable and filterable deterministically

### Proof (OS/QEMU) — gated

Once policyd + audit sink are present in QEMU:

- Security & Privacy page opens and emits markers
- toggling a permission changes effective grants (verified via policyd query)

## Touched paths (allowlist)

- `userspace/systemui/dsl/pages/settings/SecurityPrivacy.nx` (new)
- `userspace/systemui/dsl_bridge/` (extend: policyd + audit stream adapters)
- `userspace/apps/installer/` (cap approvals wiring)
- `tests/`
- `docs/security/policy-overview.md` (UI section) and/or `docs/systemui/settings.md`

## Plan (small PRs)

1. DSL page + bridge adapters + markers
2. installer approval wiring + markers
3. host snapshots + interaction tests + docs; OS markers once gated deps exist
