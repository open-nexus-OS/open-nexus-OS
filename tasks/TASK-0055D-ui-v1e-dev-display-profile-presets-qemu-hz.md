---
title: TASK-0055D UI v1e (dev-mode): deterministic display/profile presets for QEMU (`phone/tablet/laptop/laptop-pro/convertible` + orientation + shell mode + Hz)
status: Done
updated: 2026-09-03
owner: @ui @runtime
created: 2026-03-29
depends-on: []
follow-up-tasks:
  - TASK-0322
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - UI compositor baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Visible scanout bootstrap: tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md
  - Visible present baseline: tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md
  - DSL profile/runtime contract: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - SystemUI DSL OS wiring baseline: tasks/TASK-0120-systemui-dsl-migration-phase1b-os-wiring-postflight.md
  - Config/schema broker: tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Settings/config bridge note: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - UI profiles guidance: docs/dev/ui/foundations/layout/profiles.md
  - Testing contract: scripts/qemu-test.sh
  - Guest-side ingestion follow-up: tasks/TASK-0322-ui-dev-preset-guest-ingestion-fwcfg-settingsd-overlay.md
---

## DELIVERED 2026-09-03 (host authority + launcher plumbing; honest recuts stated)

**Shipped (test-all green):**

- **Preset catalog as manifests** — `source/services/systemui/manifests/presets/<id>/preset.toml`
  ×7 (`phone-portrait`, `phone-landscape`, `tablet-portrait`, `tablet-landscape`, `laptop`,
  `laptop-pro`, `convertible`), registered in `systemui::PRESETS` next to profiles/shells/
  products (ADR-0035 amendment). Shape: registered `profile` + `shell`, `[display]`
  width/height/hz/orientation/dpi_class, `[input]` touch/mouse/kbd/remote/rotary.
- **Resolver + bounded validation** — `systemui::preset` (parse via the profile manifests'
  TOML subset) + `registry::resolve_preset` / `switch_preset_shell`: unknown preset/profile/
  shell → `ManifestNotFound`; disallowed pairing → `UnsupportedShell`/`IncompatibleShell`;
  mode outside `[320, 1280×800]` or contradicting orientation → `InvalidDisplayMode`;
  non-pace-able Hz → `UnsupportedRefreshRate`. `device.sizeClass` derived from the preset
  width with the runtime's tiers. 13 host tests incl. `test_reject_*` ×7 and the
  convertible tablet↔desktop switch (same preset/profile/mode, reversible).
- **Developer selection path** — `nx ui preset list|env <id>` (new `tools/nx` subcommand,
  `systemui` as host dependency = ONE parser) renders the launcher env `just start` already
  honours: `QEMU_GPU_XRES/YRES` (→ fw_cfg `display-mode`, RFC-0074/ADR-0050 — followed
  end-to-end by gpud + windowd), `QEMU_PROOF_POINTER_SOURCE`, `NEXUS_PROFILE_INPUT_*`,
  `NEXUS_UI_PRESET_*`. `just start-preset <id>` / `just preset-list`. Unknown preset =
  `validation_reject` (exit 3) naming the valid ids, BEFORE any build/launch. 4
  process-boundary tests (`tools/nx/tests/ui_preset_cli.rs`).
- **windowd reports the real mode** — `markers::ready_marker(w, h)` /
  `display_mode_marker(w, h)` print `windowd: ready (w=<w>, h=<h>, hz=120)` and
  `display: mode <w>x<h> argb8888` for the gpud-resolved mode (both were literals that
  claimed 1280×800 in every boot); byte-identical to the ladder literals at the baseline
  (host test), `w=600, h=800` / `600x800` on the tablet-portrait boot.
- Retired `tools/systemui_profile_qemu_devices.py` (zero callers; parallel parser).

**Honest recuts (→ TASK-0322):**

- Hz is host-validated only: windowd paces every mode at 120 Hz (`PACER_INTERVAL_NS`), so
  `SUPPORTED_DISPLAY_HZ = [120]` and a 60 Hz preset is REJECTED rather than faked.
- Mode ceiling 1280×800 (shared atlas/VMO layout max; gpud clamps larger fw_cfg modes).
- The guest's profile/shell still follow the boot product + settingsd `ui.shell.mode`
  (only selftest-client/kernel read fw_cfg; settingsd has no cap). The OS markers this
  ledger listed (`windowd: preset on`, `systemui: profile …`, `systemui: shell mode ->`,
  `SELFTEST: ui preset boot ok`) are therefore NOT emitted — they move to TASK-0322 with
  the fw_cfg `ui-preset` key + settingsd default overlay.
