---
title: TASK-0140 Updates v1 UI/CLI: Settings→Updates page + nx update CLI over the real OTA engine (offline)
status: Done
owner: @ui
created: 2025-12-25
updated: 2026-09-01
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

## DELIVERED 2026-09-01 (honest recuts stated)

Shipped in three packages (P1 CLI · P2 gate+surface+page · P3 proofs+docs).
Ground-truth recuts against the plan below — each one is an honesty fix,
not a scope cut:

1. **No host↔guest transport exists** (exhaustively confirmed: no
   virtio-serial/vsock/guest-agent; UART is read-only; QMP is VM-control).
   So `nx update` is OFFLINE over built artifacts: `status`/`check` decode
   disk truth through the OS's own SSOT crates, `stage` is the §9
   provisioning drop, `switch`/`rollback` are preflights (`applied=false`).
   The DoD's "`SELFTEST: nx update status ok` against the live services"
   became the `verify-nxupdate` harness gate in the `ota-flip` lane: the
   CLI decodes the disk the LIVE machinery just wrote (floor raised,
   commit landed, build id of the flipped slot) — same seam, honest name.
2. **Page lands as `Info › System update`** (the appearance sub-page
   pattern), NOT a 13th sidebar section: the nav pane is deliberately
   unscrollable and sized for twelve rows (0311's framework ownership).
   The dead `inf.update` row became the live entry point.
3. **"Page opens" proof**: headless has no input injection, so the page's
   truth is host-proven (settings conformance suite: real compile+mount,
   service-record rendering, deny state, action→refresh chain — spy
   mirrors the REAL record shape) and the page's live READ surface is
   gated as `SELFTEST: updates surface ok` (status/feed/check coherence
   against the boot authority, state-neutral, before the OTA cycle).
4. **Deny lane**: `updates.manage` did not exist — introduced end to end
   (policyd grant vocabulary, `updated` as enforcement point with
   `policy.delegate`, dedicated wire `STATUS_DENIED`, audit line). The
   sender-spoof-free deny is proven by `test_reject_*` host tests on the
   exact gate fn the loop calls + the DENIED-state rendering test; the
   granted path is exercised live by the whole OTA ladder (selftest holds
   the grant — a broken gate kills `SELFTEST: ota stage ok`).
5. `updated` grew `OP_ROLLBACK` (9) as a gated pass-through (the page's
   rollback request; bundlemgrd active-slot compensation included) and its
   `OP_GET_STATUS` became updated's OWN contract: bootctld prefix PINNED
   at 17 bytes + 8-byte staged-build tail (RFC-0089 §8 amended).

Fallen für Nachfolger: the app-child sender id is NOT a named service —
policyd can never grant it; page actions are honest-denied by design until
a privileged broker exists (follow-up if ever wanted). `svc.updates`
records must mirror `effect_updates.rs` field-for-field in the conformance
spy (the hover_sweep rule).

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
