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
}

Event UserListEvent {
    LoadUsers,
    UsersLoaded(List<User>),
}

reduce UserListEvent {
    LoadUsers => state.loading = true,
    UsersLoaded(users) => {
        state.users = users;
        state.loading = false;
    },
}

@effect on LoadUsers {
    let users = svc.users.list();
    dispatch(UsersLoaded(users));
}
```

Reducers are pure (no IO, no `svc.*`, no time/randomness — compile error otherwise).
Effects run after commit, own all IO, and must handle both `Ok` and `Err` of every
service call.

### Initial load: root effects (no lifecycle hook)

There is **no** `on Mount`/`useEffect` in the language — a second, imperative
effect-trigger model is exactly what `principles.md` §5 forbids. The initial
load falls out of the **dataflow** instead:

```nx
Event UserListEvent { LoadUsers, UsersLoaded(List<User>) }

@effect on LoadUsers {
    match svc.users.list(timeoutMs: 250) {
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
  (they belong to theme authoring, see `docs/dev/ui/foundations/visual/colors.md`).

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

Reduced-motion behavior is part of each token's contract. There are no CSS-style
keyframes, no free-form animation variables, no magic one-off utilities.

## Escape hatch: `NativeWidget`

For surfaces the first-party catalog cannot express (document canvas, waveform,
video preview), a capability-gated native view node exists:

```nx
Page FancyChartPage {
    NativeWidget(handle: "org.example.widgets.ChartV1", props: { seriesId: $state.seriesId })
}
```

Constraints (interpreter and AOT alike): deterministic rendering for the same inputs,
bounded resources, no direct IO (services via effects only), a11y contract required.
No dynamic code loading.

## Changelog

- **v1 (2026-07-06)** — canonical surface normalized (see `grammar.md#changelog`):
  direct store fields, top-level `Event`/`reduce`/`@effect on`, `if/else` replaces
  `@when/@else`, `Page` body is the view, chained modifiers are the single modifier
  form (the `modifier { }` block is removed), positional sugar, `.key()` required on
  collection items.
