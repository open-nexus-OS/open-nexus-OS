<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Goldens

Goldens are deterministic proof artifacts (images, hashes, or derived views) used to prevent UI drift.

## What counts as a golden

- snapshot PNG (pixel-exact preferred)
- stable hash of snapshot output
- deterministic derived views (e.g., `.nxir.json` for IR debug)

## Guidelines

- keep fixtures small and bounded
- avoid nondeterminism (wallclock, random seeds, host locale)
- store goldens near the feature’s tests (or in a well-known `goldens/` subtree)

## TASK-0054 BGRA snapshot goldens

`TASK-0054` / `RFC-0046` uses repo-owned goldens under
`tests/ui_host_snap/goldens/` for the host CPU renderer proof floor.

Rules:

- equality is based on canonical decoded BGRA8888 pixels,
- PNG files are deterministic artifacts only,
- PNG metadata such as gamma or iCCP chunks must not affect equality,
- normal test runs must not rewrite goldens,
- updates require an explicit `UPDATE_GOLDENS=1` run,
- update paths must remain under the approved golden/artifact root.

## TASK-0073 design-system component goldens + a11y lints

`tests/ui_v10_goldens/` proves the design-system primitives (RFC-0070). It reuses the
`ui_host_snap` golden machinery (canonical BGRA hex compare + `UPDATE_GOLDENS=1` gate):

- **Pipeline:** component builder → `LayoutNode` → `LayoutEngine` → a small structural
  `LayoutResult` painter (rounded fills + square borders) → `ui_renderer::Frame` → BGRA hex golden
  under `tests/ui_v10_goldens/goldens/`. The painter is intentionally structural — backdrop blur and
  text are validated separately, so goldens lock **geometry + resolved colors** deterministically.
- **Coverage:** core primitives (GlassButton variants + hover/pressed/disabled/focus states, Badge,
  GlassToggle on/off, GlassCard levels, AppIcon variants) in light **and** dark themes.
- **A11y lints** (`tests/a11y.rs`, pixel-free — computed from tokens + layout):
  - **Contrast** — WCAG relative-luminance ratio. Body-text pairs (`OnSurface`/`Surface`,
    `OnPrimary`/`Primary`) must clear **4.5** (1.4.3); filled-control pairs (`OnAccent`/`Accent`,
    `OnDanger`/`Danger`, `OnSuccess`/`Success`, `OnWarning`/`Warning`) must clear **3.0** (1.4.11).
    This lint caught + fixed white-on-`#22c55e` (2.28) → `success` retuned to green-600 `#16a34a`.
  - **Touch target** — interactive roots must be ≥ `MIN_TOUCH` (24px desktop floor; 44 is the WCAG
    2.5.5 enhanced ideal).
- **Regenerate:** `UPDATE_GOLDENS=1 cargo test -p ui_v10_goldens`; commit the updated `*.bgra.hex`.
  A plain `cargo test -p ui_v10_goldens` is the drift gate.
