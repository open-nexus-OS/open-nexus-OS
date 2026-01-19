---
title: TRACK Teleprompter App (Teleprompter Pro-class): smooth script scrolling + remote control + mirror mode, deterministic and capability-gated
status: Draft
owner: @apps @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - windowd compositor/present spine: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Renderer abstraction (host-first): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Text stack foundations: tasks/TASK-0094-ui-v15a-text-primitives-uax-bidi-hittest.md, tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Content/URIs + picker + grants: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md, tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md, tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Input spine (keyboard + HID + routing): tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md
---

## Goal (track-level)

Deliver a first-party **teleprompter** app comparable to Teleprompter Pro (interaction patterns only; no trade dress):

- import/open scripts (plain text, markdown-lite, optional PDF later),
- **smooth, readable scrolling** with stable timing controls,
- **remote control** via keyboard/HID (start/stop, speed up/down, jump markers),
- mirror/flip modes (for physical mirrors),
- cue markers (section breaks) + quick navigation,
- deterministic tests for scroll/timing behavior (fixtures; no wallclock flakiness).

## Non-goals (avoid drift)

- Not a full word processor.
- No unbounded rich layout engine; keep script format bounded and predictable.
- No “always-on background capture” features; keep permissions minimal.

## Authority model (must match registry)

Teleprompter is an app. It consumes:

- `windowd` (present),
- `inputd` (remote control events; no direct device node access),
- `contentd`/`mimed`/`grantsd` for open/save without paths,
- `policyd` for any gated capabilities (if needed later).

## Key UX features (v1 scope)

- **Scroll engine**:
  - speed slider + fine adjust
  - start/stop/pause
  - “count-in” delay (optional)
  - per-script saved settings (bounded)
- **Mirror**:
  - horizontal flip, vertical flip
  - high-contrast mode (optional)
- **Markers**:
  - user inserts markers (e.g., `##` headings or explicit marker lines)
  - quick jump list
- **Remote controls**:
  - space = start/stop
  - arrows = speed adjust
  - page up/down = jump marker
  - deterministic mapping via `inputd` keymaps

## Gates / blockers

- Rendering/present: `TASK-0055` + renderer abstraction `TASK-0169`/`TASK-0170`
- Input spine (HID/keyboard routing): `TASK-0253`
- Content/picker/grants: `TASK-0081`/`TASK-0083`/`TASK-0084`

## Phase map

### Phase 0 — Host-first teleprompter core

- script parsing (bounded) + layout to lines
- scroll timing model (deterministic; test fixtures)
- render snapshots/goldens for representative scripts

### Phase 1 — OS wiring

- open script via picker (content://)
- remote controls via `inputd`
- QEMU markers only after real open + start/stop behaviors

### Phase 2 — Pro polish

- PDF import as script source (optional; bounded)
- setlists and quick-switch between scripts (optional)

## Candidate subtasks (to be extracted into TASK-XXXX)

- **CAND-PROMPT-000: Script model + bounded parser v0 (text/markdown-lite) + fixtures**
- **CAND-PROMPT-010: Scroll engine v0 (deterministic timing + controls) + host tests**
- **CAND-PROMPT-020: Remote control mapping v0 (inputd integration) + markers**
- **CAND-PROMPT-030: Mirror/high-contrast modes v0 + render goldens**

## Extraction rules

Candidates become real tasks only when they:

- define explicit bounds (max script bytes, max lines, max markers),
- have deterministic host proofs (render goldens + timing fixtures),
- keep authority boundaries (`inputd` routing; `contentd/grantsd` for file access).
