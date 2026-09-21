<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# State, Events, Reducers, Effects

The state model is:

- **Store**: typed, serializable state a feature owns — the only place state lives;
- **Event**: what the UI (or an effect) can dispatch;
- **reduce**: **pure** state transitions (no IO, no time, no randomness);
- **@effect**: runs after commit; owns all IO through `svc.*` adapters; bounded.

The UI only ever sees committed snapshots (`$state.field`); a reducer's intermediate
writes are never observable. This is dataflow instead of shared state — see
`principles.md` for why each rule exists.

## Why reducers/effects (not getters/actions)?

Purity has to be *checkable*. With reducers, "no IO in state transitions" is a
compile-time property; with free-form actions it would be a convention.

## Canonical example

```nx
Store CounterStore {
    value: Int = 0 @persist,
}

Event CounterEvent {
    Inc,
    Dec,
    SaveRequested,
}

reduce CounterEvent {
    Inc => state.value += 1,
    Dec => state.value -= 1,
    SaveRequested => state.value = state.value,  // reducers stay pure; the effect saves
}

@effect on SaveRequested {
    match svc.appState.put("counter.value", $state.value) {
        Ok(_) => dispatch(Saved),
        Err(e) => dispatch(SaveFailed(e.code)),
    }
}
```

## Local component state

Components declare local state in a `state:` block:

```nx
Component Disclosure {
    state: {
        open: Bool = false,
    }
    Stack {
        Toggle { checked: $state.open, label: @t("more") }
        if $state.open { Text(@t("details")) } else { Text(@t("collapsed")) }
    }
}
```

It compiles to an **implicit store** (same machinery, no second semantics);
`$state.field` resolves locally first. Mutations flow through two-way bindings
and handlers — the one mutation path.

### Two-way binding

Bind a control's **primary prop** to `$state` and the control edits that field.
Nothing else is declared — no handler, no payload:

```nx
Toggle { checked: $state.dark, label: @t("dark") }   // a tap flips it
TextField { value: $state.query, label: @t("find") } // typing writes it
Slider { value: $state.brightness }.label(@t("brightness")) // a tap sets the percent
```

Which controls bind, and what the interaction does, is one rule in the widget
catalog (`registry::WidgetSpec::bind`) that the compiler writes into the IR
(`ir.md` v1.8). Today: `Toggle`/`Checkbox` invert the Bool on a tap,
`TextField`/`SearchBar` take the text on a change, and `Slider` takes the
point's position across its own track as a percent `0..=100` — the value a
slider *is*. A control whose primary prop is a LABEL the app supplies
(`Button`, `Chip`, `ListItem`, …) does not bind: there is nothing to write back.
`Select` shows a value but its tap opens an app-owned option panel, so the tap
produces no value either.

A slider's tap SETS the value at the point you touch; there is no drag yet (the
surface delivers taps, not held moves).

**A bind writes the FIELD, not a reducer.** That is the point — no event to
declare — but it means state a reducer used to keep in step with that field is
no longer kept in step. Derive such state from the field instead of storing a
second copy of it: a `muted: Bool` beside a `volume: Int` is one the control
and the status bar can contradict; `volume == 0` cannot.

**Per instance, since TASK-0077B P1.** A stateful component may be instantiated
any number of times, including inside a collection: its store is KEYED, holding
one set of fields per live instance. The identity is the one `ir.md` §"Stable
node identity" defines — `ViewNode.nodeId`, and `keyed_item_id(nodeId, key)` for
a collection item — so a row keeps its own state across a REORDER without the
app moving anything, and a row that leaves the collection takes its fields with
it. (Until then a second instance was a build error, because one store per
COMPONENT would have made two instances share it.)

## Async recipes

Every async flow in this language is the same four states and one store shape.
`busy` and `failed` are separate fields on purpose — an **empty** result is not
a **failure**, and collapsing the two is how an app ends up showing an error for
a folder that is simply empty:

```nx
Store S {
    items: List<Str> = [],
    busy: Bool = false,
    failed: Int = 0,
}

Event E { Load, Loaded(List<Str>), Failed(Int) }

reduce E {
    Load => { state.busy = true; state.failed = 0; },
    Loaded(items) => { state.items = items; state.busy = false; state.failed = 0; },
    Failed(code) => { state.failed = code; state.busy = false; },
}

@effect on Load {
    match svc.catalog.list() {
        Ok(items) => dispatch(Loaded(items)),
        Err(e) => dispatch(Failed(e)),
    }
}
```

The page reads the three apart in order — spinner, failure, then content — so
an empty list renders as "nothing here" rather than as an error:

