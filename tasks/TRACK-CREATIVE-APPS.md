---
title: TRACK Creative Apps (Procreate / SketchUp / Shapr3D class): gate-closed graphics + input + zero-copy foundation, deterministic proofs
status: Living
owner: @ui @runtime
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - System Delegation / System Surfaces (import/export/share/open-with primitives): tasks/TRACK-SYSTEM-DELEGATION.md
  - Authority registry (names are binding): tasks/TRACK-AUTHORITY-NAMING.md
  - Keystone closure plan: tasks/TRACK-KEYSTONE-GATES.md
  - Drivers & accelerators (GPU/NPU/VPU contracts): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - NexusGfx SDK (Metal-like, CAD-ready): tasks/TRACK-NEXUSGFX-SDK.md
  - NexusGame SDK (realtime loop/input replay): tasks/TRACK-NEXUSGAME-SDK.md
  - Zero-Copy App Platform (OpLog/autosave/connectors/UI primitives): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Media stack (image/video/audio decoders + sessions): tasks/TRACK-MEDIA-APPS.md
  - NexusFrame (Pixelmator-class photo/design editor): tasks/TRACK-NEXUSFRAME.md
---

## Goal (track-level)

Enable a class of **professional creative applications** on Open Nexus OS:

- **2D painting / illustration** (Procreate-class)
- **3D modeling** (SketchUp-class)
- **3D CAD-lite → CAD** (Shapr3D-class direction)

while preserving core OS invariants:

- **capability-first security** (no ambient GPU/device access; policy decides, kernel enforces),
- **hybrid control/data plane** (typed IPC + bulk via VMO/filebuffer),
- **deterministic proofs** (host-first goldens + QEMU markers where meaningful),
- **bounded resources** (bytes/time/node counts; no unbounded parsing/work).

This track is primarily a **gate closure + dependency map**. It is not an implementation task.

## Non-goals (avoid drift)

- Not a Vulkan/OpenGL compatibility program.
- Not “ship Procreate/SketchUp/Shapr3D as-is”; we define phased targets and gates.
- Not “GPU driver at all costs”; GPU enablement must be capability-gated, auditable, and testable.

## Authority model (must match registry)

Creative apps consume these canonical authorities (see `tasks/TRACK-AUTHORITY-NAMING.md`):

- **Compositor / window system**: `windowd`
- **Input routing**: `inputd` (single authority)
- **IME**: `imed` (overlay + keymap hooks)
- **Audio mixing / route** (if used): `audiod`
- **Policy**: `policyd`
- **Logs/audit**: `logd`
- **Persistence substrate**: `statefsd` (durable `/state`)

No creative app or SDK may introduce parallel “gfx manager”, “input manager”, or duplicate policy logic.

## System Delegation integration

Creative apps should rely on platform delegation primitives (not ad-hoc integrations):
- open/import via picker/Open With (mimed + appmgrd),
- export/share via Intents/Chooser (Files/Notes/Chat),
- optional “Edit in …” surfaces should be chooser/default driven instead of embedding cross-app UIs.

## Keystone gates (closure definitions)

These gates are the minimum closure plan required for “creative apps” to be real, not demo-only.

### Gate A — Kernel IPC + cap transfer (Keystone Gate 1)

Reference: `tasks/TRACK-KEYSTONE-GATES.md` (Gate 1).

**Unblocked when**:

- QEMU selftests prove channel create, send/recv, cap transfer, backpressure (see Gate 1 closure markers in the track).

### Gate B — Safe userspace device access (MMIO model) (Keystone Gate 2)

Reference: `tasks/TASK-0010-device-mmio-access-model.md`.

**Unblocked when**:

- a userspace service can map a permitted MMIO range via capability,
- attempts to map outside the permitted window are deterministically denied,
- mappings are **USER|RW only, never executable** (W^X at boundary),
- at least one userspace virtio-mmio driver smoke test exists (per Keystone Gate 2 closure definition).

This gate is a hard prerequisite for real GPU device services.

### Gate C — Zero-copy VMO/filebuffer data plane (VMO share proof)

Reference: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`.

**Unblocked when**:

- two-process QEMU proof exists: producer transfers VMO cap → consumer maps RO → computes digest,
- marker set includes `SELFTEST: vmo share ok` (and supporting markers described in the task).

This gate is required for large canvases, textures, meshes, and asset streaming without copy storms.

### Gate D — Present/compositor spine (headless present + markers)

Reference: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md`.

**Unblocked when**:

- `windowd` can accept VMO-backed surface buffers, compose on a vsync tick, and emit deterministic markers,
- basic bounds exist (surfaces count, pixel sizes, bytes).

### Gate E — Renderer abstraction + OS wiring (backend-agnostic)

References:
- `tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md`
- `tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md`

**Unblocked when**:

- Host: Scene-IR validate + cpu2d goldens are deterministic and bounded.
- OS/QEMU: `windowd` renders via `renderer::Backend` and emits present markers (`windowd: present ok`, `SELFTEST: renderer v1 present ok`).

