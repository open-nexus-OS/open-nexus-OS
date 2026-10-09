<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Syntax Tour

A guided tour of the `.nx` surface. The normative grammar is `grammar.md`; the compiler
is the executable source of truth for errors and formatting.

## Conventions

- explicit `import "..."` — no auto-import, resolution is reproducible;
- one canonical layout, produced by `nx dsl fmt` (`parse → fmt → parse` is idempotent);
- files: pages/components under `ui/pages|components/**.nx`, stores under
  `ui/composables/**.store.nx` (see `project-layout.md`).

## Stores, events, reducers, effects

State fields are declared directly in the store; events, reducers, and effects are
top-level declarations — the file reads top-to-bottom as *what we keep → what can
happen → how state changes → what side effects run*:

```nx
Store UserListStore {
    users: List<User> = [],
    loading: Bool = false,
    error: Int = 0,
}

Event UserListEvent {
    LoadUsers,
    UsersLoaded(List<User>),
    LoadFailed(Int),
}

reduce UserListEvent {
    LoadUsers => state.loading = true,
    UsersLoaded(users) => {
        state.users = users;
        state.loading = false;
    },
    LoadFailed(code) => {
        state.error = code;
        state.loading = false;
    },
}

@effect on LoadUsers {
    match svc.users.list() {
        Ok(users) => dispatch(UsersLoaded(users)),
        Err(e) => dispatch(LoadFailed(e)),
    }
}
```

Reducers are pure (no IO, no `svc.*`, no time/randomness — compile error otherwise).
Effects run after commit, own all IO, and must handle both `Ok` and `Err` of every
service call — **`NX0407`, an error, not advice**: an effect plan stops at the first
unhandled failure, so a call with no `Err` arm leaves `loading` set and the page on
its spinner with nothing to tell the user. A service call therefore appears in exactly
one place: as the scrutinee of a `match` with both arms.

### Initial load: root effects (no lifecycle hook)

There is **no** `on Mount`/`useEffect` in the language — a second, imperative
effect-trigger model is exactly what `principles.md` §5 forbids. The initial
load falls out of the **dataflow** instead:

```nx
Event UserListEvent { LoadUsers, UsersLoaded(List<User>), LoadFailed(Int) }

@effect on LoadUsers {
    match svc.users.list() {
        Ok(users) => dispatch(UsersLoaded(users)),
        Err(e)    => dispatch(LoadFailed(e)),
    }
}
```

