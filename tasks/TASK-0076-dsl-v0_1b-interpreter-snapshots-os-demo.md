---
title: TASK-0076 DSL v0.1b: IR interpreter bridge + headless snapshots + SystemUI demo + OS markers/postflight
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v0.1a foundations: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - UI runtime baseline: tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
  - UI layout baseline: tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - UI kit baseline: tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
  - Theme tokens baseline: tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With DSL syntax and IR in place (v0.1a), v0.1b makes it real:

- interpret IR into the existing runtime/layout/kit,
- headless snapshot testing,
- a demo DSL app launched from SystemUI to get QEMU markers.

## Goal

Deliver:

1. `userspace/dsl/nx_interp`:
   - walk Scene-IR and instantiate:
     - runtime signals/derived/effects
     - layout nodes
     - kit primitives
   - wire events (tap) and two-way bindings (TextField) minimally
   - theme read-through (and `.nxtheme` read-only override, optional)
   - marker: `dsl: interpreter on`
2. `nx dsl snapshot`:
   - render headless scenes via interpreter into PNGs
   - store under `target/nxir/snapshots/`
3. Example app `dsl_hello`:
   - a page with a TextField, cards/grid, and one button
   - demonstrates state binding and a computed signal
4. SystemUI integration:
   - add “DSL Demo” entry that launches and mounts the interpreter output into a managed window
   - markers:
     - `dsl: demo launched`
     - `dsl: first frame ok`
5. Host tests + OS selftests + postflight.

## Non-Goals

- Kernel changes.
- Full language features (loops, macros, user-defined functions).
- Full theme override semantics.

## Constraints / invariants (hard requirements)

- Deterministic rendering for snapshots (stable rounding and ordering).
- Bounded interpreter work per frame (caps on node count and event queue).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_1b_host/`:

- IR → interpreter snapshot PNGs match goldens (pixel-exact preferred; SSIM threshold if needed and documented)
- two-way binding: simulated TextField input updates state and causes deterministic re-render

### Proof (OS/QEMU) — gated

UART markers:

- `dsl: interpreter on`
- `dsl: demo launched`
- `dsl: first frame ok`
- `SELFTEST: dsl v0.1 demo ok`
- `SELFTEST: dsl v0.1 binding ok`

## Touched paths (allowlist)

- `userspace/dsl/nx_interp/` (new)
- `tools/nx-dsl/` (extend: snapshot)
- `userspace/apps/examples/dsl_hello/` (new)
- SystemUI launcher entries (demo hook)
- `tests/dsl_v0_1b_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-dsl-v0-1.sh` (delegates)
- `docs/dsl/testing.md` (new/extend)

## Plan (small PRs)

1. interpreter bridge + mount helpers + marker
2. nx dsl snapshot + host snapshot tests + goldens
3. example app + SystemUI integration + markers
4. OS selftests + docs + postflight
