<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DB Queries (DSL)

This doc describes optional DSL-level query objects and paging tokens.

See also: builder-spec pattern guidance in `docs/dev/dsl/patterns.md`.

Principles:

- determinism (stable ordering/tie-breaks)
- boundedness (caps on results, paging)
- no DB required for core DSL semantics (host-first; OS-gated)

## QuerySpec roadmap

The QuerySpec story is intentionally staged:

- **v1 foundation**:
  - first-class QuerySpec value,
  - minimal syntax,
  - service-gated execution,
  - canonical form/hash floor,
  - and opaque page-token transport.
- **v2 hardening**:
  - richer builder ergonomics,
  - stronger defaults,
  - stricter lint/diagnostic posture,
  - and tighter deterministic paging/default-order rules.
- **v3 lazy data surfaces**:
  - QuerySpec consumed through paged providers,
  - virtual-list integration,
  - placeholder semantics,
  - and bounded in-flight lazy loading.

This split keeps early consumers usable without waiting for every later hardening detail.

## Query building vs query execution

We distinguish:

- **building** a query: pure operations on a query object (OK in reducers/pure helpers)
- **executing** a query: IO (must be done in effects/services; bounded and time-limited)

## Typed fields (avoid stringly-typed queries)

`where(field, value)` must not accept arbitrary field-name strings. Use one of:

- enum fields (e.g. `UserField::Role`)
- generated typed handles (from schema/IDL)

This prevents injection-by-field-name and keeps validation deterministic.

## Bounded filters and loops

Loops are allowed but must be bounded:

- cap filter list length (lint/config)
- cap max result size
- require explicit ordering

Example (illustrative):

```nx
for f in filters { // bounded (e.g. max 16)
  query = query.where(f.field, f.value)
}
query = query.orderBy(UserField::Name, Asc)
```

## Effects-first execution

Execution belongs in an effect:

```nx
@effect on LoadRequested {
  match svc.db.users.query(query, limit=50, timeoutMs=250) {
    Ok(rows) => dispatch(Loaded(rows)),
    Err(e) => dispatch(LoadFailed(e.code)), // stable code, not to_string()
  }
}
```

Recommended posture:

- build QuerySpec in store/composable/reducer-adjacent pure code,
- store it in state if that makes the UI easier to reason about,
- execute it only from an effect or effect-side service adapter,
- and treat responses as bounded snapshots rather than ambient live views.

## Typical shape in UI code

Illustrative flow:

```nx
reduce(state, event) -> state {
  match event {
    SearchChanged(text) => {
      state.query = PickerQueries.forText(text)
      state
    }
    NextPageRequested(token) => {
      state.query = state.query.page(token)
      state
    }
  }
}

effect(event) {
  match event {
    SearchChanged(_) | NextPageRequested(_) => {
      let res = PickerService.query(state.query, timeoutMs=250)
      emit(PageLoaded(res))
    }
  }
}
```

The important part is not the exact syntax, but the shape:

- query changes are pure state changes,
- execution is lazy/effect-driven,
- and paging stays explicit in state and tests.

## Where QuerySpec fits best

Use QuerySpec where the UI needs a **structured data view** with deterministic filtering, ordering, and paging:

- content/provider-backed lists and pickers,
- files/document surfaces,
- offline/cache-backed feed or timeline views,
- map result/bookmark/history lists,
- and connector-backed tables/dashboards.

These are the strongest cases because the UI can build the query as a pure value, keep it stable across state changes,
and hand execution to a service-gated adapter.

Typical fit:

- build query in store/composable code,
- execute via `contentd.query(...)` or a domain service query RPC in an effect,
- feed the resulting rows into virtualized list/grid/table surfaces,
- and keep paging/order explicit so host tests remain deterministic.

Good early consumers:

- document picker provider lists/search,
- Files folder/search/filter views,
- browser history and later bookmark/favorite lists,
- feed/timeline cache views,
- and connector-backed office tables/dashboards.

These surfaces may add domain-specific helper builders or presets, but they should not fork canonicalization or execution
rules.

## Where QuerySpec is usually not the primary API

Do not force QuerySpec onto surfaces that are mainly command- or workflow-oriented:

- launcher actions and app launch flows,
- settings mutation flows,
- compose/send actions in chat or social apps,
- search palette execution routing,
- and live protocol/session control paths.

Those flows should keep domain-specific service APIs. QuerySpec may still sit behind the scenes for local indexes or cache
views, but the UI contract should remain the service/domain action rather than a generic query surface.

## Storage posture

QuerySpec is a **query contract**, not a mandate for one storage engine.

Recommended posture:

- use typed snapshots/logs by default where they are enough,
- add a queryable storage abstraction when history/indexing/filtering really require it,
- and keep optional SQL/libSQL-style backends behind the same service/query contract.

For example, browser history/bookmarks need deterministic filter/order/search behavior, but they do not require the DSL or
UI to depend directly on a relational engine.
