---
title: TASK-0068 UI v7c: screenshot (screencapd) + share-sheet broker + privacy/policy guards
status: Draft
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - ADR: docs/adr/0022-modern-image-formats-avif-webp.md
  - UI v4a compositor baseline (readback): tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - UI v6a WM baseline (grab window): tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - Clipboard v2 (destination): tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - DSoftBus (peer share, optional): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Policy as Code (consent/limits): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Persistence (/state save): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Rebase (2026-08-14) — capture-only

### Verified reality

**Zero code exists.** A grep for `screencap|screenshot` across `source/`,
`userspace/`, and `tools/` finds only host-side QEMU proof tooling. Everything
in this ledger is greenfield — nothing to re-implement, but nothing to lean on
either.

### Scope cut — the share half moves out

The Non-Goals already point there: the intent-based share pipeline is
TASK-0126/0127/0128. This rebase completes the cut:

- **`sharesheetd` is removed from this ledger** (it was "name TBD" anyway).
- The share-sheet UI is a **DSL app surface** (in the mold of settings'
  `userspace/apps/settings/ui/components/chrome/PickerSheet.nx`), NOT a
  "SystemUI overlay". Per the boundary SSOT
  (`docs/dev/ui/windowd-cleanup-map.md:4-9`), shell UI belongs to the DSL
  shell app and widgets — never windowd, never a bespoke overlay service.

What remains here is the **capture substrate**:

1. `screencapd` service: `grabDisplay` / `grabWindow` / `grabRegion` →
   VMO + metadata (w/h/stride).
2. A bounded **windowd readback API** that screencapd consumes — windowd
   exposes readback of the last composed buffer and nothing more: no capture
   UI, no consent UI, no export logic in windowd.
3. **Consent model + pixel/byte caps** (fail-closed, policy-guarded via
   policyd).

### Dependency kept

TASK-0105 (screen recorder / capture overlay) depends on this capture
substrate.

### Process gate

**RFC seed required** for the screencap/readback API (new service API + wire
format) before implementation: `docs/rfcs/RFC-TEMPLATE.md`, next free number,
RFC index update. New markers must land together with `scripts/qemu-test.sh`
and `tools/nx/chains/markers.txt` (no-fake-green contract).

### Corrected proof + touched paths

`tests/ui_v7c_host/` never existed; host proofs go to
`source/services/screencapd/tests/` (service layout: src/ + tests/) and
`source/services/windowd/tests/` for the readback fixture. Allowlist below
updated; share-broker/export/sheet bullets in the sections that follow are
superseded by this rebase.

## Context

Screenshot and sharing are powerful and privacy-sensitive. With kernel unchanged, the capture pipeline
must be implemented in userspace, most naturally by `windowd` readback of the last composed buffer
and a dedicated service facade (`screencapd`).

We also need a minimal share-sheet broker to route payloads to:

- clipboard,
- save-to-file under `/state`,
- (optional) peer via DSoftBus (stubbed by default).

## Goal (rebased 2026-08-14 — capture-only)

Deliver:

1. `screencapd` service:
   - `grabDisplay`, `grabWindow`, `grabRegion`
   - returns VMO + metadata (w/h/stride)
   - implemented via a bounded `windowd` readback API
2. Privacy/policy:
   - consent model for screencap (v1: allow in selftests only; otherwise require explicit “consent” flag from focused window)
   - size/pixel caps; reject out-of-bounds regions
3. Host tests + OS markers.

## Non-Goals

- Kernel changes.
- Full gallery app.
- Any peer share.
- **The share half entirely** (broker, export destinations, sheet UI): the
  intent-based share pipeline is Share v2 (`TASK-0126`/`TASK-0127`/`TASK-0128`);
  the sheet UI is a DSL app surface there. `sharesheetd` is cut from this ledger.

## Constraints / invariants (hard requirements)

- Bounded capture:
  - cap max pixels and max bytes per capture
  - reject out-of-bounds regions
- Deterministic output for test patterns (host tests).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`source/services/screencapd/tests/` + `source/services/windowd/tests/`
(corrected 2026-08-14; `tests/ui_v7c_host/` never existed):

- render a checkerboard into a composed buffer fixture
- `grabRegion` returns correct checksum (display/window/region variants)
- out-of-bounds region and over-cap pixel/byte requests reject deterministically
- policy deny case blocks capture without consent flag (host simulated)

### Proof (OS/QEMU) — gated

UART markers (new markers land together with `scripts/qemu-test.sh` +
`tools/nx/chains/markers.txt`):

- `screencapd: ready`
- `screencap: grab ok (kind=display|window|region)`
- `SELFTEST: ui v7 screencap ok`

## Touched paths (allowlist) — corrected 2026-08-14

- `source/services/screencapd/` (new: src/ + tests/, os-lite)
- `source/libs/nexus-wire/src/` (screencapd wire module — approval zone `source/libs/**`)
- `source/services/windowd/` (readback API only; check the cleanup map first)
- `docs/rfcs/` (screencap/readback RFC seed — approval zone)
- `source/apps/selftest-client/`
- `docs/dev/ui/system-experiences/capture-and-share/screencap-share.md`

## Plan (small PRs)

1. RFC seed for the screencap/readback API (approval gate)
2. windowd readback API + bounds/limits + host fixture
3. screencapd (grab APIs, consent + caps, policy deny) + markers
4. tests + OS markers + docs

## Follow-ups

- Share v2 (intent-based, multi-app): `TASK-0126` (intentsd+policy), `TASK-0127` (chooser+targets+grants), `TASK-0128` (app senders+selftests+docs)
