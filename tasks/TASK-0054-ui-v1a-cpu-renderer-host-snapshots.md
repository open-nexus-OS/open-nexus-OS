---
title: TASK-0054 UI v1a (host-first): BGRA8888 CPU renderer + damage tracking + headless snapshots (PNG/SSIM)
status: In Review
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks:
  - TASK-0054B
  - TASK-0054C
  - TASK-0054D
  - TASK-0169
  - TASK-0170
links:
  - RFC: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Gfx compute/executor model: docs/architecture/nexusgfx-compute-and-executor-model.md
  - Gfx text pipeline integration: docs/architecture/nexusgfx-text-pipeline.md
  - UI consumer of buffer/sync contracts: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers (future vsync spine): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Kernel/UI perf floor follow-up: tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md
  - Kernel IPC fastpath follow-up: tasks/TASK-0054C-ui-v1a-kernel-ipc-fastpath-control-plane-vmo-bulk.md
  - Kernel MM perf floor follow-up: tasks/TASK-0054D-ui-v1a-kernel-mm-perf-floor-vmo-surface-reuse.md
  - Renderer abstraction successor: tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - OS renderer wiring successor: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Production gates: tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
---

## Context

We need the first UI slice to be QEMU-tolerant and deterministic. The easiest way to build confidence
without kernel/display drivers is:

- a CPU renderer that draws into BGRA8888 buffers,
- stable “headless snapshot” tests on host,
- explicit damage tracking that later feeds a compositor.

This task is **host-first**. The OS compositor + surface IPC + VMO buffer sharing are in `TASK-0055`.

Sequencing note:

- `TASK-0054` stays intentionally kernel-free.
- If early QEMU fluidity and later blur/glass/animation work need a stronger kernel/perf floor, use the
  explicit follow-up slices `TASK-0054B` / `TASK-0054C` / `TASK-0054D` rather than stretching v1a itself.

Scope note:

- Renderer Abstraction v1 (`TASK-0169`/`TASK-0170`) supersedes the “ad-hoc cpu renderer” direction by introducing
  a stable Scene-IR + Backend trait with a deterministic cpu2d backend and goldens.
  If `TASK-0169` lands, this task should be treated as “implemented by” that work (avoid parallel renderer crates).

Current-state check (2026-04-27 review sync):

- `userspace/ui/renderer/` now exists as the narrow TASK-0054 renderer floor, not as the `TASK-0169` Scene-IR /
  Backend-trait architecture.
- `TASK-0169` and `TASK-0170` remain follow-up/successor scope. This task was not implemented by promoting
  `TASK-0169`.
- `Makefile` already exports `CARGO_TARGET_DIR=$(CURDIR)/target`, and `justfile` / `scripts/fmt-clippy-deny.sh`
  now default Cargo output to `<repo>/target` with `NEXUS_CARGO_TARGET_DIR` as the portable override.
- The task is Gate E (`Windowing, UI & Graphics`, `production-floor`) under
  `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`; it must not claim Gate A kernel `production-grade`
  closure. Kernel/core production-grade follow-ups stay in `TASK-0054B` / `TASK-0054C` / `TASK-0054D`,
  plus `TASK-0288` and `TASK-0290`.

## Goal

Deliver:

1. `userspace/ui/renderer` crate:
   - BGRA8888 framebuffer operations
   - text rendering with a single embedded fallback font
   - damage tracking (dirty rect accumulation)
2. `tests/ui_host_snap/`:
   - render fixed scenes
   - write PNGs and compare against goldens (pixel-exact first; SSIM optional follow-up)
3. Deterministic marker strings for later OS bring-up (not required to run in v1a).

## Non-Goals

- Kernel changes.
- A compositor.
- GPU acceleration.
- Input routing.
- OS/QEMU present markers or `windowd` wiring (handled by `TASK-0170` / `TASK-0055`).
- Production-grade kernel zero-copy, IPC, MM, or SMP claims (handled by `TASK-0054B` / `TASK-0054C` /
  `TASK-0054D`, then `TASK-0288` / `TASK-0290`).

## Constraints / invariants (hard requirements)

- Deterministic output for a fixed seed:
  - no time-based randomness,
  - stable font rasterization parameters,
  - stable pixel format and stride rules.
