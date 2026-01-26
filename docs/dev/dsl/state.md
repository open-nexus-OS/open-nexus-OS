<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# State, Events, Reducers, Effects

The DSL model is:

- **State**: serializable, deterministic data
- **Events**: what UI dispatches
- **Reducers**: **pure** state transitions (no IO)
- **Effects**: run after commit; may perform IO through service adapters; must be bounded

## Why not “getters/actions”?

We use *reducers/effects* to match the semantics and enforce purity deterministically.

## Example (illustrative)

```nx
Store CounterStore {
  State { value: Int }
  Event { Inc, Dec, SaveRequested }

  reduce(state, event) -> state {
    match event {
      Inc => state.value += 1
      Dec => state.value -= 1
      SaveRequested => state // reducers stay pure
    }
  }

  effect(event) {
    match event {
      SaveRequested => {
        // v0.2b: call service adapters from effects only
        // svc.settings.set("counter.value", state.value) (pseudo)
      }
    }
  }
}
```

## Session vs durable vs DB

Recommended tiering:

1. **Session state** (default): in-memory store state per app instance/window.
2. **Durable small**: typed snapshots (`.nxs`) via settings/app-state substrate.
3. **Durable large/queryable**: DB only when required (indexing/query/history).

## Lint rules summary (v0.x)

This is the “default posture” the DSL aims to enforce. Exact severities may evolve per task, but the intent is stable.

- **Reducer purity (Error)**:
  - reducers must be pure: no IO, no `svc.*`, no DB/files, no time/RNG dependence.
- **Effects handle failures (Error)**:
  - effects must not ignore `Result<T, E>`; handle both success and failure deterministically.
  - prefer stable error codes/enums over `to_string()` for user-facing flows.
- **Profile branching fallback (Warning)**:
  - for profile-driven layout branching with `@when`, missing `@else` is a warning (upgradeable with `--deny-warn`).
- **Bounded loops (Error)**:
  - loops are allowed, but must be bounded in v0.x (no unbounded/infinite loops).
- **A11y and list keys (Error)**:
  - list items require stable keys (`@key`)
  - missing a11y label hints are rejected where required by the lane’s lint posture.
