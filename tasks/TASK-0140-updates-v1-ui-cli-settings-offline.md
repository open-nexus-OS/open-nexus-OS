---
title: TASK-0140 Updates v1 UI/CLI: Settings→Updates page + nx update CLI over the real OTA engine (offline)
status: Draft
owner: @ui
created: 2025-12-25
updated: 2026-08-25
depends-on:
  - TASK-0179   # the engine these surfaces drive
follow-up-tasks: []
links:
  - Contract: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md (§8 updated v2, §9 feed)
  - DSL syntax/layout convention: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - DSL app integration kit: tasks/TASK-0122C-dsl-app-integration-kit-v1-picker-clipboard-share-print.md
  - Settings tree baseline: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - Policy gates (updates.manage): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## REWRITE 2026-08-25 (RFC-0089 lane recut — supersedes the pre-rewrite body)

The old body linked the Superseded TASK-0178 as a live bootctld stub, referenced
`bootctl.json`/healthmux/soft-reboot, planned a separate `tools/nx-update/`
binary (forbidden — single `nx` entrypoint per TRACK-AUTHORITY-NAMING), and
re-required already-gated markers (`SELFTEST: ota stage/switch/rollback ok`) —
the exact duplicate-marker trap 0036's rebase warned about. All gone. These
surfaces now drive the REAL engine: `updated` v2 (`OP_STAGE_SOURCE`, feed ops)
and bootctld status — no update logic of their own.

**Collision note (in-flight UI tracks)**: the design-handoff tracks (0305–0313)
own the Settings shell/framework. This task lands LAST in the lane and owns ONLY
the Updates page content + its route/registry entry — no shared framework churn.

## Context

After TASK-0179 the OTA engine is real and QEMU-proven, but only the selftest
drives it. Users and developers need honest surfaces: a Settings page showing
slot/trial/version truth, and a scriptable CLI.

## Goal

1. **Settings → Updates (DSL page)** in the canonical Settings tree (Store +
   Event + reduce + @effect + Page shape; picker via the shared integration kit):
   - shows: active slot, staged build (id + rollback index), pending/trial state,
     tries left, floor, current build id — all read from `updated`/bootctld
     status ops (never derived client-side).
   - actions (policy-gated `updates.manage`, system-only default): check feed,
     stage a listed container, schedule switch, commit-after-quorum status view,
     rollback request. Buttons reflect REAL state transitions (no fake progress).
2. **`nx update` CLI** (subcommand of the canonical `nx` tool — no new binary):
   `nx update check | stage <source> | switch | status | rollback`; stable
   parseable output lines; nx exit classes stay the CLI contract.
3. Docs: `docs/updates/` workflow (UI + CLI), including the honest boundary:
   commit happens via health quorum, not via a button.

## Non-Goals

- Any update logic in UI/CLI (verify/stage/apply live in `updated`; state in
  bootctld). Network fetch. New payload formats. Kernel changes.

## Constraints / invariants (hard requirements)

- Offline-only sources (`/data/updates/`, `pkg://updates/`).
- Deny-by-default: without `updates.manage` the page is read-only and the CLI
  mutating verbs fail with the stable policy reject.
- No fake success; no `unwrap/expect`; stable marker strings.

## Stop conditions (Definition of Done)

### Proof (Host)

- CLI golden tests (argument parsing, stable output, exit classes).
- Page store/reduce host tests per the DSL testing convention.

### Proof (OS/QEMU) — headless ladder additions

- `SELFTEST: settings updates page ok` (page opens, status fields match
  bootctld record truth).
- `SELFTEST: nx update status ok` (CLI status against the live services).
- Deny lane: mutating verb without grant ⇒ stable reject marker.

## Touched paths (allowlist)

- Settings DSL tree (Updates page + route/registry entry only)
- `tools/nx/` (update subcommands)
- `source/apps/selftest-client/` + proof-manifest markers
- `docs/updates/`, `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. `nx update` CLI + goldens (host-first, engine already proven).
2. DSL page + bridge adapters + host tests.
3. OS wiring + selftests + docs.
