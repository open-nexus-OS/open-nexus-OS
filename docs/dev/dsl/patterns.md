<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Patterns (v0.x): “Generics-like” Ergonomics

We intentionally avoid “full generics” in v0.x.
Instead, we recommend three patterns that are familiar to frontend developers while keeping the DSL deterministic and bounded.

## Pattern 1: Components + Props (the default)

### Use when

- you want reusable UI building blocks
- you want a React/Vue-like mental model
- you need a stable API surface for design-system primitives and app components

### How to use

- keep props small and focused
- prefer **semantic tokens** over raw values (colors, sizes)
- for complex configuration, pass a small “spec” object (bounded) rather than many booleans

Example (illustrative):

```nx
Component IconButton {
  props: {
    icon: IconId
    label: Text
    kind: ButtonKind  // Primary | Secondary | Ghost
    onTap: EventRef
  }

  view: Button {
    label: $props.label
    kind: $props.kind
    leadingIcon: Icon { name: $props.icon, size: 20 }
    on Tap -> emit($props.onTap)
  }
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

### Use when

- you need a structured, typed object (query spec, filter spec, paging spec)
- you want strong validation without embedding a full type system into the DSL

### How to use

- building is **pure** (OK in reducers/pure helpers)
- execution is **IO** (effects/services only, bounded + time-limited)
- avoid stringly-typed fields; prefer enums/handles

Example (illustrative):

```nx
// build (pure)
let q = Query.users()
  .whereId($state.userId)
  .select([UserField::Name, UserField::Role])
  .limit(1)

// execute (IO) via effect/service
on Mount -> emit(ProfileEvent::LoadUser(q))
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

Instead of `List<T>`-style generics, we parameterize behavior via **slots** (render callbacks).

### Use when

- you need highly reusable UI primitives with custom rendering (List/Table/Menu/Form)
- you want a consistent performance contract (virtualization, paging) baked into the primitive

### How to use

- keep slot bodies small; move complex UI into components
- enforce determinism/purity rules inside slots (no IO)
- require stable keys for list-like slots

Example (illustrative):

```nx
List {
  items: $state.notifications
  @key: item.id

  item(item) => Row {
    Icon { name: item.icon, size: 20 }
    Text { value: item.title }
  }

  empty => Text { value: "No notifications" }
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
