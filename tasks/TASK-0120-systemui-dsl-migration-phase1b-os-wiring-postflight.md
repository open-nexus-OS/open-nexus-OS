---
title: TASK-0120 SystemUI→DSL Migration Phase 1b: mount in OS + feature flags + nx-dsl systemui targets + selftests/postflight/docs
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Phase 1a pages/tests: tasks/TASK-0119-systemui-dsl-migration-phase1a-launcher-qs-host.md
  - DSL CLI/tooling: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - DSL interpreter/snapshots: tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
  - AOT codegen (optional): tasks/TASK-0079-dsl-v0_3a-aot-codegen-incremental-assets.md
  - Prefs store (feature flag): tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Phase 1a proves the DSL pages on host. Phase 1b wires them into the OS:

- SystemUI mounts DSL Launcher and DSL Quick Settings (with legacy fallback),
- OS selftests assert DSL pages are live and functional,
- a postflight delegates to canonical host tests and QEMU run.

## Goal

Deliver:

1. SystemUI runtime selection:
   - mount DSL Launcher and DSL Quick Settings when `prefsd.systemui.dsl=true`
   - keep legacy implementation behind a feature flag (fallback and troubleshooting)
   - markers:
     - `systemui:dsl launcher on`
     - `systemui:dsl qs on`
2. `nx dsl` integration for SystemUI targets:
   - `nx dsl build --systemui` (interp default)
   - `nx dsl watch --systemui`
   - optional `--aot` wiring once codegen is present
   - profile wiring:
     - SystemUI passes a stable `device.profile` into the DSL runtime (from platform detection; deterministic in QEMU fixtures)
     - host tests and QEMU selftests may force `profile=desktop|tv` via fixtures to keep proofs deterministic
3. OS selftests:
   - wait for mount markers
   - open an app from DSL Launcher and confirm app launch marker
   - toggle dark mode + adjust volume in DSL Quick Settings and confirm state echo
   - markers:
     - `SELFTEST: systemui dsl launcher ok`
     - `SELFTEST: systemui dsl qs ok`
4. Postflight:
   - `tools/postflight-systemui-dsl-phase1.sh` delegates to:
     - `cargo test -p systemui_dsl_phase1_host`
     - bounded QEMU run + marker checks
5. Docs:
   - `docs/systemui/dsl-migration.md` with parity rules, flags, troubleshooting, and how to add pages.

## Non-Goals

- Kernel changes.
- Full SystemUI migration (only Launcher + Quick Settings in Phase 1).

## Constraints / invariants (hard requirements)

- No fake success: OS markers must reflect real mount and real interactions.
- Deterministic markers and bounded selftest timeouts.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Phase 1a host test suite remains green.

### Proof (OS/QEMU) — required once gated deps exist

UART markers:

- `systemui:dsl launcher on`
- `systemui:dsl qs on`
- `SELFTEST: systemui dsl launcher ok`
- `SELFTEST: systemui dsl qs ok`

## Touched paths (allowlist)

- SystemUI mount points (Launcher + Quick Settings)
- `tools/nx-dsl/` (systemui targets)
- `source/apps/selftest-client/`
- `tools/postflight-systemui-dsl-phase1.sh`
- `docs/systemui/dsl-migration.md`

## Plan (small PRs)

1. SystemUI mount wiring + feature flag + markers
2. nx-dsl `--systemui` build/watch targets
3. selftests + postflight + docs