`LoadUsers` carries an `@effect` but is dispatched by **nothing** — no handler,
no reducer, no other effect. It is a **root**: it can only ever run at mount,
so the runtime runs it once, at mount. Writing the obvious program just loads;
there is no lifecycle code to write and none to get wrong. An event that a
handler *does* dispatch (e.g. `Submit` from a button's `on Tap`) is not a root
and never auto-fires.

The runtime derives the roots statically from the IR at mount and runs them
via `View::run_initial_effects`; the host calls it once right after mount.

## Pages and components

A `Page` body **is** its view. Components declare `props` first:

```nx
Page UserListPage {
    Stack {
        if $state.loading {
            Text(@t("common.loading"))
        } else {
            List($state.users) { user in
                UserRow { user: user, onOpen: UserListEvent::Open }.key(user.id)
            }
        }
    }
    .padding(4)
    .gap(2)
}

Component UserRow {
    props: {
        user: User,
        onOpen: EventRef,
    }
    Stack {
        Avatar { initials: $props.user.initials }
        Text($props.user.name).textSize(base)
    }
    .direction(row)
    .gap(3)
    on Tap -> emit($props.onOpen)
}
```

Single-primary-prop widgets accept positional sugar: `Text("Hi")` ≡
`Text { value: "Hi" }`.

### Container primitives: `Panel` and `Circle`

Two named containers for the shapes every surface is built from — both host
arbitrary children (icons, text, stacks) and take every modifier:

```nx
Panel {                       // panel-glass surface: material(panel) +
    Text(@t("net.title"))     // rounded(lg) + padding(3) pre-applied;
    Slider { value: $state.v }// explicit .material/.rounded/.padding win
}
.width(328)

Circle { size: 22             // perfectly round: size pins a square box,
    Icon { symbol: "play", size: 12 } // radius is welded to full, content
}                             // centers on both axes
.material(subtle)
```

`Panel` is the building block for Control-Center tiles, window content
panels and the properties sidebar; `Circle` for round buttons, badges and
avatar-like elements. Both are sugar over `Stack` — same layout semantics,
no new paint machinery.

## Conditionals

Plain `if/else`, including on the device environment:

```nx
if device.profile == desktop {
    SplitView { /* sidebar + content */ }
} else {
    Stack { /* single column */ }
}
```

`if` on `device.profile` without a final `else` is a warning (a device you didn't
think of gets the default branch, not a blank screen). `match` is available and must
be exhaustive.

## Loops and collections

- `for x in xs { … }` — bounded iteration for building static structure (the bound
  must be statically known);
- `List($state.items) { item in … }` — the keyed, virtualizable collection template.
  Every item needs a stable `.key(expr)`.

## Modifiers

Chained utility calls with token arguments (full catalog: `modifiers.md`):

```nx
Button { label: @t("cta") }
  .padding(4)
  .bg(accent)
  .fg(onAccent)
  .textSize(sm)
  .rounded(md)
  .shadow(sm)
```

- duplicate modifiers on one node = error;
- modifiers are pure;
- arguments are semantic tokens — raw hex/px values are not expressible in app code
  (they belong to theme authoring, see `docs/dev/ui/foundations/visual/colors.md`);
- token arguments of closed vocabularies are **checked at compile time**:
  `.fg(oNSurface)` is an error with a "did you mean" hint, not a silent no-op.

### Motion

Semantic motion tokens with explicit categories — no free-form animation language:

```nx
Button { label: @t("cta") }
  .animate(snappy, value: $state.enabled)
  .transition(fadeScale)
  .effect(wiggle, trigger: $state.nudgeTick)
```

- `.animate(token, value:)` — animate state-driven property changes;
- `.transition(token)` — insert/remove/open/close lifecycle motion;
- `.effect(token, trigger:)` — bounded attention effect when the trigger changes.

### Layering: `.overlay()` goes ON TOP

`.overlay()` lifts a container OUT of flow and paints it **last**, over its
parent's content — drop-downs, dialogs, sheets. It is not a way to put
something *behind* the UI: a full-bleed `.overlay()` declared first still ends
up in front, and (because later ids win hit-testing) it swallows every tap.
A backdrop is a **background** on a container that wraps the content:

```nx
Stack {                       // content, tinted
    /* zones */
}
.grow(1)
.bg(wallpaperTint)
```

### Modal and transient layers (`on Dismiss`, ADR-0068)

```nx
if $state.confirm {
    Stack {
        Stack { Button { label: "Cancel" } on Tap -> dispatch(Cancel) }.width(320)
            on Tap -> dispatch(Noop)        // the panel absorbs its own taps
    }
    .overlay(modal)                        // bounded stack (max 4), focus trap, confinement
    .bg(scrim)
    on Dismiss -> dispatch(Cancel)         // ESC and the backdrop fire THIS — the one way out
} else { Stack { } }

if $state.toast != "" {
    Stack { Text($state.toast) }
    .overlay(transient)                    // confines nothing
    .dismissAfter(4000)                    // the host's timer fires `on Dismiss`
    on Dismiss -> dispatch(HideToast)
} else { Stack { } }
```

The runtime never hides a layer by itself: `on Dismiss` dispatches, the reducer changes the
state, the layer leaves with the next emit. A kinded overlay without `on Dismiss` is NX0413.
The shared library (`window-kit`) ships the design handoff's elements on this contract —
`WinAlert`, `WinModal` (slots `body`/`footer`), `WinToast` — with a declared store contract.

### Host triggers

Most triggers come from a press on the node (`Tap`, `LongPress`, `Change`, …). These are fired
BY NAME by the host on whichever node declares them — the page reacts to something that
happened outside its own input:

| Trigger | Fired when |
|---|---|
| `WindowsChanged` | the compositor's window set moved (opened, closed, minimized, restored, focused) |
| `Dismiss` | ESC, a backdrop press or a transient's timeout closes the topmost layer (above) |
| `ClipboardChanged` | this app copied or cut text from one of its fields into the clipboard |
| `CaptureOpen` · `CaptureScreen` · `CaptureWindow` | Print · Shift+Print · Alt+Print (the desktop surface only — RFC-0095) |

```nx
on ClipboardChanged -> dispatch(ClipsReload)   // the open history re-reads at once
```

### The drag gesture (`DragStart` · `DragMove` · `DragEnd`, RFC-0095)

A press is a `Tap`. Held and moved past 3 px it becomes a drag: `DragStart` fires on the node
under the PRESS that declares it (hit-tested like a tap; a panel absorbs with its own
`on DragStart -> dispatch(Noop)`), and `DragMove` / `DragEnd` (the release) then reach THAT
node wherever the pointer goes. The reducer reads the pointer and the press at dispatch time:
`device.dragX`, `device.dragY`, `device.dragStartX`, `device.dragStartY` (surface pixels, Int).

`DragMove` fires at most once per presented frame, with the NEWEST position — positions that
arrive while a frame is on its way are superseded, never dispatched; the start is also the
first move, and the release moves to where the pointer let go before `DragEnd`. So a reducer
must compute from the current pointer (`device.dragX` minus an anchor), never accumulate
deltas. The repaint is the rows the change touched (a box that only moved or resized
damages what its new geometry changes), not the frame.

```nx
Stack { … }
on DragStart -> dispatch(SelBegin)     // reduce: state.anchorX = device.dragStartX; …
on DragMove -> dispatch(SelMove)       // reduce: state.w = device.dragX - state.anchorX; …
on DragEnd -> dispatch(SelEnd)
```


Reduced-motion behavior is part of each token's contract. There are no CSS-style
keyframes, no free-form animation variables, no magic one-off utilities.

These are **implemented** (TASK-0062/0075): the app-host `AnimationDriver` ticks
the intent on the compositor frame pulse and the painter applies the per-node
transform (the app stays out of the per-frame loop). See
`docs/dev/ui/foundations/animation.md`. Live demo:
`userspace/apps/counter/ui/pages/CounterPage.nx`.

## Escape hatch: `NativeWidget`

For surfaces the first-party catalog cannot express (document canvas, waveform,
video preview), a capability-gated native view node exists:

```nx
Page FancyChartPage {
    NativeWidget(handle: "org.example.widgets.ChartV1", props: { seriesId: $state.seriesId })
}
```

Constraints: deterministic rendering for the same inputs,
bounded resources, no direct IO (services via effects only), a11y contract required.
No dynamic code loading.

## Changelog

- **v1 (2026-07-06)** — canonical surface normalized (see `grammar.md#changelog`):
  direct store fields, top-level `Event`/`reduce`/`@effect on`, `if/else` replaces
  `@when/@else`, `Page` body is the view, chained modifiers are the single modifier
  form (the `modifier { }` block is removed), positional sugar, `.key()` required on
  collection items.