This is the “swap point” that allows a future GPU backend without rewriting `windowd`.

### Gate F — Input spine (touch/mouse/kbd routing, deterministic)

Reference: `tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md`.

**Unblocked when**:

- `inputd` is the single routing authority on OS/QEMU,
- deterministic keymap/repeat/dispatch works end-to-end with markers (`SELFTEST: input cursor ok`, `SELFTEST: input touch ok`, etc.),
- window focus + IME show/hide hooks are wired (stubs allowed but must be explicit).

## The GPU driver “blocker” (what it means here)

This track treats “GPU driver” as a **system blocker** for Pro-class creative workloads.

Reference contracts:

- Device-class services and DriverKit direction: `tasks/TRACK-DRIVERS-ACCELERATORS.md`
- SDK consumer contracts: `tasks/TRACK-NEXUSGFX-SDK.md`
- Keystone gate for safe MMIO: `tasks/TASK-0010-device-mmio-access-model.md`
- Zero-copy buffer contract: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`

### Minimum definition (Phase 0: bring-up real behavior)

GPU is considered “unblocked for creative workloads (v0)” only when we have:

- **device online** (cap-gated open),
- **buffer import** (VMO/filebuffer-backed payload handles),
- **submit + fence completion** (bounded in-flight work; no busy-wait),
- deterministic failure modes + clear “why denied” diagnostics (bounded),
- no fake success markers: only claim “ok” after submit/fence actually completes.

### Security invariants (non-negotiable)

- No ambient GPU access; access is mediated by policy and capabilities (`policyd` decides).
- MMIO mappings are USER|RW only, never executable (W^X).
- Command submission is validated in userland (bounded); device reset/fault containment exists.
- Audits exist for sensitive operations (device open/close, reset, privileged modes).

## App-class readiness matrix (what “creative” means by phase)

### Phase 0 — “Creative-lite” (CPU backend, real foundations)

Goal: ship a credible base without claiming pro performance.

**Requires**:

- Gates A/C/D/E/F (IPC, VMO share, present spine, renderer abstraction + OS wiring, input spine)

**Delivers**:

- 2D sketching/whiteboard class app (bounded canvas, limited layers),
- 3D viewer (small meshes, bounded features) if 3D pipeline exists in CPU backend,
- deterministic host tests + QEMU “present/input ok” markers.

### Phase 1 — “Pro 2D” (painting-ready performance)

**Blocker**: GPU driver definition (above) + NexusGfx 2D pipeline direction.

**Requires**:

- GPU service Phase 0 real behavior (device online + submit/fence),
- NexusGfx SDK surface stable enough for a rendering backend to target,
- strict budgets/backpressure (no runaway layer/brush allocations).

**Notes**:

- Procreate-class also needs **stylus pressure/tilt** input surfaces. This is a separate capability/driver path and must not be conflated with GPU enablement.

### Phase 2 — “Pro 3D modeling” (SketchUp-class)

**Blocker**: GPU driver + NexusGfx 3D baseline (triangles/depth/picking direction).

**Requires**:

- stable picking/selection buffers (deterministic),
- input gestures + camera controls bounded and testable,
- viewport perf instrumentation (no “perf ok” without real perf stack).

### Phase 3 — “CAD direction” (Shapr3D-class)

This is the longest pole. GPU unblocks the viewport, but CAD also needs:

- authoritative geometry kernel (B-Rep/constraints) with deterministic proofs,
- robust import/export story (bounded parsing; policy-gated file access),
- selection + snapping inference engine (bounded; deterministic).

GPU is **necessary but not sufficient** for Shapr3D-class functionality.

## Candidate subtasks (to be extracted into TASK-XXXX)

These are placeholders to avoid drift; extract only when implementable under current gates and with proofs.

- **CAND-CREATIVE-000: Creative gates checklist + CI proof harness**  
  - unify “creative-ready” markers as a dashboard (host + QEMU), no fake success.

- **CAND-CREATIVE-010: Stylus input v0 (pressure/tilt) — capability-gated**  
  - extend input stack without creating parallel authorities; deterministic fixtures for tests.

- **CAND-CREATIVE-020: Painting canvas model v0 (bounded layers + undo semantics)**  
  - define a bounded raster model and undo/redo semantics (not OpLog for every pixel).

- **CAND-CREATIVE-030: 3D viewer v0 (mesh load + orbit controls + picking stub)**  
  - bounded loaders; deterministic camera fixtures; no claims beyond the phase.

- **CAND-CREATIVE-040: NexusGfx CAD-lite primitives v0 (picking + selection buffers + instancing)**  
  - aligns with `TRACK-NEXUSGFX-SDK` CAD notes; host-first goldens.

## Extraction rules (how this becomes real tasks)

A candidate becomes a real `TASK-XXXX` only when it:

- states explicit bounds (bytes/time/nodes/frames),
- declares determinism requirements and has host proof (goldens/replay fixtures),
- declares OS/QEMU marker proofs where meaningful,
- names authority boundaries and does not introduce competing authorities,
- documents security invariants (no secrets in logs; policy gating; deny-by-default).
