---
title: TASK-0055D UI v1e (dev-mode): deterministic display/profile presets for QEMU (`phone/tablet/laptop/laptop-pro/convertible` + orientation + shell mode + Hz)
status: Draft
owner: @ui @runtime
created: 2026-03-29
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI compositor baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Visible scanout bootstrap: tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md
  - Visible present baseline: tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md
  - DSL profile/runtime contract: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - SystemUI DSL OS wiring baseline: tasks/TASK-0120-systemui-dsl-migration-phase1b-os-wiring-postflight.md
  - Config/schema broker: tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Settings/config bridge note: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - UI profiles guidance: docs/dev/ui/foundations/layout/profiles.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want early UI work to exercise the same cross-device/profile logic that apps and SystemUI will rely on later.
Today the roadmap already points toward:

- `ui.profile` and display-dimension config in early compositor bring-up,
- DSL/runtime profile overrides,
- SystemUI passing `device.profile` into the DSL runtime.

What is still missing is a deterministic **developer-facing display/profile preset** story for QEMU so we can test:

- phone vs tablet vs laptop shell posture,
- portrait vs landscape,
- refresh-rate-sensitive pacing,
- and responsive/base-vs-platform-override behavior

without inventing ad-hoc local launch arguments per developer.

We also want this path to become the long-term extension model for forks and products:

- human-editable TOML manifests, not scattered hardcoded switches,
- strict schema validation, not free-form strings,
- and deterministic resolution from preset/product choice into runtime `device.profile` / `device.shellMode`.

## Goal

Deliver a bounded dev-mode preset mechanism for QEMU/UI bring-up:

1. **Deterministic preset catalog**:
   - stored as small TOML manifests (one preset per file; no monolithic config blob)
   - `phone-portrait`
   - `phone-landscape`
   - `tablet-portrait`
   - `tablet-landscape`
   - `laptop`
   - `laptop-pro`
   - `convertible`
2. **Each preset defines a stable bundle**:
   - resolved profile ID
   - orientation
   - resolved shell ID / `device.shellMode`
   - width/height
   - `ui.display.hz`
   - scale / dpi class
   - input flags (`touch`, `mouse`, `kbd`, etc.)
   - references to registered profile/shell manifests rather than free-form ad-hoc strings
3. **Developer selection path**:
   - dev-mode preset selection via config / CLI / bounded startup selector
   - deterministic in host tests and QEMU selftests
4. **SystemUI integration**:
   - selected preset feeds the same runtime/env contract seen by DSL and SystemUI
   - no separate “dev-only fake profile” path
   - `convertible` can switch shell posture at runtime (`desktop` <-> `tablet`) without pretending to be a different hardware profile
5. **Manifest model**:
   - authoring format: TOML
   - schema validation before runtime use
   - optional canonical/compiled artifact later, but TOML remains the authoring path

## Non-Goals

- End-user boot picker for production devices.
- A full EDID/monitor negotiation stack.
- Arbitrary custom resolutions/hz input without bounds.
- Replacing later real hardware mode detection.

## Constraints / invariants (hard requirements)

- Presets must be deterministic and versioned/documented.
- Preset TOML must validate against a bounded schema with actionable diagnostics.
- Preset selection must not create a second SystemUI or DSL runtime path.
- Width/height/hz/profile/input mapping must remain bounded and testable.
- `shellMode` changes must be explicit, deterministic, and reversible.
- Default QEMU proofs should still have a canonical baseline preset.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- host fixtures can force each preset deterministically,
- invalid manifest shape, unknown profile ID, or unknown shell ID reject deterministically,
- DSL/SystemUI profile-dependent snapshots differ only in intended, documented ways,
- `convertible` shell-mode switching is deterministic and preserves the same device identity,
- invalid preset names or incompatible values reject deterministically.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: preset on (name=...)`
- `windowd: mode <w>x<h>@<hz>`
- `systemui: profile <profile> orient=<portrait|landscape> shell=<...>`
- `systemui: shell mode -> desktop|tablet` (for `convertible` proof path)
- `SELFTEST: ui preset boot ok`

## Touched paths (allowlist)

- `source/services/windowd/`
- SystemUI runtime/profile detection wiring
- `tools/nx-dsl/` or QEMU/dev tooling entrypoints
- `source/apps/selftest-client/`
- `ui/profiles/` / `ui/shells/` / `ui/products/` or equivalent manifest directories
- `schemas/` (preset/profile/shell manifest schema, if adopted under config/schema infra)
- `docs/dev/ui/foundations/layout/profiles.md`
- `docs/dev/ui/foundations/quality/testing.md`
- `docs/systemui/dsl-migration.md`

## Plan (small PRs)

1. define TOML preset manifest shape + canonical preset names
2. wire schema validation + preset resolution into QEMU/dev-mode launch path
3. pass resolved profile/orientation/shell/display values into SystemUI + DSL runtime
4. add `convertible` shell-mode switching proof path
5. add host/QEMU fixtures + docs
