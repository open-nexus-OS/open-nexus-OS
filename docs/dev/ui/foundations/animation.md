<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Animation

Animation should be:

- deterministic for goldens (where asserted),
- bounded in work per frame,
- accessible (reduced motion settings where applicable).

See also:

- `docs/dev/ui/foundations/quality/performance-philosophy.md` for the default frame-budget/degrade stance,
- `docs/dev/ui/foundations/quality/testing.md` for how animation scenes should be exercised.

## Default posture

The default animation model should follow the **idea** of SwiftUI more than CSS:

- animation is usually attached to a **state/value change**,
- transitions are a separate category from ordinary value animation,
- attention effects are explicit rather than hidden in global styles,
- and motion stays token-driven, bounded, and backend-neutral.

We want the **semantics** of SwiftUI with a **shorter authoring syntax** than SwiftUI's default API surface.

## Motion categories

Use three categories:

1. **Value/state animation**
   - a property change is animated because a state value changed
   - examples: opacity, scale, position offset, sheet progress, token-driven color/elevation changes
2. **Transition**
   - insert/remove/open/close/navigation visibility changes
   - examples: fade in, slide up, fade+scale on mount/unmount
3. **Attention effect**
   - explicit user-noticeable motion such as wiggle, shake, pulse
   - should normally be trigger-based rather than permanent

Do not collapse all three into one generic “animation” bucket. They have different intent, testing posture, and degrade
rules.

## Recommended DSL posture

Prefer concise modifier syntax with semantic tokens:

```nx
Button { label: "Continue" }
  .animate(snappy, value: $state.enabled)
  .transition(fadeScale)
  .effect(wiggle, trigger: $state.nudgeTick)
```

Canonical form may still lower to a `modifier { ... }` block, but chaining is the ergonomic surface.

Recommended meaning:

- `.animate(token, value: expr)`
  - animate state/value-driven property changes
- `.transition(token)`
  - define how a node enters/leaves or opens/closes
- `.effect(token, trigger: expr)`
  - run an explicit bounded effect when the trigger changes

This gives us:

- SwiftUI-like state-driven semantics,
- shorter authoring than verbose animation builders,
- and a deterministic separation between ordinary UI motion and attention effects.

## Why not CSS-style `@keyframes` first

Do not make the default model look like free-form CSS animation:

- global `@keyframes` naming and ad-hoc `--animate-*` variables are too web-specific,
- free-form keyframes make backend-neutral behavior harder,
- and “infinite utility animation everywhere” conflicts with our idle-cost and reduced-motion posture.

Illustrative anti-default:

```css
--animate-wiggle: wiggle 1s ease-in-out infinite;
@keyframes wiggle { ... }
```

That style may inspire ergonomics, but it should not be the core contract for this DSL.

## Boundedness and determinism rules

Animation rules must stay compatible with the retained layout/runtime contract:

- prefer animating **paint-only** and **place-only** properties,
- animate measure-affecting properties only with explicit justification,
- use deterministic clocks/timelines in tests,
- coalesce same-frame changes,
- and degrade explicitly when over budget.

Default property posture:

- good default: opacity, transform-like offset/scale, token-driven elevation/emphasis, sheet/sticky progress
- acceptable with care: reserved-size expand/collapse where invalidation stays local
- avoid as a default: free-running layout thrash, text reflow animation, anything that forces repeated full-tree measure

## Reduced motion

Reduced motion is part of the design contract, not an afterthought.

Recommended fallback order:

- replace effect motion with a smaller fade/emphasis change,
- reduce repeat counts and distances,
- shorten or remove non-essential transition motion,
- preserve layout/interaction semantics even when motion is reduced.

For example, `wiggle` under reduced motion should normally degrade to a bounded emphasis/fade pulse rather than keep
rotating forever.

## Attention effects

Effects like `wiggle`, `shake`, `pulse`, or `bounce` should be:

- explicit,
- bounded,
- and usually trigger-based.

Recommended posture:

- prefer `.effect(wiggle, trigger: expr)` over a permanent `.wiggle`,
- stop after a bounded repeat count unless the effect is a documented exception,
- suspend or avoid work when offscreen/inactive where possible,
- and never let “attention” effects dominate idle cost.

## Keyframes posture

Keyframes may exist later, but as a **typed, bounded motion spec**, not as a CSS sublanguage.

Illustrative later direction:

```nx
motion Wiggle {
  duration: 600ms
  repeat: 2
  keyframes {
    0%   { rotate(-3deg) }
    50%  { rotate(3deg) }
    100% { rotate(-3deg) }
  }
}
```

Then:

```nx
Button { label: "Continue" }
  .effect(Wiggle, trigger: $state.nudgeTick)
```

This keeps motion specs typed, namespaced, and compatible with reduced-motion and backend-neutral lowering.

## Testing posture

Animation tests should prefer:

- deterministic fixture state,
- sampled/frozen frames at named times,
- stable reduced-motion behavior,
- and explicit acceptance of which scenes assert goldens vs only behavior.

Typical proof shapes:

- “given state change X, property Y reaches expected sampled values”
- “transition token Z yields deterministic enter/exit frames”
- “effect trigger changes once -> effect runs once with bounded repeats”

## Recommended v1 scope

For v1, keep the motion surface intentionally small:

- `animate(token, value: expr)`
- `transition(token)`
- `effect(token, trigger: expr)`

with a curated token set such as:

- `snappy`
- `smooth`
- `emphasized`
- `fade`
- `slideUp`
- `fadeScale`
- `wiggle`
- `pulse`

Anything beyond that should justify itself as a concrete product need rather than arriving as an abstract animation
system.