- Pixel format: **BGRA8888**.
- Stride alignment: 64-byte aligned rows (documented and tested).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No heavy dependency chains; keep renderer small and auditable.
- Host goldens must be deterministic across machines:
  - no host font discovery,
  - no locale-dependent text fallback,
  - no filesystem-order-dependent test discovery,
  - no gamma/iCCP-dependent PNG comparison behavior.
- Any optional SSIM path must be explicitly bounded and deterministic; pixel-exact goldens are the v1 proof floor.

## Alignment with `TRACK-DRIVERS-ACCELERATORS`

- **Buffers**: treat “framebuffers” as VMO/filebuffer-backed memory (even on host we can emulate VMO maps).
- **Sync**: do not invent new fence semantics in v1a; renderer is pure compute.
- **Budgets**: enforce hard bounds on image sizes and allocations.

## Alignment with `NexusGfx` architecture docs

- This task should be treated as an **early structural slice** of the future `NexusGfx` CPU reference path, not as a
  parallel long-term renderer architecture.
- Text rendering/materialization must align with `docs/architecture/nexusgfx-text-pipeline.md` and the canonical UI
  layout/text contracts it references.
- Resource and executor assumptions should remain portable to later `TASK-0169` / `TRACK-NEXUSGFX-SDK` work.

## Security considerations

This task is host-first and kernel-free, but it still handles untrusted-shaped rendering inputs and file-backed
golden fixtures. Treat it as production-floor UI infrastructure, not as a toy renderer.

### Threat model

- **Resource exhaustion**: oversized frames, images, glyph runs, or damage lists causing unbounded allocation or CPU work.
- **Golden update abuse**: accidental or malicious regeneration masking rendering regressions.
- **Path traversal / fixture drift**: snapshot tooling reading or writing outside the expected `tests/ui_host_snap/`
  tree.
- **Renderer/input confusion**: text/image APIs accepting malformed dimensions, strides, or coordinates that produce
  out-of-bounds writes.
- **Future authority drift**: introducing renderer APIs that assume direct MMIO/GPU/device authority or a second
  surface/present contract.

### Security invariants (MUST hold)

- Frame dimensions, stride, image sizes, glyph counts, and damage rect counts are bounded and reject with stable errors.
- All pixel writes are bounds-checked through safe APIs; no unsafe code is needed for v1a.
- Golden updates require an explicit opt-in such as `UPDATE_GOLDENS=1`; normal tests must never rewrite goldens.
- Snapshot fixture paths stay under the test fixture root; no absolute paths or `..` traversal.
- The renderer remains pure compute: no direct device/MMIO/IRQ authority, no present ownership, no policy bypass.

### DON'T DO

- DON'T add a GPU, `wgpu`, display, or device-service path in this task.
- DON'T discover system fonts or depend on host fontconfig/locale state.
- DON'T silently accept oversized images or damage growth by clipping after allocation.
- DON'T print success markers for OS present/compositor behavior from host-only code.
- DON'T duplicate the future `TASK-0169` Scene-IR/Backend architecture if that task is selected as the implementation path.

## Red flags / decision points (must be resolved before implementation)

- **RED: `TASK-0169` overlap**:
  - If the desired direction is Scene-IR + Backend trait, run `TASK-0169` instead and mark this task as implemented-by
    that work.
  - If this task proceeds, keep the API intentionally narrow (`Frame`, primitives, `Damage`) and document it as the
    host proof floor that `TASK-0169` may absorb.
- **RED: font determinism**:
  - Do not use host fonts. Use a repo-owned fallback font fixture or a tiny deterministic bitmap/vector test font.
  - If a production-quality fallback font cannot be included safely, make the text proof a deterministic fixture-font
    proof and create a follow-up for full font fallback.
- **RED: PNG dependency and cross-platform comparison**:
  - Prefer a minimal PNG encoder/decoder dependency with explicit rules, or write PNG output through a small
    deterministic helper.
  - Ignore metadata/gamma/iCCP for comparisons; compare decoded BGRA/RGBA pixels or a deterministic raw buffer.
- **RED: root workspace touch**:
  - Adding crates/tests requires updating root `Cargo.toml`, which is protected by project rules. The implementation
    plan must call this out explicitly before editing.
- **RED: production-grade claim boundary**:
  - This task can satisfy Gate E `production-floor` host renderer evidence only. Kernel/core `production-grade`
    UI claims remain blocked on `TASK-0054B` / `TASK-0054C` / `TASK-0054D`, `TASK-0288`, and `TASK-0290`.

