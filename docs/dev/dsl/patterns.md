<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Patterns (v0.x): “Generics-like” Ergonomics

We intentionally avoid “full generics” in v0.x.
Instead, we recommend three patterns that are familiar to frontend developers while keeping the DSL deterministic and bounded.

This page is about **composition** — how to reuse a piece of UI. The async
flow patterns (loading / loaded / failed / empty, retry, latest-wins
cancellation) live with the state model they are made of, in
[`state.md` §Async recipes](state.md#async-recipes).

## Pattern 1: Components + Props (the default)

### Use when

- you want reusable UI building blocks
- you want one definition used from many places, with a typed props contract
- you need a stable API surface for design-system primitives and app components

### How to use

- keep props small and focused
- prefer **semantic tokens** over raw values (colors, sizes)
- for complex configuration, pass a small “spec” object (bounded) rather than many booleans

Example — `EventRef` hands the caller's event to the component, which emits it:

```nx
Event ButtonEvent { Pressed, }

Store S { n: Int = 0, }

reduce ButtonEvent { Pressed => state.n += 1, }

Page ToolbarPage {
    Stack {
        IconButton { icon: "plus", label: @t("add"), onTap: ButtonEvent::Pressed }
    }
}

Component IconButton {
    props: {
        icon: Str,
        label: Str,
        onTap: EventRef,
    }
    Stack {
        Icon { symbol: $props.icon, size: 20 }
        Text($props.label).textStyle(body)
    }
    .direction(row)
    .gap(gapInline)
    .padding(8)
    .bg(fill)
    .rounded(full)
    .label($props.label)
    on Tap -> emit($props.onTap)
}
```

### Avoid when

- you are trying to build a “mega component” with dozens of props and hidden modes
- the “props API” would be unstable or hard to document

### Risks if misused

- **API drift**: many optional props become effectively an untyped configuration language
- **inconsistent UX**: components diverge from the design system when props allow too much freedom

Mitigation:

- lint for overly large props blocks (soft limit)
- prefer composition (small components) over large mode switches

## Pattern 2: Builder Specs (typed outside, referenced inside)

This pattern is ideal for queries, filters, layout specs, and other structured “plans”:

- DSL builds a **spec** using pure operations
- effects/services **execute** the spec (IO)

For QuerySpec specifically, treat `docs/dev/dsl/db-queries.md` as the concrete contract and this section as the more
general DSL pattern.

### Use when

- you need a structured, typed object (query spec, filter spec, paging spec)
- you want strong validation without embedding a full type system into the DSL

### How to use

- building is **pure** (OK in reducers/pure helpers)
- execution is **IO** (effects/services only, bounded + time-limited)
- avoid stringly-typed fields; prefer enums/handles

Example (pseudo-code — a query builder is planned, not implemented; today a query is the
declarative `Query` form in `db-queries.md`):

```text
// build (pure)
let q = Query.users()
  .whereId($state.userId)
  .select([UserField::Name, UserField::Role])
  .limit(1)

// execute (IO) via effect/service
@effect on LoadUser { /* runs the query */ }
```

### Avoid when

- a simple prop/field would do (don’t build builders for everything)
- execution cannot be bounded (unbounded scans, unbounded joins)

### Risks if misused

- **layer confusion**: developers execute builders in reducers (violates purity)
- **debug complexity**: “where is this executed?” becomes unclear

Mitigation:

- lint: “builder execution” is only allowed in effects/services
- stable error codes (no stringly errors)

## Pattern 3: Parametric Primitives + Slots (use sparingly)

Instead of `List<T>`-style generics, a component takes **slots**: named content regions the
caller fills (RFC-0084). A slot body is lexically part of the caller.

### Use when

- you need highly reusable UI primitives whose content the caller supplies (a group, a form section, a window scaffold)
- you want a consistent performance contract (virtualization, paging) baked into the primitive

### How to use

- keep slot bodies small; move complex UI into components
- enforce determinism/purity rules inside slots (no IO)
- require stable keys for list-like slots

Example — the component declares `slot rows` and places it with `Slot rows`; the caller
fills it, and the collection inside is the keyed `List`:

```nx
Store NoticeStore {
    notifications: List<Str> = [],
}

Event NoticeEvent { Clear, }

reduce NoticeEvent { Clear => state.notifications = [], }

Component Group {
    props: {
        title: Str,
    }
    slot rows
    Stack {
        Text($props.title).textStyle(caption).fg(labelSecondary)
        Stack {
            Slot rows
        }
        .rounded(lg)
        .material(card)
    }
    .gap(8)
}

Page NoticesPage {
    Group { title: @t("notices.title") } {
        rows {
            List($state.notifications) { item in
                Text(item).key(item)
            }
        }
    }
}
```

### Avoid when

- the slot becomes a “mini program” with lots of branching and stateful logic
- you need IO, timers, or service calls inside the slot (that belongs in effects/services)

### Risks if misused

- **unbounded work**: heavy slot logic in large lists kills perf
- **hard-to-review behavior**: business logic hides in UI rendering callbacks

Mitigation:

- lint: cap slot complexity (heuristic) and enforce virtualization for large lists
- push business logic into stores/effects, keep slots presentational

## Why not full generics (v0.x)

Full generics typically imply:

- a type checker / inference rules
- monomorphization or runtime generics
- a larger surface area for “magic” behavior

That conflicts with v0.x goals: **determinism**, **boundedness**, and **front-end friendly ergonomics**.

## System surfaces: shell + greeter as DSL consumers (TASK-0080B)

The system's own chrome is written in the same DSL as apps —
`userspace/apps/desktop-shell/` (ShellPage + LauncherPage) and
`userspace/apps/greeter/` (GreeterPage) are ordinary project trees
compiled by the ordinary project mode. Two rules make a system surface:

1. **Authority stays in services.** The launcher ASKS `svc.ability.launch`
   (abilitymgr decides, fail-closed); the greeter ASKS `svc.session.login`
   (sessiond decides, TASK-0065B contract, `docs/dev/ui/shell/session.md`).
   A system page never contains auth/launch logic — it renders state and
   dispatches. If a flow needs a decision, that decision is a service method
   on the generated surface, not an `if` in the page.
2. **Data comes from the real registries.** The launcher's app list is
   `svc.bundlemgr.enumerate(query)` — filtering happens AT the service (the
   canonical search-as-you-type recipe: every keystroke re-dispatches; stale
   responses are dropped by per-(event,case) generations). No hardcoded app
   lists anywhere in the shell.

Host proofs live in `tests/systemui_bootstrap_shell_host/`: profile matrix
(desktop/phone/tablet + the phone launcher override), transcripted launch and
login flows (byte-exact replay — a miss can never look like success), and the
lint gate both trees must pass.
