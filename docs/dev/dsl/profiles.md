<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Profiles & Device Environment

Every program runs against a small, **read-only** device environment so one codebase
serves phone/tablet/desktop/tv/auto/foldable deterministically:

- `device.profile` — validated profile id: `{ phone, tablet, desktop, tv, auto, foldable, convertible }`
  baseline; products may add validated ids via manifests
- `device.posture` — `{ flat, half_fold, tent, book }` (only meaningful when foldable)
- `device.orientation` — `{ portrait, landscape }`
- `device.shellMode` — validated shell id (an explicit operating mode — e.g. a
  convertible switching between desktop and tablet shells — never a hardware proxy)
- `device.sizeClass` — `{ compact, regular, wide }`
- `device.dpiClass` — `{ low, normal, high }`
- `device.input` — flags `{ touch, mouse, kbd, remote, rotary }`

## Where the values come from (SSOT)

The platform's **shell-config registry** (`source/services/systemui/manifests/`,
ADR-0035) is the single source: a *product* manifest selects the profile, shell, and
theme; the *profile* manifest carries input capabilities and display defaults
(orientation/dpi class/size class); the *shell* manifest names the shell program
(`dsl_root`) and its first-frame geometry. The runtime derives `device.*` from the
resolved chain. Host tests inject fixture environments; the contract is identical.

Unknown ids or incompatible profile/shell pairings are rejected deterministically.

## Deterministic file overrides (default UI + per-device variants)

Write the page once for the default; add a variant file only where a device class
needs a structurally different layout:

- `ui/platform/<profile>/pages/<Page>.nx` overrides `ui/pages/<Page>.nx`
- `ui/platform/<profile>/components/<Comp>.nx` overrides `ui/components/<Comp>.nx`

Rules:

- fixed precedence, resolved at `nx dsl build` (no filesystem-order dependence);
  the chosen source is recorded in the IR (provenance);
- conflicts/ambiguity are lint errors; a missing override falls back cleanly;
- overrides are **profile-keyed only** — orientation/shell-mode differences use
  inline branching.

## Inline branching

Plain `if/else` on the environment (evaluated top-to-bottom):

```nx
if device.profile == phone {
    Stack { /* phone layout */ }
} else if device.profile == tablet {
    Stack { /* tablet layout */ }
} else {
    Stack { /* default (desktop/tv/auto/…) */ }
}
```

`match` is available and must be exhaustive. Lint: `if` on `device.profile` without a
final `else` is a **warning** by default (`--deny-warn` promotes) — a device you did
not think of gets the default branch, not a blank screen.

## Guidance

- Apps branch on responsive layout (`sizeClass`) and `device.profile` first.
- Only shell-owned surfaces should branch on `device.shellMode`.
- Never assume the baseline ids are the only valid ids — products extend the registry
  declaratively.

## Changelog

- **v1 (2026-07-06)** — environment SSOT documented (shell-config registry, ADR-0035);
  `if/else` replaces the former `@when/@else` form; `shellMode`/`posture`/
  `orientation` added; override provenance recorded in IR.

## Region axes: `device.locale` / `device.keymap` (RFC-0075 Phase 8b)

Runtime-varying axes fed by the windowd region push (`ui.locale` /
`input.keymap` settings): string equality in `if device.locale == "…"` /
`if device.keymap == "…"` arms, re-selected on reemit like a size-class
change. Use them for RARE structural decisions only — repeated per-language
content belongs in DATA (locale packs for text, `svc.ime.rows` for key
layouts), never in per-language `if` trees.

## Shell axis: `device.tilePreview` (TASK-0066 / ADR-0069)

The tile zone a window's title-bar drag would take if released now — windowd's
zone names (`left-half` · `right-half` · `top-half` · `bottom-half` ·
`top-left` · `top-right` · `bottom-left` · `bottom-right` · `fill`), `""` when
no drag is near an edge. Pushed by windowd to the DESKTOP surface only
(`OP_SURFACE_TILE_PREVIEW`, one push per change) and re-selected on reemit; the
desktop shell's `overlays/TilePreview.nx` paints the hint. Apps never see a
non-empty value — it is a shell axis, not an app one.

## Gesture axes: `device.dragX` / `dragY` / `dragStartX` / `dragStartY` (RFC-0095)

The pointer and the press of the drag gesture in flight, surface pixels (Int; 0 outside a
drag). The host fills them while a drag runs; a reducer bound to `on DragStart` / `DragMove` /
`DragEnd` reads them at dispatch time (`syntax.md`, "The drag gesture").
