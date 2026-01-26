---
title: TASK-0077C DSL v0.2c Pro UI: virtualized table/grid + timeline primitives + NativeWidget “blessed path” (bounded, deterministic)
status: Draft
owner: @ui
created: 2026-01-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v0.2a core: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - DSL v0.2b stubs/CLI: tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
  - DSL v1 DevX track: tasks/TRACK-DSL-V1-DEVX.md
  - Zero-copy app platform (hard apps): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Zero-copy VMOs (data plane gate): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - UI virtual list/tokens baseline: tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
  - UI design system primitives: tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
  - NativeWidget contract: docs/dev/dsl/syntax.md
---

## Context

Hard apps (Office/BI, DAW, Live Studio, Video Editor) need a few “pro surfaces” that are not realistic to express
purely as DSL node trees without becoming QML-like. We keep the DSL bounded and provide:

- a small set of pro primitives (virtualized table/grid, timeline shell),
- and a **blessed NativeWidget path** for heavy interactive canvases.

## Goal

Deliver v1-ready pro UI building blocks that keep apps “first-party” and performant:

1. **Virtualized table/grid contract**:
   - stable keys and deterministic ordering
   - bounded row/column counts per viewport
   - paging tokens and backpressure hooks
2. **Timeline primitives contract** (shared shape across Slides/Video/DAW):
   - tracks + clips + keyframes are data; rendering is bounded
   - deterministic zoom/scroll mapping and selection semantics
3. **NativeWidget blessed path**:
   - recommended widgets (timeline canvas, waveform/meters, video preview surface)
   - strict constraints: deterministic, bounded per frame, no direct IO (svc.* in effects only)
   - snapshot/golden strategy for these widgets (host-first)

## Non-Goals

- Turning the DSL into a full scripting language for pro tools.
- Implementing full Office/DAW/Video functionality in this task; this is the UI substrate only.

## Constraints / invariants (hard requirements)

- Virtualization is mandatory for large collections (lint warns/errors where applicable).
- Deterministic input replay possible for pro widgets (bounded event streams).
- Bounded CPU/memory per frame; caches must have explicit budgets/eviction.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_pro_primitives_host/` (or equivalent):

- virtual table renders deterministically for fixtures (goldens)
- timeline widget renders deterministic frame sequences for a scripted interaction fixture
- NativeWidget surfaces obey boundedness constraints (test harness enforces budgets)

## Touched paths (allowlist)

- `userspace/ui/kit/` and/or `userspace/ui/widgets/` (pro primitives as appropriate)
- `userspace/dsl/nx_interp/` (wiring to render/host these primitives)
- `tests/ui_pro_primitives_host/`
- `docs/dev/ui/` (table/timeline widget docs) + `docs/dev/dsl/` (NativeWidget “blessed path” guidance)
