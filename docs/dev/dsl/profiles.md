<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Profiles & Device Environment

The runtime exposes a small, read-only device environment to support cross-device UI deterministically:

- `device.profile`: `{ phone, tablet, desktop, tv, auto, foldable }`
- `device.sizeClass`: `{ compact, regular, wide }`
- `device.dpiClass`: `{ low, normal, high }`
- `device.input`: flags `{ touch, mouse, kbd, remote, rotary }`

## Deterministic overrides

If present:

- `ui/platform/<profile>/pages/<Page>.nx` overrides `ui/pages/<Page>.nx`
- `ui/platform/<profile>/components/<Comp>.nx` overrides `ui/components/<Comp>.nx`

Rules:

- fixed precedence (no filesystem iteration dependence)
- conflicts are lint errors

## Inline branching (canonical)

Use `@when` as the canonical conditional construct for UI branching:

```nx
@when device.profile == phone {
  Stack { /* phone layout */ }
}
@when device.profile == tablet {
  Stack { /* tablet layout */ }
}
@else {
  Stack { /* default layout (desktop/tv/auto/...) */ }
}
```

Semantics:

- evaluated top-to-bottom, **first match wins**
- `@else` is the fallback branch (at most one)

Lint posture:

- For profile-driven layout branching, missing `@else` is a **warning** by default (upgradeable via `--deny-warn`).

## `match` (syntactic sugar)

`match(device.profile) { ... else ... }` is allowed as syntactic sugar and lowers to an equivalent `@when` chain.
It must not introduce different semantics.

Example (illustrative):

```nx
match(device.profile) {
  phone => Stack { /* phone layout */ }
  tablet => Stack { /* tablet layout */ }
  else => Stack { /* default */ }
}
```
