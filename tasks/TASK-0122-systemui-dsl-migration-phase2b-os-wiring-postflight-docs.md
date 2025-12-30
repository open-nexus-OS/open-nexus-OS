---
title: TASK-0122 SystemUI→DSL Migration Phase 2b: OS wiring for Settings/Notifs + feature flags + selftests/postflight + docs
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Phase 2a pages/tests: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - DSL CLI/tooling: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - DSL interpreter baseline: tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
  - Prefs store (feature flags): tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Phase 2a delivered DSL Settings and read-only Notifications Center plus host proofs.
Phase 2b wires them into the OS:

- SystemUI routes mount DSL pages when enabled,
- legacy UI remains behind feature flags,
- OS selftests validate behavior,
- postflight delegates to canonical host tests + QEMU marker run,
- docs updated with Phase 2 scope and parity rules.

## Goal

Deliver:

1. SystemUI mount + routing:
   - mount DSL Settings and DSL Notifications Center when `prefsd.systemui.dsl=true` (default true)
   - keep legacy behind features:
     - `legacy_systemui_settings`
     - `legacy_systemui_notifications`
   - markers:
     - `systemui:dsl settings on`
     - `systemui:dsl notifs on`
     - `systemui:dsl swap settings`
     - `systemui:dsl swap notifs`
2. OS selftests:
   - wait for bridge init + mount markers
   - settings flow: toggle dark mode and high contrast then restore; adjust volume then restore
     - `SELFTEST: systemui dsl settings ok`
   - notifs flow: enqueue notifications via notifd test hook; open notif center; assert list marker
     - `SELFTEST: systemui dsl notifs ok`
3. Postflight:
   - `tools/postflight-systemui-dsl-phase2.sh` delegates to:
     - `cargo test -p systemui_dsl_phase2_host`
     - bounded QEMU run + grep for OS markers
4. Config/policy:
   - add systemui settings/notifs defaults (`read_only=true`) and enforce read-only in DSL view
5. Docs:
   - extend `docs/systemui/dsl-migration.md` with Phase 2
   - add `docs/systemui/settings.md` and `docs/systemui/notifications.md`
   - update `docs/dsl/cli.md` for `--systemui` targets

## Non-Goals

- Kernel changes.
- Making Notifications Center actionable in Phase 2 (read-only only).

## Constraints / invariants (hard requirements)

- No fake success markers; must reflect real mounts and real interactions.
- Deterministic OS selftests; bounded timeouts.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required once gated deps exist

UART markers:

- `systemui:dsl settings on`
- `systemui:dsl notifs on`
- `SELFTEST: systemui dsl settings ok`
- `SELFTEST: systemui dsl notifs ok`

## Touched paths (allowlist)

- SystemUI routing/mount points (Settings + Notifications)
- `source/apps/selftest-client/`
- `tools/postflight-systemui-dsl-phase2.sh`
- `docs/systemui/dsl-migration.md`
- `docs/systemui/settings.md`
- `docs/systemui/notifications.md`

## Plan (small PRs)

1. OS routing/mount wiring + markers
2. selftests + postflight
3. config/policy defaults + docs

