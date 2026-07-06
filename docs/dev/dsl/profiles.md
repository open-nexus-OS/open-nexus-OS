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
