<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Keystone Map (SDK + “Hard Apps” → v1 Readiness)

This page documents the **keystone primitives** that make the “hard apps” feasible:

- Office Suite (Word/Sheets/Slides)
- DAW
- Live Studio
- Video Editor

If these apps are buildable and performant under our constraints, the ecosystem story is “real”.

Scope: this is a **planning/architecture map**, not an implementation task.

## Core stance

- The DSL is not meant to become a full general-purpose language.
- We build “pro workloads” by combining:
  - a deterministic DSL (UI + state + effects),
  - typed service stubs (`svc.*`) and builder specs,
  - and **native widgets** for heavy interactive surfaces (timeline/canvas/waveforms).

## Keystone primitives (cross-track)

### Keystone 1: Hybrid control/data plane

Single source of truth: `docs/adr/0017-service-architecture.md`.

- Control plane: small structured IPC (typed).
- Data plane: bulk buffers via VMO/filebuffer (bounded).

This is required for:

- media frames/audio buffers,
- large datasets (Sheets/BI),
- large documents and previews,
- capture/compose/export pipelines.

### Keystone 2: Zero-copy bulk handle transfer (VMO/filebuffer)

Single source of truth task: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`.

This is the “truth moment” for claiming zero-copy across the platform.
Everything “pro” depends on it:

- NexusGfx (buffers/images)
- NexusMedia (frames/audio buffers)
- DAW (plugin isolation + shared audio buffers)
- Live Studio (capture → compose → encode pipelines)
- Video Editor (preview/export caches)
- Office Suite (large workbook/deck buffers, fast open/save paths)

### Keystone 3: Timelines + deterministic scheduling

Timelines appear in multiple domains:

- DAW: transport + automation lanes + MIDI timing
- Live Studio: A/V sync, scene transitions, streaming cadence
- Video Editor: NLE timeline + preview frames + export
- Slides: tracks/keyframes/triggers (motion)

We should avoid “four different timeline engines”. Prefer shared contracts:

- stable time model (ticks, frames, beats)
- bounded queues and deterministic ordering
- explicit deadlines/backpressure where real-time applies

### Keystone 4: Autosave + recovery + audit (OpLog + snapshots)

From the zero-copy app platform direction:

- append-only OpLog (human-readable, bounded)
- typed snapshots (fast load, deterministic replay)
- deterministic crash recovery (host-first harness)

This is mandatory for:

- Office Suite (“Save is legacy UX”),
- DAW projects,
- video editor projects,
- live studio scene setups.

### Keystone 5: Deterministic proofs (host-first)

Across hard apps we rely on proofs like:

- “edit script → frame hash sequence”
- “project script → audio checksum”
- goldens for UI primitives and key surfaces

This keeps correctness real even before OS/QEMU performance is meaningful.

## What must be “DSL v1 prepared”

### DSL should provide

- Components + props (primary composition model)
- Builder specs (typed outside, executed in effects/services; bounded)
- Slots for a small set of primitives (List/Table/Menu/Form), used sparingly
- `svc.*` calls only in effects/services with explicit bounds (timeouts/bytes/rows)
- Deterministic formatting/lowering/IR for goldens

### DSL should NOT try to be

- a full scripting language for DAW/BI/Video
- a place where we run heavy decode/encode pipelines
- a general-purpose plugin runtime

### Native widgets are expected (escape hatch)

For pro apps, we should assume NativeWidgets exist for:

- timeline canvas (zoom/scroll/selection)
- waveform view + meters
- video preview surface
- charting and large tables (virtualized)

The DSL remains the “shell”: layout, inspectors, toolbars, routing, state, and effects.

## Track alignment (who consumes what)

- Zero-copy app platform: `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md`
- NexusNet SDK: `tasks/TRACK-NEXUSNET-SDK.md`
- NexusMedia SDK: `tasks/TRACK-NEXUSMEDIA-SDK.md`
- NexusGfx SDK: `tasks/TRACK-NEXUSGFX-SDK.md`
- NexusGame SDK: `tasks/TRACK-NEXUSGAME-SDK.md`
- Office Suite: `tasks/TRACK-OFFICE-SUITE.md`
- DAW: `tasks/TRACK-DAW-APP.md`
- Live Studio: `tasks/TRACK-LIVE-STUDIO-APP.md`
- Video Editor: `tasks/TRACK-VIDEO-EDITOR-APP.md`
