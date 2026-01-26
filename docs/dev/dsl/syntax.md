<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Syntax (v0.x)

This page documents the v0.x syntax at a high level. Exact grammar evolves with tasks; the CLI is the source of truth for errors and formatting.

See also: recommended patterns (components/props, builder specs, slots) in `docs/dev/dsl/patterns.md`.

## Conventions

- explicit `import "..."` (no auto-import)
- stable formatting via `nx dsl fmt`

## Conditionals

Canonical form:

```nx
@when device.profile == phone { Stack { /* ... */ } }
@when device.profile == tablet { Stack { /* ... */ } }
@else { Stack { /* default */ } }
```

Notes:

- `@when` is evaluated top-to-bottom; first match wins.
- `match(device.profile) { ... else ... }` may exist as syntactic sugar and must lower to the same IR.

## Loops (bounded)

Loops are allowed, but must be bounded in v0.x (no unbounded `while(true)` patterns).

Example (illustrative):

```nx
for filter in filters {
  // build-only (pure) operations are allowed here
  query = query.where(filter.field, filter.value)
}
```

## Modifiers (styling/layout)

Modifiers are styling/layout annotations applied to a view node.

### Canonical form: `modifier { ... }`

```nx
Button { label: "Continue" }
modifier {
  padding(2)
  bg(accent)
  radius(md)
}
```

### Sugar: chaining

Chaining is syntactic sugar and lowers to the equivalent modifier block:

```nx
Button { label: "Continue" }
  .padding(2)
  .bg(accent)
  .radius(md)
```

Determinism rules:

- duplicate setters should be rejected by lint (preferred), or must follow a documented deterministic rule (e.g. last-wins)
- modifiers must be pure (no IO, no `svc.*`)
- prefer **semantic tokens** for styling (e.g. `bg(accent)`) rather than arbitrary colors; theme authoring is where raw values belong (see `docs/dev/ui/colors.md`)

## Escape hatch: `NativeWidget` (custom widgets)

The DSL is intentionally bounded and token-driven. For rare cases where a design requires behavior that is not yet
expressible with first-party primitives, an optional escape hatch may be used.

Concept:

- `NativeWidget(handle: NativeWidgetHandle, props: NativeProps)` is a view node implemented in Rust.
- The handle is **capability-gated/registered** by the platform or app package (no dynamic code loading in v0.x).

Constraints (must remain true in both interpreter and AOT):

- deterministic rendering given the same inputs (no wallclock / RNG unless injected and declared)
- bounded resource usage (memory, CPU per frame)
- no direct filesystem/DB/network access from the widget (use `svc.*` via effects for IO)
- a11y contract: provide semantics/roles/labels or the node is lint-rejected where required

Example (illustrative):

```nx
Page FancyChartPage {
  NativeWidget(handle: "com.acme.widgets.ChartV1", props: { seriesId: $state.seriesId })
}
```

## Example (illustrative)

```nx
import "@app/ui/components/Card"

Component CardRow {
  view: Card {
    Text { value: "Hello" }
  }
}
```