```nx
if $state.busy {
    Text(@t("common.loading"))
} else if $state.failed == 0 {
    List($state.items) { item in Text(item).key(item) }
} else {
    Text(@t("list.failed"))
}
```

**Both arms are mandatory** (`NX0407`, see `services.md`): an effect plan stops
at the first unhandled failure, so a load without an `Err` arm leaves `busy`
true for ever.

**Retry is re-dispatching the trigger.** `Load` clears `failed` on the way in,
so a retry cannot show a stale error over fresh results. Wire it to a button:

```nx
Button { label: @t("common.retry") }
on Tap -> dispatch(Load)
```

**Automatic retry cannot be written today, and that is deliberate to state
rather than work around.** An effect body lowers to a bounded LINEAR plan, so
`if` inside an effect is `NX0501` — there is no way to consult an attempt
counter before re-dispatching. An unconditional `Err(e) => dispatch(Load)` is
not a retry policy: it runs until `MAX_DISPATCH_CASCADE` (64) trips the dispatch
budget. Automatic bounded retry needs a conditional effect step the v0.1
lowering subset does not have.

**Search-as-you-type needs no extra machinery** — see
[Effect cancellation](#effect-cancellation-latest-wins) below: a re-fired
trigger drops the older plan's pending follow-ups, so stale results never
overwrite newer ones.

*Proven in `tests/dsl_conformance/tests/async_recipes.rs` (the four states,
retry, and the rejection that pins the "no automatic retry" claim) and
`corpus.rs::stale_effect_followups_are_cancelled_when_the_trigger_refires`.*

## Large data: the store bounds the list, not the view

A `List($state.items)` renders whatever the store holds, and the compiler does
not — cannot — bound it: `NX0404` requires a `for` to iterate a literal list,
but whether a store list stays bounded is decided where it is written. Two
tools, and one of them must be in play for any list that grows:

- **`tail(list, n)`** in the reducer that appends — the resident window
  (`chat.store.nx`: `state.messages = tail(state.messages + rows, 64)`);
- a **QuerySpec `limit`** on every page a query effect loads (`db-queries.md`,
  keyset paging), which caps what one append can add.

Paint is visibility-indexed and scroll is a paint-only offset, so the cost of a
frame is the screen, not the list — but the store's memory is the list, and on
the app-host's heap that is the ceiling. `tests/dsl_conformance::corpus::
a_list_over_a_store_list_is_accepted_and_the_bound_is_the_stores_job` pins that
the compiler accepts the unbounded shape, so nobody mistakes the absence of a
lint for a guarantee.

## Effect cancellation (latest wins)

Re-firing a trigger **cancels the previous plan's pending follow-ups**: each
`(event, case)` carries a generation; follow-up dispatches are tagged with the
generation of the trigger that produced them and are dropped if it has
advanced by the time they dequeue. The canonical search-as-you-type recipe
gets this for free — stale results never overwrite newer ones.

## Session vs durable vs queryable

1. **Session state** (default): in-memory store state per app instance.
2. **Durable small**: store fields marked `@persist` — typed snapshots written on
   suspend via the state substrate, restored on mount.
3. **Durable large/queryable**: only through the query service contract
   (see `db-queries.md`) — apps never open storage directly.

"Queryable" does not mean "SQL database": the QuerySpec contract is engine-agnostic
and the platform engine is a deterministic, bounded, pure-Rust store.

## Lint/error posture (v1)

- **Reducer purity (Error)** — no IO, no `svc.*`, no DB/files, no time/RNG.
- **Effects handle failures (Error)** — both `Ok` and `Err` of every `Result`; stable
  error codes, never formatted strings, for user-facing flows.
- **Profile fallback (Warning)** — `if device.profile == …` without a final `else`
  (upgradeable with `--deny-warn`).
- **Bounded loops (Error)** — `for` requires a statically known bound.
- **Keys + a11y (Error)** — collection items need `.key(expr)`; unlabeled interactive
  nodes need `.label(…)`.
- **Exhaustive match (Error)** — over events and enums.

## Changelog

- **2026-09-20 (TASK-0077B P3/P4)** — §Async recipes added: the four-state store shape,
  retry as a re-dispatch, and the LIMIT that automatic retry needs a conditional effect step
  the v0.1 lowering subset does not have. The lint posture below is unchanged — it has said
  *"Effects handle failures (Error)"* since v1; P3 made the compiler match it (`NX0407`).
  §Two-way binding records the bind surface after P2/P2b.
- **v1 (2026-07-06)** — canonical shape normalized (direct store fields, top-level
  `Event`/`reduce`/`@effect on`, `@persist` on fields); local `$state` defined as
  implicit stores; lint posture consolidated.
