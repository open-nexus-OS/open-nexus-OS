---
title: TRACK Live Studio App (OBS-class): capture + scene compose + stream/record, capability-gated and deterministic
status: Draft
owner: @media @ui @runtime
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority registry (names are binding): tasks/TRACK-AUTHORITY-NAMING.md
  - Keystone closure plan: tasks/TRACK-KEYSTONE-GATES.md
  - NexusMedia SDK (audio/video/image contracts): tasks/TRACK-NEXUSMEDIA-SDK.md
  - NexusGfx SDK (render/compute contracts): tasks/TRACK-NEXUSGFX-SDK.md
  - Drivers & accelerators (GPU/VPU/audio/camera contracts): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - NexusNet SDK (streaming/networking, policy-gated): tasks/TRACK-NEXUSNET-SDK.md
  - Zero-copy data plane (VMOs): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - windowd present spine (OS/QEMU): tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Renderer abstraction (host + OS wiring): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Input spine (for hotkeys): tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md
  - Screen capture direction: tasks/TASK-0105-ui-v17c-screen-recorder-capture-overlay.md
  - Camera/mic device direction: tasks/TASK-0104-ui-v17b-camerad-micd-virtual-sources.md
---

## Goal (track-level)

Deliver a first-party **Live Studio** app comparable to OBS Studio:

- scene graph with sources (screen/window, camera, images, text, browser/webview later),
- transforms/crop/masks + basic effects (chroma key as a later phase),
- audio mixing (desktop + mic sources) with meters,
- recording to file and streaming to a remote endpoint (protocol phased),
- deterministic proofs and strong security boundaries (no ambient capture).

This app is a reference workload for:

- `tasks/TRACK-NEXUSMEDIA-SDK.md` (capture/encode/audio sync),
- `tasks/TRACK-NEXUSGFX-SDK.md` (scene composition backend evolution),
- `tasks/TRACK-NEXUSNET-SDK.md` (bounded streaming and auth/grants),
- `tasks/TRACK-DRIVERS-ACCELERATORS.md` (VPU/GPU/camera/audio device-class services).

## Non-goals (avoid drift)

- Not a plugin ecosystem in v1 (OBS plugins can be a later track).
- Not “support every streaming protocol immediately”.
- Not “ambient global capture”: capture must be explicit, consented, and auditable.

## Authority model (must match registry)

- `windowd`: preview/present
- `audiod`: audio mix/route authority (studio mixes route through audiod contracts)
- `policyd`: allow/deny capture/network; audit records
- `logd`: logs/audit sink
- capture authorities (directional): camera/mic/screen capture services (see linked tasks)

Do not create parallel mixers or a second capture authority.

## Keystone gates / blockers

### Gate 1 — IPC + cap transfer (Keystone Gate 1)

Reference: `tasks/TRACK-KEYSTONE-GATES.md`.

Needed for: safe service boundaries (capture/encode/network), handle transfer, and backpressure.

### Gate 2 — Safe userspace device access (MMIO model)

Reference: `tasks/TASK-0010-device-mmio-access-model.md`.

Needed for: real camera/audio/VPU/GPU device-class services in userland (later phases).

### Gate 3 — Zero-copy VMO/filebuffer data plane

Reference: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`.

Needed for: frame/audio buffers and low-copy pipeline between capture → compose → encode.

### Gate 4 — Present spine (windowd) + renderer abstraction

References: `tasks/TASK-0055`, `tasks/TASK-0169`, `tasks/TASK-0170`.

Needed for: deterministic preview and composition correctness tests (goldens).

### Gate 5 — Network/streaming (policy-gated)

Reference: `tasks/TRACK-NEXUSNET-SDK.md`.

Needed for: bounded retry/timeout behavior; no “raw sockets everywhere”.

## Capture is security-critical (explicit stance)

Unlike a pure editor, Live Studio touches sensitive surfaces:

- screen/window capture,
- microphone/camera,
- potentially system audio capture,
- streaming to network endpoints.

Rules:

- capture must be capability-gated and typically also runtime-consent gated,
- indicators are required (recording/live indicator),
- no “warn and continue” on denied capture: fail closed,
- inputs are bounded (frame sizes, sample rates, channel counts),
- logs/audits must not leak content (no raw frames/audio dumps).

## Phase map (what “done” means by phase)

### Phase 0 — Host-first studio core (fake devices, real semantics)

Goal: prove scene + A/V sync + muxing semantics without real capture devices.

- fake camera/mic sources (fixtures) and fake screen source (generated pattern)
- scene graph composition to frames (cpu2d backend)
- record-to-file sink (bounded), no network

Proof:

- host tests: “scene script → frame hash sequence”
- audio tests: deterministic mix output checksums

### Phase 1 — OS/QEMU capture wiring (still bounded, honest)

- integrate minimal screen recorder/capture overlay direction (see `TASK-0105`)
- integrate camera/mic virtual sources direction (see `TASK-0104`)
- preview via `windowd`
- recording works end-to-end with markers only after real behavior

### Phase 2 — Streaming MVP (one protocol, bounded)

- add a single streaming protocol MVP (chosen later; bounded)
- strict network caps and deterministic backoff
- audit records for “stream start/stop” and endpoint identity (no secrets)

### Phase 3 — Pro pipelines (hardware encode + advanced effects)

- VPU encode backend behind NexusMedia contracts (submit + fence real)
- GPU composition backend behind renderer/NexusGfx contracts
- chroma key, more effects, multitrack audio routing (still bounded)

## Candidate subtasks (to be extracted into TASK-XXXX)

- **CAND-LIVESTUDIO-000: Scene graph v0 + composition goldens (host-first)**
- **CAND-LIVESTUDIO-010: Capture fixtures v0 (virtual screen/cam/mic) + deterministic A/V sync tests**
- **CAND-LIVESTUDIO-020: Record sink v0 (container + profiles, bounded)**
- **CAND-LIVESTUDIO-030: Streaming MVP v0 (one protocol, bounded, policy-gated)**
- **CAND-LIVESTUDIO-040: Hardware encode adapter v0 (VPU path behind NexusMedia)**

## Extraction rules

Candidates become real tasks only when they:

- include negative tests (`test_reject_*`) for denied capture/network and oversized inputs,
- define hard bounds and deterministic proofs,
- keep authority boundaries (no parallel capture/mixers, no duplicate policy).
