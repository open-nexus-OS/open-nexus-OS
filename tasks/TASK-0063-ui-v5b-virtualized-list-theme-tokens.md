---
title: TASK-0063 UI v5b: virtualized list widget + theme/design tokens v1 (light/dark) + live switching
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v5a runtime baseline: tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
  - UI v3b scroll/clip baseline: tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - UI v4a tiling baseline (perf): tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - Config broker (theme overrides): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (audit theme changes): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

After v5a establishes a reactive runtime and timeline, v5b adds two “productivity multipliers”:

- a virtualized list for large content (recycling, bounded surfaces),
- theme tokens v1 (light/dark) with live switching via config.

This task is v5b (components + theming). It assumes scroll/clip and present scheduler exist.

## Goal

Deliver:

1. `userspace/ui/widgets/virtual_list`:
   - stable visible-range computation
   - recycling pool for item surfaces
   - bounded cache sizes and deterministic reuse
2. `userspace/ui/theme`:
   - roles/tokens schema and loader
   - light/dark modes and overrides
   - notification to dependents (signal-based)
3. Live theme switching:
   - via `configd` 2PC (host-first; OS-gated)
   - audited change events
4. Host tests and OS markers.

## Non-Goals

- Kernel changes.
- A full design system; tokens v1 are minimal and strictly versioned.

## Constraints / invariants (hard requirements)

- Deterministic virtualization behavior (given viewport/scroll, visible range is stable).
- Bounded memory:
  - cap pool size and cached surfaces
  - cap theme token sizes and parsed tree depth
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (config dependency)**:
  - Live switching depends on `configd` and `/state/config` overrides being real; host tests must simulate this cleanly.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v5b_host/`:

- virtual list:
  - 1000 items + small viewport → stable visible range
  - scrolling by N viewports triggers bounded recycle events
- theme:
  - load tokens (default + override)
  - role-to-RGBA mapping stable
  - switching notifies dependents exactly once per commit

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `ui: virtual list on`
- `virtualize: mount(<n>)`
- `virtualize: recycle(<n>)`
- `uitheme: loaded (mode=light|dark)`
- `uitheme: switched (to=dark)`
- `SELFTEST: ui v5 virtualize ok`
- `SELFTEST: ui v5 theme ok`

## Touched paths (allowlist)

- `userspace/ui/widgets/virtual_list/` (new)
- `userspace/ui/theme/` (new)
- `schemas/ui.tokens.schema.json` (new)
- `tests/ui_v5b_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v5b.sh` (delegates)
- `docs/ui/widgets/virtual-list.md` + `docs/ui/theme.md` (new)

## Plan (small PRs)

1. virtual list widget + markers
2. theme tokens v1 + schema + markers
3. live switching via config (gated) + audit events
4. host tests + OS markers + docs
