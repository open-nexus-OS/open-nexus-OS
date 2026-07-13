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

## Implementation status (TASK-0062 / TASK-0075)

The motion surface below is **implemented** for in-surface DSL nodes: `.animate`,
`.transition`, and `.effect` (`userspace/dsl/core/src/registry.rs`, modifier ids
43/44/45) parse, validate their token against the curated set
(`check/names.rs::check_motion_token`), lower generically, and are now **bound at
runtime** end-to-end. This is **Tier 2** of the animation architecture (ADR-0031,
RFC-0059); **Tier 1** (compositor layer transforms for whole windows/layers, the
"same as scroll" `OP_SET_LAYER_TRANSFORM`) is the open Track C.

Pipeline (one engine, host owns the clock, the DSL stays pure —
`docs/dev/dsl/principles.md` §4):

1. **Stamp (DSL, pure).** Emit records a value-typed `AnimIntent{kind, token,
   value}` per animated node and marks the driving `value:`/`trigger:` expression
   a **Paint** dependency (`userspace/dsl/runtime/src/{anim,emit,view}.rs`;
   `View::animations()`). No clock, no physics in the DSL.
2. **Drive (app-host, Tier 2).** The app-host owns an
   `animation::AnimationDriver` (the OS-wide physics SSOT) and diffs the intents
   across re-emits: a changed value (re)starts the token's keyframe/spring;
   mounted resting states are seeded (`source/services/app-host/src/probe/anim.rs`,
   `anim_sync`). It is ticked on the compositor **Choreographer frame pulse**
   (`OP_SURFACE_FRAME`) — the exact cadence scroll momentum rides — so the app is
   out of the per-frame loop and the pulse re-arms only while motion is live.
   On the idle→active edge the driver's clock is re-seeded
   (`AnimationDriver::reset_clock`) so the first tick measures one frame, not the
   whole idle gap — otherwise the keyframe jumps straight to its end (an instant
   pop). Same tick-clock seed the scroll momentum does.
3. **Paint (scene-raster).** Each tick's `SceneUpdate`s fold into a per-node
   `NodeAnim` (opacity/translate/scale) the CPU painter applies
   (`userspace/ui/scene_raster/src/anim.rs`, `paint_row_picked_animated`).

Token → physics mapping (SSOT `userspace/ui/animation/src/motion.rs` +
`userspace/ui/theme-tokens/src/lib.rs`):

| Token | Category | Primary prop | Secondary | Duration | CPU easing |
|---|---|---|---|---|---|
| `snappy` | value | opacity | — | Swift (160ms) | ease-out |
| `smooth` | value | opacity | — | Base (400ms) | ease-in-out |
| `emphasized` | value | opacity | — | Slow (500ms) | ease-out |
| `fade` | transition | opacity | — | Quick (280ms) | ease-in-out |
| `slideUp` | transition | translateY | (+opacity) | Base (400ms) | ease-out |
| `fadeScale` | transition | opacity | scale | Quick (280ms) | ease-out |
| `wiggle` | effect | translateX | — | Base (400ms) | linear (osc.) |
| `pulse` | effect | scale | — | Quick (280ms) | linear (osc.) |

A nonzero `value:` is "present/in place", zero is "absent/offset" — the
value-tracking contract (a `Bool` is the canonical driver). Effects fire a bounded
oscillation that returns to identity on every `trigger:` change.

**Reduced motion** is honored for free: a reduced-motion theme resolves
`motion_ms` to 0 and the keyframe track jumps straight to the final frame
(`KeyframeTrack::new` clamps to 1ns) — no per-token special-casing.

**Live demo:** `userspace/apps/counter/ui/pages/CounterPage.nx` authors
`.effect(wiggle, trigger: $state.value)` on the value text (wiggles on every ±)
and `.animate(fadeScale, value: $state.value)` on an activity bar (fades+scales in
while the count is non-zero). Host proof: `apps_compile.rs::counter_emits_animation_intents`.

**Current scope limits** (documented, not silent): TEXT nodes fade + horizontally
translate (`wiggle`); vertical translate and scale apply to **filled** nodes (the
fill path). Whole-node scale/exit transitions and window-level transforms are
Track C (Tier 1). Concurrent animations are capped at `MAX_NODE_ANIMS` /
`MAX_ACTIVE_ANIMATIONS` (6).

### Widget animation (kit widgets)

Inherently-animated **kit widgets** (`userspace/ui/widgets/*`) — the `Skeleton`
loading placeholder, `Spinner`, indeterminate `ProgressBar` — build one *resting*
frame for a `phase`; their motion is the animation system's job, not a re-render.

The binding runs the resting frame through the **same paint-time transform loop**
as the modifiers, NOT a per-frame re-emit. This is a hard constraint: the app-host
heap is a **non-freeing bump allocator**, so re-emitting the whole view every frame
(to advance a widget's internal `phase`) would leak the old scene each frame and
OOM. Instead the runtime stamps a continuous `AnimKind::Loop` intent on the widget
node (`is_looping_widget`, `emit.rs`) and the app-host breathes the node via a
spring that **re-fires toward the opposite endpoint on each convergence**
(`AnimState::{sync_loops,tick_loops}`, `AnimationDriver::is_active`). The spring
path carries no `Vec`, so the loop is allocation-free — it runs forever without
growing the heap. The pulse stays armed while the widget is on screen and stops
the instant it leaves the tree.

`Skeleton` fits this directly (its root is a filled rect the per-node fill
transform can fade). A `Spinner`'s spoke rotation and a shimmer's clipped sweep are
NOT expressible as a single filled-rect transform (paths / clip) — they belong on
the **compositor layer transform** (Track C, Tier 1), the render-server-owns-
rotation model. The widgets' `.phase()` builders remain the deterministic CPU
resting-frame API used by goldens.

**Live demo:** `Skeleton { … }` on the counter page breathes continuously on the
frame pulse (leak-free); boot-proven by a multi-frame present burst.

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