- `phone`/`laptop` guest profiles do not exist; phone presets use the `tablet` profile,
  laptop presets `desktop`.

**Proof:** `cargo test -p systemui` (31), `cargo test -p windowd` (169 + ready-marker
test), `cargo test -p nx --test ui_preset_cli` (4); `just check` green; preset boot
`NEXUS_LOG_EXPAND=windowd QEMU_DISPLAY_BACKEND=egl-headless RUN_TIMEOUT=100 just start-preset
tablet-portrait` (uart, 2026-09-03): `windowd: display mode 600x800` → `windowd: shell config
product=tablet profile=tablet shell=tablet` → `windowd: ready (w=600, h=800, hz=120)` →
`display: mode 600x800 argb8888` → `inputd: display mode 600x800`; `SELFTEST: ui visible present
ok` on the 600×800 scanout. (`SELFTEST: qos FAIL` in that interactive headless-GL boot is a
pre-existing pattern — present in 215 earlier logs — unrelated to the mode.) `just test-all`:
all gates green through `ci-os-reset`; `ci-os-ota` (ota-flip) hit the known multi-boot
`SELFTEST: statefs enc roundtrip FAIL` variance once (10 historical uarts, all reset/flip
lanes) and passed on isolated re-run (`verify-uart ok`, `verify-nxupdate ok`);
`ci-os-ota-backstops` green (tamper/downgrade/fallback). Baseline ladder literal unchanged.

## Rebase (2026-08-14)

### Shipped since draft — do NOT re-implement

- **Per-profile TOML manifests exist**:
  `source/services/systemui/manifests/profiles/{desktop,tablet}/profile.toml`
  with `[input]` and `[display_defaults]` sections, consumed by
  `tools/systemui_profile_qemu_devices.py` for QEMU device wiring.
- **`device.profile` / `device.shellMode` / `device.sizeClass` are a live DSL
  env axis** (`userspace/dsl/core/src/registry.rs:570-575`;
  `source/services/app-host/src/probe/env.rs:72-88` derives `sizeClass` from
  the REAL surface width), with re-emit on change.
- **`ui.shell.mode` is a persisted settingsd key**
  (`source/libs/nexus-wire/src/settingsd.rs:68`, default `"tablet"`).

### Honest residual scope

1. **Preset catalog**: `phone-portrait`, `phone-landscape`, `tablet-portrait`,
   `tablet-landscape`, `laptop`, `laptop-pro`, `convertible` — today only
   `desktop` and `tablet` manifests exist.
2. **Schema validation** for the manifests (bounded schema, actionable
   diagnostics — today ad-hoc parsing).
3. **Decoupling resolution/Hz**: today one mode is hardcoded end-to-end —
   `scripts/qemu-test.sh` asserts the literal marker
   `windowd: ready (w=1280, h=800, hz=120)`. Presets that vary w/h/hz must
   turn that assertion into a per-preset expectation (marker contract:
   `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt` + docs together).

### Constraint added by the rebase — no parallel authority

Presets resolve INTO the existing authorities: the settingsd `ui.shell.mode`
key and the systemui profile-manifest model. Do not introduce a second
profile store, a second shell-mode source, or a dev-only fake env path.

### Corrected touched paths

The draft's `tools/nx-dsl/` is dead (real tool home: `tools/nx/`), and the
manifest home is `source/services/systemui/manifests/profiles/`, not
`ui/profiles/`/`ui/shells/`/`schemas/`. The allowlist below is updated;
`scripts/**` and `config/**` are approval zones.

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

## Touched paths (allowlist) — corrected 2026-08-14

- `source/services/systemui/manifests/profiles/` (preset manifests — extend the existing model)
- `tools/systemui_profile_qemu_devices.py` (preset → QEMU device resolution)
- `tools/nx/` (dev tooling entrypoints, if needed)
- `scripts/qemu-test.sh` (per-preset marker expectations — approval zone `scripts/**`)
- `source/services/windowd/` (mode plumbing only)
- `source/services/app-host/src/probe/env.rs` (env axis wiring, if extended)
- `source/apps/selftest-client/`
- `docs/dev/ui/foundations/layout/profiles.md`
- `docs/dev/ui/foundations/quality/testing.md`
- `docs/dev/ui/dsl-migration.md`

## Plan (small PRs)

1. define TOML preset manifest shape + canonical preset names
2. wire schema validation + preset resolution into QEMU/dev-mode launch path
3. pass resolved profile/orientation/shell/display values into SystemUI + DSL runtime
4. add `convertible` shell-mode switching proof path
5. add host/QEMU fixtures + docs
