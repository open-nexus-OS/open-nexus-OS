---
title: TASK-0068 UI v7c: screenshot (screencapd) + share-sheet broker + privacy/policy guards
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v4a compositor baseline (readback): tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - UI v6a WM baseline (grab window): tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - Clipboard v2 (destination): tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - DSoftBus (peer share, optional): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Policy as Code (consent/limits): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Persistence (/state save): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Screenshot and sharing are powerful and privacy-sensitive. With kernel unchanged, the capture pipeline
must be implemented in userspace, most naturally by `windowd` readback of the last composed buffer
and a dedicated service facade (`screencapd`).

We also need a minimal share-sheet broker to route payloads to:

- clipboard,
- save-to-file under `/state`,
- (optional) peer via DSoftBus (stubbed by default).

## Goal

Deliver:

1. `screencapd` service:
   - `grabDisplay`, `grabWindow`, `grabRegion`
   - returns VMO + metadata (w/h/stride)
   - implemented via `windowd` readback (bounded)
2. Share broker service (name TBD: `shared`/`sharesheetd`):
   - accepts payloads (screenshot VMO, clipboard items, app-provided)
   - exports to clipboard or `/state/pictures` (peer stub optional)
3. SystemUI share sheet overlay (minimal):
   - open sheet, show targets, export
4. Privacy/policy:
   - consent model for screencap (v1: allow in selftests only; otherwise require explicit “consent” flag from focused window)
   - size/pixel caps
5. Host tests + OS markers.

## Non-Goals

- Kernel changes.
- Full gallery app.
- Full peer share (default off; stub only).
- Full intent-based share pipeline (chooser + targets + results + grants). This is Share v2 (`TASK-0126`/`TASK-0127`/`TASK-0128`).

## Constraints / invariants (hard requirements)

- Bounded capture:
  - cap max pixels and max bytes per capture
  - reject out-of-bounds regions
- Deterministic output for test patterns (host tests).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v7c_host/`:

- render a checkerboard into a composed buffer fixture
- `grabRegion` returns correct checksum
- share-to-file writes expected PNG (or raw BGRA with documented format)
- policy deny case blocks capture without consent flag (host simulated)

### Proof (OS/QEMU) — gated

UART markers:

- `screencapd: ready`
- `share: sheet open (kind=screenshot)`
- `share: exported (dest=clipboard|file|peer)`
- `SELFTEST: ui v7 share ok`

## Touched paths (allowlist)

- `source/services/screencapd/` (new)
- `source/services/sharesheetd/` (new)
- `source/services/windowd/` (readback API)
- `tests/ui_v7c_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v7c.sh` (delegates)
- `docs/dev/ui/screencap-share.md`

## Plan (small PRs)

1. screencapd API + readback implementation + bounds/limits + marker
2. share broker + destinations (clipboard/file) + markers
3. SystemUI overlay minimal wiring
4. tests + OS markers + docs + postflight

## Follow-ups

- Share v2 (intent-based, multi-app): `TASK-0126` (intentsd+policy), `TASK-0127` (chooser+targets+grants), `TASK-0128` (app senders+selftests+docs)