## Production gate mapping

Per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`, this task belongs to Gate E
(`Windowing, UI & Graphics service floor`, `production-floor`).

This task contributes:

- deterministic host renderer goldens,
- bounded resource/damage behavior,
- documented pure-compute renderer boundaries,
- and an honest CPU baseline for later present/input/perf work.

This task does **not** close:

- Gate A kernel/core production-grade zero-copy or IPC/MM/SMP behavior,
- OS present/input marker closure,
- GPU/accelerator backend readiness,
- or consumer smoothness/perf budgets beyond bounded host scenes.

## Stop conditions (Definition of Done)

### Proof — required (host)

`cargo test -p ui_host_snap` green:

- renderer draws expected pixels for:
  - clear
  - rect
  - rounded-rect (simple coverage)
  - blit (from an in-memory image)
  - text (hello world)
- damage tracking:
  - rect ops add expected damage boxes
  - multiple ops coalesce/limit rect count deterministically
- snapshot tests:
  - produce PNGs
  - compare to goldens (pixel-exact or SSIM threshold if implemented)
- reject tests:
  - oversize frame/image rejects before allocation,
  - invalid stride/dimensions reject with stable error,
  - damage rect overflow coalesces or rejects deterministically,
  - golden update is disabled unless `UPDATE_GOLDENS=1`,
  - fixture path traversal / absolute write targets reject.

### Review evidence (2026-04-27)

Green host proof commands:

- `cargo test -p ui_renderer -- --nocapture` — 3 tests.
- `cargo test -p ui_host_snap -- --nocapture` — 24 tests.
- `cargo test -p ui_host_snap reject -- --nocapture` — 14 reject-filtered tests.
- `just diag-host` — green host diagnostics compile gate.

Proof notes:

- The chosen route is the narrow TASK-0054 route: `Frame`, BGRA8888 primitives, deterministic fixture-font text,
  and bounded `Damage`.
- The renderer crate uses `#![forbid(unsafe_code)]`, checked newtypes for frame/image dimensions, stride, and damage
  count, explicit owned buffers, and no global mutable renderer state.
- Damage overflow deterministically coalesces to full-frame damage.
- Snapshot comparison uses canonical BGRA pixels under `tests/ui_host_snap/goldens/`; PNG files are deterministic
  artifacts, and gamma/iCCP metadata is ignored by decode/compare proof.
- Golden update and absolute/traversal paths reject unless explicitly allowed through the update path; the safe update
  path is proven under `target/ui_host_snap_artifacts/<pid>`.
- The closure review added full rounded-rect/text masks, blit clipping with padded source stride, exact buffer-length
  accept/reject coverage, oversized height rejects, malformed fixture-font rejects, and an anti-fake-marker source scan.
- No OS/QEMU markers, `windowd`, compositor, GPU, MMIO/IRQ, scheduler, MM, IPC, VMO, or timer changes were introduced
  or claimed.

## Touched paths (allowlist)

- `userspace/ui/renderer/` (new crate)
- `userspace/ui/fonts/` (embedded fallback font data)
- `tests/ui_host_snap/` (new)
- `docs/dev/ui/foundations/quality/testing.md` (new)
- `Cargo.toml` (workspace membership for new crates/tests; protected path, must be explicitly justified before edit)
- `Cargo.lock` (workspace package metadata for the new `ui_renderer` / `ui_host_snap` members)
- `CHANGELOG.md` / `tasks/IMPLEMENTATION-ORDER.md` / `tasks/STATUS-BOARD.md` (closeout sync only, if task status changes)

## Plan (small PRs)

1. **Renderer core**
   - `Frame` with BGRA8888 pixels and stride
   - primitives: clear/rect/round_rect/blit/text
   - `Damage` accumulator with bounded rect count (e.g., `SmallVec<[IRect; 4]>`)

2. **Host snapshot tests**
   - goldens stored under `tests/ui_host_snap/goldens/`
   - deterministic rendering inputs (fixed font, fixed rasterization settings)
   - comparison:
     - pixel-exact first
     - optional SSIM tolerance as follow-up if minor differences are unavoidable across platforms

3. **Docs**
   - how to update goldens
   - how to add new cases
