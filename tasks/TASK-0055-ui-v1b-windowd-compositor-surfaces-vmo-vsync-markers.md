---
title: TASK-0055 UI v1b (OS-gated): windowd compositor + surfaces/layers IPC + VMO buffers + vsync timer + markers
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI track dependencies: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers (vsync spine): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Config broker (ui profile): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (permissions): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Logging/audit sink: tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - UI v1a renderer: tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want the first UI slice runnable in QEMU **without kernel display/input drivers**. That implies:

- “headless present” is acceptable: compose into a VMO-backed framebuffer and emit deterministic markers
  (and optionally export snapshots to `/state` once persistence exists).
- control plane uses typed IPC (Cap’n Proto) consistent with Vision.
- data plane uses VMO/filebuffer for shared buffers.

This task is OS-gated on VMO plumbing and a timing spine.

Scope note:

- Renderer Abstraction v1 (`TASK-0169`/`TASK-0170`) defines the Scene-IR + Backend trait and the deterministic cpu2d default.
  `windowd` composition should call into that backend rather than inventing separate rendering primitives.

## Goal

Deliver:

1. Surface/layer IPC contracts (Cap’n Proto) for:
   - creating surfaces with VMO buffers,
   - queueing buffers with damage,
   - an atomic scene commit,
   - vsync subscription (events),
   - input stubs (no routing yet).
2. `windowd` compositor:
   - manages a layer tree,
   - composites on a vsync tick (default 60Hz),
   - is damage-aware (skip present if nothing changed),
   - signals a minimal “present fence” (v1 semantics).
3. SystemUI host concept:
   - minimal “desktop/mobile” plugins may start as in-process modules (v1),
   - later extracted to separate processes once plugin ABI is ready.
4. OS selftest markers + postflight (delegating to canonical harness).

## Non-Goals

- Kernel changes.
- Real display output or virtio-gpu integration.
- Real input routing and focus (stubs only in v1).
- A full plugin ABI system (v1 can keep it simple).

## Constraints / invariants (hard requirements)

- Must not invent a parallel buffer/sync model:
  - use VMO handles for buffers (TRACK contract),
  - vsync driven by timed service / monotonic timer (QoS/timers contract),
  - fences must be bounded and auditable.
- Bounded composition:
  - cap number of surfaces,
  - cap layer depth,
  - cap pixel dimensions and total bytes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Deterministic markers in QEMU.

## Red flags / decision points

- **RED (VMO availability)**:
  - Buffer sharing depends on `TASK-0031` semantics being proven in QEMU.
  - If VMO transfer/mapping is incomplete, v1b must either gate the feature or use a copy fallback
    (explicitly documented as “non-zero-copy fallback”).
- **YELLOW (present fence semantics)**:
  - v1 can implement “fence” as a simple event/cap that is signaled after composition tick,
    but it must be clearly documented as minimal and not suitable for real latency-sensitive pipelines yet.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Host tests can be limited to protocol codec and in-proc composition (no QEMU):

- `tests/ui_windowd_host/`:
  - compose two surfaces with damage and verify resulting pixels match a golden.
  - verify “no damage → no present” behavior deterministically.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: ready (w=..., h=..., hz=60)`
- `windowd: systemui loaded (profile=desktop|mobile)`
- `windowd: present ok (seq=... dmg=...)`
- `launcher: first frame ok`
- `SELFTEST: ui launcher present ok`
- `SELFTEST: ui resize ok`

## Touched paths (allowlist)

- `source/services/windowd/` (new)
- `source/services/windowd/idl/` (new capnp)
- `userspace/apps/launcher/` (new demo client, minimal)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui.sh` (delegates)
- `scripts/qemu-test.sh` (marker list)
- `docs/ui/overview.md` + `docs/ui/testing.md` + `docs/ui/profiles.md`

## Plan (small PRs)

1. **IDLs**
   - `surface.capnp`, `layer.capnp`, `vsync.capnp`, `input.capnp` (stub).
   - VMO handle types and rights documented.

2. **`windowd` compositor**
   - layer tree + surface registry
   - vsync tick (60Hz default) using the timing spine
   - damage-aware composition using renderer primitives (from v1a)
   - markers: ready/systemui loaded/present ok

3. **Minimal launcher**
   - creates a surface
   - draws a simple scene via CPU renderer into its VMO buffer
   - queues buffer with damage
   - marker `launcher: first frame ok`

4. **Config + policy**
   - config schema for `ui.profile` and display dimensions (host-first, OS-gated)
   - policy permissions for reading assets and spawning plugins (minimal)

5. **Proof**
   - host tests for composition snapshots
   - OS selftest markers and postflight-ui
