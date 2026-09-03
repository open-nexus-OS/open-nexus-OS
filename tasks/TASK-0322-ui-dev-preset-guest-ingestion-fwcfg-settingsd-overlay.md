---
title: TASK-0322 UI dev presets, guest side: fw_cfg `ui-preset` key → settingsd default overlay → `systemui: profile …` marker + `SELFTEST: ui preset boot ok` (+ mode-driven pacer Hz)
status: Draft
owner: @ui @runtime
created: 2026-09-03
updated: 2026-09-03
size: M
depends-on:
  - TASK-0055D
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Host-side preset authority (shipped): tasks/TASK-0055D-ui-v1e-dev-display-profile-presets-qemu-hz.md
  - Preset resolver: source/services/systemui/src/preset.rs
  - Display-mode SSOT (fw_cfg): docs/adr/0050-display-mode-authority.md
  - Shell-mode authority (settingsd `ui.shell.mode`): source/services/settingsd/src/registry.rs
  - fw_cfg reader precedent (selftest-client only): source/apps/selftest-client/src/os_lite/boot_cfg.rs
  - init fw_cfg grant (selftest-client only): source/init/nexus-init/src/bootstrap/orchestrator.rs
  - Profiles guidance: docs/dev/ui/foundations/layout/profiles.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

TASK-0055D shipped the preset catalog + validation + launcher plumbing on the
HOST: `just start-preset <id>` boots the right display mode (fw_cfg
`display-mode`, followed end-to-end) with the right emulated input. Two
preset axes do not reach the guest yet, and the ledger recorded them as
honest recuts rather than faking markers:

1. **Profile/shell**: on the device the shell follows the boot product
   (`tablet`) and settingsd's `ui.shell.mode` (default `tablet`). A `laptop`
   preset exports `NEXUS_UI_PRESET_SHELL=desktop` but the guest boots the
   tablet shell. Only `selftest-client` (cap slot 0x31, granted by init) and
   the kernel read fw_cfg today; settingsd has no fw_cfg access.
2. **Hz**: windowd paces every mode at a single 120 Hz constant
   (`PACER_INTERVAL_NS`); the preset validator therefore accepts only 120.

## Goal

A preset boot is observable IN the guest: `just start-preset laptop` boots
the desktop shell and prints `systemui: profile desktop orient=landscape
shell=desktop`, `windowd: preset on (name=laptop)`, and — in a gated
selftest lane on the baseline preset — `SELFTEST: ui preset boot ok`. The
convertible proof path prints `systemui: shell mode -> desktop|tablet` on
the runtime toggle.

## Non-Goals

- A second shell-mode store or a dev-only fake env path: the preset seeds
  settingsd's DEFAULT for `ui.shell.mode` (and only when no persisted value
  exists); settingsd stays the authority, the Control Center toggle stays
  the runtime switch.
- New kernel syscalls or ABI: prefer a userspace fw_cfg read with an
  init-granted cap (the selftest-client pattern) over extending
  `SYSCALL_BOOT_DISPLAY_MODE`.
- Resolutions above the 1280×800 layout maximum (separate atlas/VMO work).

## Constraints / invariants

- Identity from `sender_service_id`; the fw_cfg string is bounded (preset
  ids are `[a-z0-9-]{1,32}`) and validated against the SAME registry
  (`systemui::resolve_preset`) before anything is applied.
- Unknown/invalid preset in fw_cfg ⇒ deterministic `windowd: preset
  rejected (name=…)` marker and the plain default boot — never a partial
  apply.
- Marker contract: new markers land in `scripts/qemu-test.sh` +
  `tools/nx/chains/markers.txt` + docs together; the baseline literal
  `windowd: ready (w=1280, h=800, hz=120)` is unchanged.
- No fake success: `hz=<n>` in the ready marker prints the pacer's REAL
  interval; `SUPPORTED_DISPLAY_HZ` only widens once the pacer is mode-driven.

## Plan (small PRs)

- **P1 — settings default overlay**: launcher writes
  `-fw_cfg name=opt/org.open-nexus/ui-preset,string=<id>` from
  `NEXUS_UI_PRESET`; init grants the fw_cfg window to settingsd (a second
  `FW_CFG_DST_SLOT` transfer alongside selftest-client); settingsd reads the
  key at boot, resolves the preset via the registry, and seeds the
  `ui.shell.mode` DEFAULT (persisted value wins). Host tests: overlay
  precedence, `test_reject_overlay_unknown_preset`,
  `test_reject_overlay_unknown_key`.
- **P2 — guest markers**: windowd prints `windowd: preset on (name=…)` and
  `systemui: profile <p> orient=<o> shell=<s>` from the resolved config;
  the convertible toggle prints `systemui: shell mode -> …`.
- **P3 — proof lane**: `SELFTEST: ui preset boot ok` on the baseline preset
  (headless/smp1: preset key present + resolved config matches the
  catalog); a `just test-os` preset flavour for one non-baseline preset.
- **P4 (optional) — mode-driven pacer**: fw_cfg `display-mode` grows an
  optional `@<hz>` suffix (ADR-0050 amendment, kernel parse is one
  approval-zone change); windowd derives `PACER_INTERVAL_NS` from it;
  `SUPPORTED_DISPLAY_HZ` widens to `[60, 120]`.

## Stop conditions (Definition of Done)

- Host: overlay + reject tests green; catalog tests unchanged.
- OS/QEMU: the three markers above in one uart for `just start-preset
  laptop`; `SELFTEST: ui preset boot ok` gated in headless/smp1.
- Docs: profiles.md "honest limits" shrinks accordingly; CHANGELOG; board.

## Red flags / decision points

- **RED**: fw_cfg cap grant to a second service touches the init ctrl-plane
  (historically hazardous ordering — see the TASK-0315 fixed-slot finding).
  Do it as a separate post-pass transfer, never inside the spawn-time slot
  distribution.
- **YELLOW**: settingsd reading fw_cfg before its statefs restore must not
  delay `settingsd: ready`; bound the read (one page, one file) and fall
  back to the code default on any failure.
