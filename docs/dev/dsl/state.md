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
    match svc.appState.put("counter.value", $state.value, timeoutMs = 250) {
        Ok(_) => dispatch(Saved),
        Err(e) => dispatch(SaveFailed(e.code)),
    }
}
```

## Local component state

Components may declare local `$state` fields (ergonomic sugar): they compile to an
implicit per-instance store with the same reducer machinery — no second semantics.
Local state survives keyed reorders in collections (identity via `.key(expr)`).

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

- **v1 (2026-07-06)** — canonical shape normalized (direct store fields, top-level
  `Event`/`reduce`/`@effect on`, `@persist` on fields); local `$state` defined as
  implicit stores; lint posture consolidated.
