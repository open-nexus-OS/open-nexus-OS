---
title: TRACK NexusFrame (Pixelmator-class): fast photo/design editor (layers, masks, non-destructive), deterministic, capability-gated
status: Draft
owner: @apps @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - System Delegation / System Surfaces (“Edit in Frame”, export/share): tasks/TRACK-SYSTEM-DELEGATION.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Creative apps foundation (gates mindset): tasks/TRACK-CREATIVE-APPS.md
  - Zero-Copy App Platform (autosave/recovery patterns): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - NexusGfx SDK (render/compute contracts): tasks/TRACK-NEXUSGFX-SDK.md
  - Drivers & accelerators (GPU/device-class direction): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Zero-copy VMOs (bulk buffers): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - windowd compositor/present spine: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Renderer abstraction (host-first): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Renderer wiring (OS/QEMU): tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Content/URIs + picker + grants: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md, tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md, tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Files app integration (share/open-with): tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - Share/Intents (optional “Edit in Frame” offers): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md, tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
---

## Goal (track-level)

Deliver **NexusFrame**, a first-party **Pixelmator-class** photo/design editor that feels fast and “pro”:

- Raster-first editor: **canvas + layers + blend modes + masks**
- Selection tools + transforms: crop/rotate/scale, basic warp-lite
- Non-destructive editing (phased): adjustment layers / filter stack with bounded caches
- Text + shapes (vector-lite) for design overlays
- Export/import: PNG/JPEG/WebP/HEIF (bounded), plus a stable project format
- Deterministic proofs (host goldens) and capability-first security (no ambient file access)

This app is a reference workload for NexusGfx + zero-copy bulk buffers and for “creative pro” UX.

## Non-goals (avoid drift)

- Not a Photoshop clone in v1.
- Not a full vector illustrator (no InDesign/Illustrator scope creep).
- Not “embed arbitrary third-party plugin UIs”; Frame owns its UI for a coherent product.
- No unbounded background work (endless preview recompute, unbounded history growth).

## Authority model (must match registry)

NexusFrame is an **app**. It consumes:

- `windowd` (present/compositor)
- `contentd` + `mimed` (content:// access and type associations)
- `grantsd` (scoped grants for cross-subject file access)
- `policyd` (permission decisions)
- `logd` (audit/log sink)

No parallel authorities (no new “imaged”, “photod” service) unless explicitly extracted later with a deprecation plan.

## Capability stance (directional)

Frame must remain capability-gated:

- file/content access via `content://` + scoped grants (no paths)
- optional network features (if ever added) must be policy-gated and bounded
- any “import from Photos/Gallery” uses picker/intents + grants

## System Delegation integration

Frame should register as a system editing surface so other apps don’t ship their own editors:
- “Edit in NexusFrame” should be a chooser/default-driven flow (intents + grants),
- exports should be shared via Share v2 targets (Files/Notes/Chat) with bounded payloads.

## Keystone gates / blockers

### Gate A — Rendering substrate (2D now, GPU later)

References:

- `tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md`
- `tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md`
- `tasks/TRACK-NEXUSGFX-SDK.md`

Frame is “real” when the editor can render deterministically (host goldens) and present via `windowd` on OS/QEMU.

### Gate B — Zero-copy bulk buffers (VMO/filebuffer)

Reference: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`.

Needed for: large canvases, layer tiles, previews, history buffers without copy storms.

### Gate C — Pathless storage + grants

References: `TASK-0081`, `TASK-0083`, `TASK-0084`.

Needed for: open/save/export/share with safe, scoped access.

### Pro-performance blocker — GPU backend

Reference: `tasks/TRACK-DRIVERS-ACCELERATORS.md` and `tasks/TRACK-NEXUSGFX-SDK.md`.

“Pixelmator-class pro feel” (large layers + realtime filters) is gated on a real GPU backend behind stable contracts.

### Optional pro blocker — Stylus pressure/tilt

If Frame includes “paint/retouch” tools (brush/smudge/clone/heal), stylus input becomes a separate gate.
This should align with the input authority model (`inputd`) and be deterministic/fixture-testable.

## Product architecture stance (how we stay fast + testable)

### Canonical document model (raster-first)

Frame’s core model should be deterministic and bounded:

- canvas size + color space + pixel format
- layers:
  - bitmap tiles (bounded tile size)
  - blend mode + opacity
  - masks (bitmap)
- edits:
  - command log for semantic operations (transform, adjust params)
  - bounded caches for previews

### Undo/redo stance (raster-friendly, bounded)

Do **not** store per-pixel OpLog entries. Prefer:

- command-level undo for semantic edits,
- tile-diff snapshots for destructive pixel ops (bounded history depth, bounded bytes),
- deterministic replay from periodic checkpoints.

## Phase map (what “done” means by phase)

### Phase 0 — Host-first “Frame Lite” (fastest credible slice)

- canvas + layers + blend modes + basic masks
- selection (rect/lasso-lite) + transform (crop/rotate/scale)
- import/export PNG/JPEG (bounded)

Proof:

- host goldens: “fixture doc → rendered output hash”
- negative tests: reject oversized images / invalid metadata bounds

### Phase 1 — OS wiring (real app on OS/QEMU)

- open/save/export via picker (`TASK-0083`) + grants (`TASK-0084`)
- present via `windowd`
- share “exported image” via share v2 (optional; `TASK-0126`/`0127`)

Proof:

- OS markers only after real open/save/export occurred (no fake success)

### Phase 2 — Non-destructive editing v1 (adjustment stack)

- adjustment layers / filter stack (bounded parameter sets)
- cache + preview invalidation (deterministic, bounded)

Proof:

- host goldens for filter outputs and cache determinism

### Phase 3 — Pro acceleration (GPU backend behind contracts)

- GPU backend slot-in behind renderer/NexusGfx contracts
- perf gates are host-first; OS markers only report real behavior

### Phase 4 — Pro tools (optional)

- brush engine (paint/smudge) and retouch tools
- stylus pressure/tilt gate (if needed)

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-FRAME-000: Document model v0 (canvas/layers/masks) + deterministic render goldens**
- **CAND-FRAME-010: Selection + transform v0 (crop/rotate/scale) + tests**
- **CAND-FRAME-020: Export pipeline v0 (PNG/JPEG/WebP/HEIF bounded) + `test_reject_*`**
- **CAND-FRAME-030: Non-destructive adjustments v0 (stack + cache) + goldens**
- **CAND-FRAME-040: OS wiring v0 (picker/grants/windowd) + deterministic markers**
- **CAND-FRAME-050: GPU acceleration adapter v0 (behind NexusGfx contracts)**
- **CAND-FRAME-060: Brush/retouch v0 (bounded; stylus optional)**

## Extraction rules

A candidate becomes a real `TASK-XXXX` only when it:

- declares explicit bounds (pixels, layers, tile sizes, history bytes),
- includes deterministic host proofs (goldens and rejection tests),
- keeps authority boundaries (no parallel policy/filesystems),
- documents any stubbed behavior explicitly (no fake “ok” markers).
