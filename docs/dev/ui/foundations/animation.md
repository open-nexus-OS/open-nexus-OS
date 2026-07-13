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
RFC-0059).

**Tier 1 is implemented too** (Track C): `OP_SET_LAYER_TRANSFORM` (wire op 10)
generalizes the compositor scroll override — windowd animates a retained window
layer's translate / opacity / uniform scale purely on the GPU (record + coalesced
flush in gpud, no re-render, no re-upload). Every window slice carries a
`Layer::layer_id` (slot+1, DISTINCT from `scroll_id` so scrolling never moves the
chrome slices); gpud keys a per-id override table applied in
`composite_pending_rt_layers`. **Override semantics: the encode is always the
untransformed base and the override survives full presents** (no bake, no clear —
a baked opacity multiplied with the override froze windows invisible at 0×x).
windowd's own `AnimationDriver` drives the WINDOW TRANSITIONS on it
(`runtime/transitions.rs`): open = fade+scale-in from 92%; close = fade-out then
the deferred close; minimize = fly to the window's EXACT future dock cell
(`dock::dock_slot_rect`, center-to-center — gpud scale is center-anchored) then
the deferred minimize; restore = the reverse fly-in FROM the clicked dock cell
(WM state change runs UP FRONT, springs settle at identity — no deferred
action); fullscreen enter/leave = the geometry flips instantly and the
transform seeds the OLD frame's apparent rect, springing to identity while the
async band re-create swaps in sharp content (the live-resize clamping covers
the gap). Markers: `windowd: transition open/close/minimize/restore/fullscreen`,
`gpud: layer transform live id=N`.

The compositor also owns the **wait cursor** (loading ring): 8 procedurally
rastered ring frames ride the cursor shape cache (slots 5..12 of 16, uploaded
once at arm + retried from the wait tick), and `cursor_wait_tick` cycles them
with the 2-byte `OP_SELECT_CURSOR_SHAPE` on a ~90 ms grid while a launch is
pending — begun by windowd's own `launch_app` (greeter/shell) or the
`CONTROL_LAUNCH_PENDING` surface-control hint an app-host sends on
`svc.ability.launch`, ended by the fresh window's `SURFACE_CREATE` (desktop or
app), with a 4 s failsafe so a crashing launch never wedges the pointer.
Markers: `windowd: cursor ring on/off`.

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

### Interaction motion (design-handoff feel)

Pointer interactions ride the SAME driver + per-node transform (no extra loop):
**hover** springs the hovered handler box to 1.06 with a bright 2 px ring
(`HoverWash.ring_alpha`), cascading to the contained subtree anchored at the
control's center; **press** dips to 0.92 and pops elastically past identity
(280 ms keyframe). Kinds that animate a PART instead of the whole control carry
a structural `press_offset` on their handler (`registry::press_offset`, like
`child_path`): the **toggle** stretches its THUMB along the travel axis
(`NodeAnim.scale_y_pct` — a non-uniform-scale superset where `None` mirrors the
uniform scale byte-identically, radius follows the SMALLER axis so the knob
stays a capsule) and elastically slides it in from the old end after the flip.

### Widget animation (kit widgets)

Inherently-animated **kit widgets** (`userspace/ui/widgets/*`) — the `Skeleton`
loading placeholder, `Spinner`, indeterminate `ProgressBar` — build one *resting*
frame for a `phase`; their motion is the animation system's job, not a re-render.

The binding runs the resting frame through the **same paint-time transform loop**
as the modifiers, NOT a per-frame re-emit. This is a hard constraint: the app-host
heap is a **non-freeing bump allocator**, so re-emitting the whole view every frame
(to advance a widget's internal `phase`) would leak the old scene each frame and
OOM. Instead the runtime stamps a continuous `AnimKind::Loop` intent on the widget
node whose **value carries the loop sub-kind** (`loop_subkind`, `emit.rs`;
`LOOP_*` consts in `nexus-dsl-runtime::anim`), and the app-host drives the
widget's fixed builder structure at paint time
(`AnimState::{sync_loops,tick_loops}`):

- **`LOOP_SWEEP`** — `Skeleton` shimmer band + indeterminate `ProgressBar` pip:
  the sole child (`root+1`) rides a TranslateX **sawtooth** — an overdamped
  spring glides it across the track (travel = parent − child width, read from
  the live layout each cycle), resets to the left edge on convergence, and
  re-fires. Springs only: a `keyframe_to` per cycle would alloc a waypoint
  `Vec` on the non-freeing heap forever.
- **`LOOP_CAROUSEL`** — `Spinner`: the 12 spoke children (`root+1..=root+12`)
  get a stepped per-spoke opacity fade (leading spoke opaque, tail fading)
  advanced on a ~80 ms time grid — a rotation with **no springs and no
  rebuild**. The DSL registry builds the spinner `.flat()` (all spokes opaque)
  so the paint-time wash is the only fade — a wash over the baked resting fade
  would double-fade the tail.
- **`LOOP_BREATHE`** — the generic whole-node opacity pulse (spring re-fires
  toward the opposite endpoint on each convergence) stays available for
  future resting-frame widgets.

All three are allocation-free per tick, damage only the widget's rows, keep the
frame pulse armed while the widget is on screen (compositor visibility gating
parks hidden windows), and stop the instant the node leaves the tree. The
widgets' `.phase()` builders remain the deterministic CPU resting-frame API
used by goldens.

**Live demo (boot-proven):** the counter page runs `Skeleton` (shimmer sweep),
`ProgressBar` (indeterminate pip) and `Spinner` (spoke carousel) concurrently;
double-snapshot diffs show band/pip travel + spoke rotation on device.

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
