---
title: TRACK Video Editor App (Instagram Edits-class): timeline NLE + effects + export, NexusMedia/NexusGfx-backed (deterministic, policy-gated)
status: Draft
owner: @media @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - System Delegation / System Surfaces (Edit-in target + export/share via intents): tasks/TRACK-SYSTEM-DELEGATION.md
  - Authority registry (names are binding): tasks/TRACK-AUTHORITY-NAMING.md
  - Keystone closure plan: tasks/TRACK-KEYSTONE-GATES.md
  - NexusMedia SDK (audio/video/image contracts): tasks/TRACK-NEXUSMEDIA-SDK.md
  - NexusGfx SDK (render/compute contracts): tasks/TRACK-NEXUSGFX-SDK.md
  - Drivers & accelerators (GPU/VPU/audio/camera contracts): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Zero-copy data plane (VMOs): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - windowd present spine (OS/QEMU): tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Renderer abstraction (host + OS wiring): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Renderer abstraction OS wiring: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Media decoders v1 (host-first): tasks/TASK-0099-ui-v16a-media-decoders.md
  - Media sessions/system surfaces: tasks/TASK-0101-ui-v16c-media-sessions-systemui-controls.md
---

## Goal (track-level)

Deliver a first-party **mobile-first video editor** with functionality comparable to “Instagram Edits”:

- import clips (camera roll + files),
- timeline editing (trim/split, reorder, multi-clip),
- overlays (text, stickers/graphics), transitions, simple keyframes,
- audio bed + clip audio + basic mixing,
- export to modern formats and share.

## System Delegation integration

Video Editor should be a standard “Edit in …” destination:
- other apps (Photos/Camera/Files/NexusMoments/NexusVideo) should delegate to the editor via intents + scoped grants,
- export/share should route back through Share v2 (no custom per-app sharing code paths).

This app is a **reference workload** for:

- `tasks/TRACK-NEXUSMEDIA-SDK.md` (decode/encode + timeline substrate),
- `tasks/TRACK-NEXUSGFX-SDK.md` (real-time preview composition),
- `tasks/TRACK-DRIVERS-ACCELERATORS.md` (VPU/GPU path behind stable contracts).

## Non-goals (avoid drift)

- Not a full DaVinci/FinalCut replacement in v1.
- Not a “FFmpeg port as architecture”. We can use bounded codec libs, but the *contracts* are NexusMedia.
- No unbounded background rendering/export loops; all work is budgeted and cancelable.

## Authority model (must match registry)

- `windowd`: preview/present
- `audiod`: audio mix/route authority
- `policyd`: permissions (read media, capture, export)
- `contentd`/`grantsd` (when present): file/content access, scoped grants
- `logd`: audit/log sink

No parallel “videoexportd” authority unless explicitly extracted later with a deprecation plan.

## Keystone gates / blockers (what must be true for “real editor”)

### Gate 1 — IPC + cap transfer (Keystone Gate 1)

Reference: `tasks/TRACK-KEYSTONE-GATES.md`.

Needed for: service boundaries (decode/encode service, storage/content brokers) and buffer handle transfer.

### Gate 2 — Safe userspace device access (MMIO model)

Reference: `tasks/TASK-0010-device-mmio-access-model.md`.

Needed for: real VPU/GPU device-class services in userland (later phases).

### Gate 3 — Zero-copy data plane (VMO/filebuffer)

Reference: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`.

Needed for: frame buffers, clip caches, waveform/thumb generation without copy storms.

### Gate 4 — Present spine (windowd)

References: `tasks/TASK-0055`, `tasks/TASK-0169`, `tasks/TASK-0170`.

Needed for: deterministic preview frames and bounded render loop.

### GPU/VPU “pro” blocker

Reference: `tasks/TRACK-DRIVERS-ACCELERATORS.md`.

Editing is considered “pro-ready” only when:

- decode → compose → encode can run with bounded deadlines/backpressure,
- device submit + fence completion is real (no fake success),
- failures are deterministic and auditable.

## Phase map (what “done” means by phase)

### Phase 0 — Host-first editor core (real semantics, CPU preview)

**Goal**: prove the model without claiming realtime performance.

- timeline model + edit ops (trim/split/reorder) with deterministic tests
- preview renderer uses cpu2d backend via renderer abstraction (host)
- export uses bounded, fixture-based codecs (or stubbed with explicit `placeholder`)

**Proof**:

- host tests: “edit script → frame hash sequence” for fixtures (NexusMedia direction)
- goldens for composited preview frames (renderer goldens)

### Phase 1 — OS/QEMU wiring (honest preview + media sessions)

- preview runs through `windowd` present path
- media session integration for preview playback controls where applicable
- export path is gated and emits explicit markers only after real work

**Proof**:

- QEMU markers for present + bounded playback loop (no perf claims)

### Phase 2 — Hardware acceleration path (VPU/GPU behind contracts)

- optional VPU decode/encode service behind NexusMedia contracts
- optional GPU preview backend behind renderer/NexusGfx contracts
- strict budgets/backpressure + cancelation

**Proof**:

- deterministic host fixtures for correctness + bounded perf traces (host-first)
- OS markers only for “behavior happened” (submit/fence completed, export completed)

## Security & privacy (non-negotiable)

Editing touches sensitive surfaces (files, camera roll, capture, export):

- no ambient filesystem paths; use scoped grants
- bounded parsing of containers/metadata (no unbounded MP4 atoms / ID3 / EXIF blobs)
- no secrets in logs; audit export/share events without embedding user content
- screen/camera/mic capture (if added) must be consent + indicator gated

## Candidate subtasks (to be extracted into TASK-XXXX)

- **CAND-VIDEDIT-000: Timeline core v0 (clips/tracks/edits) + deterministic host proofs**
- **CAND-VIDEDIT-010: Preview composition v0 (text/overlays/transitions subset) + goldens**
- **CAND-VIDEDIT-020: Export pipeline v0 (bounded profiles, cancelation, audit hooks)**
- **CAND-VIDEDIT-030: Thumbs/waveforms cache v0 (budgets, deterministic eviction)**
- **CAND-VIDEDIT-040: VPU/GPU acceleration adapters v0 (behind NexusMedia/NexusGfx contracts)**

## Extraction rules

Candidates become real tasks only when they:

- state explicit bounds (frames/sec targets as *goals*, not proofs; bytes/time caps as requirements),
- provide deterministic host proofs (goldens/hashes),
- use canonical authorities and capability gates (no parallel policy logic).
