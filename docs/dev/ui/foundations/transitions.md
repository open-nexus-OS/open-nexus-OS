<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Transitions

Transitions should be:

- deterministic where asserted (goldens),
- bounded in work per frame,
- consistent across input modalities.

## Purpose

Transitions are the motion contract for:

- insert/remove,
- show/hide,
- open/close,
- route or sheet changes,
- and other visibility/lifecycle changes.

They are **not** the same thing as value animation or attention effects.

## Recommended DSL posture

Prefer a dedicated transition modifier:

```nx
Sheet { ... }
  .transition(slideUp)
```

or:

```nx
Toast { ... }
  .transition(fadeScale)
```

This keeps lifecycle motion separate from:

- `.animate(token, value: expr)` for state/value changes,
- `.effect(token, trigger: expr)` for explicit attention effects.

## Good default transition tokens

Recommended small starter set:

- `fade`
- `slideUp`
- `slideDown`
- `fadeScale`
- `pushInline`

The token set should stay small and reusable across apps/SystemUI.

## Determinism posture

Transitions should:

- use deterministic durations/curves/tokens,
- preserve stable placement rules,
- avoid hidden backend-specific motion differences,
- and degrade explicitly under reduced motion.

Reduced-motion fallback usually means:

- shorter duration,
- less travel distance,
- or fade-only/static emphasis where appropriate.
