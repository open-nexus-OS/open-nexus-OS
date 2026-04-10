---
title: TASK-0170B NexusGfx v1c (OS/QEMU-gated): windowd handoff + minimal pass planning + present completion alignment
status: Draft
owner: @ui @runtime
created: 2026-04-10
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusGfx track: tasks/TRACK-NEXUSGFX-SDK.md
  - Renderer abstraction host slice: tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Renderer abstraction OS wiring baseline: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - UI compositor baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - DriverKit core contracts: tasks/TASK-0280-driverkit-v1-core-contracts-queues-fences-buffers.md
  - NexusGfx v1b resource/fence core: tasks/TASK-0169B-nexusgfx-v1b-resource-fence-core-cpu-mock-submit.md
  - Gfx command/pass model: docs/architecture/nexusgfx-command-and-pass-model.md
  - Gfx sync/lifetime model: docs/architecture/nexusgfx-sync-and-lifetime.md
  - Gfx tile-aware design: docs/architecture/nexusgfx-tile-aware-design.md
---

## Context

`TASK-0170` wires `windowd` to the renderer backend and proves basic present behavior, but it does not yet lock the
portable handoff between UI composition and a future `NexusGfx` pass/submit model.

If that handoff stays implicit, later GPU or compute integration will either:

- force `windowd` to invent a second compositor contract, or
- require a broad migration after apps already depend on the first OS path.

This task is the small follow-up that keeps `windowd` honest:

- minimal pass planning,
- explicit present completion alignment,
- CPU2D-default execution,
- and no real GPU dependency.

## Goal

Deliver a bounded OS/QEMU-facing follow-up that:

1. adds a minimal pass-planning layer for `windowd` composition:
   - compose/copy/present ordering,
   - explicit damage-to-pass mapping,
   - stable pass ordering for the same scene input;
2. aligns present completion with the shared fence/completion model from `TASK-0169B`;
3. keeps CPU2D as the default executor while making the sequencing compatible with later `NexusGfx` backends;
4. proves the flow with deterministic host and/or QEMU markers.

## Non-Goals

- Real visible scanout (`TASK-0055B` / `TASK-0055C` own that direction).
- Tile-optimized or GPU-specific scheduling.
- Full swapchain/vsync-domain work (`TASK-0207` / `TASK-0208` family owns later windowing expansion).
- Rewriting `windowd` into a new graphics API authority.

## Constraints / invariants (hard requirements)

- `windowd` remains the compositor/present authority; `NexusGfx` remains the execution/resource vocabulary.
- Pass planning must be deterministic for the same surface/layer input.
- Completion/reporting must not invent ad-hoc events outside the shared fence/completion model.
- No fake success markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security considerations

### Threat model

- **Unbounded composition state**: too many surfaces/passes causing denial of service.
- **Completion confusion**: present reported before bounded work actually completed.
- **Contract drift**: `windowd` inventing private sequencing semantics that later bypass validation or policy.

### Security invariants (MUST hold)

- Surface counts, pass counts, bytes, and damage regions remain bounded.
- Present completion is tied to actual execution/completion state.
- `windowd` does not become a parallel policy or device authority.

### DON'T DO

- DON'T add a second fence or submit vocabulary for `windowd`.
- DON'T report `present ok` before the bounded completion path finishes.
- DON'T mix visible display work into this task unless required for proof and explicitly labeled.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Deterministic tests prove:
  - stable pass ordering for the same scene/layer input,
  - damage maps to the expected minimal pass plan,
  - completion state is propagated deterministically back to `windowd`.

### Proof (OS/QEMU) — gated

- If OS wiring is available, required markers are:
  - `windowd: compose begin`
  - `windowd: compose end`
  - `windowd: present ok`
  - `SELFTEST: renderer v1 present ok`

- Marker semantics must remain honest and may not imply visible display output or GPU acceleration.

## Touched paths (allowlist)

- `source/services/windowd/`
- `source/apps/selftest-client/`
- `tests/ui_windowd_host/` or similar host proof suite
- `docs/renderer/overview.md` or `docs/dev/ui/foundations/rendering/architecture.md`
- `tasks/TRACK-NEXUSGFX-SDK.md`

## Plan (small PRs)

1. Add minimal pass-plan structures and deterministic pass ordering rules.
2. Thread shared completion/fence semantics through `windowd` present flow.
3. Add host proofs, then QEMU markers only where the OS path is already real.
4. Document the handoff boundary so later GPU backends replace execution only, not compositor semantics.
