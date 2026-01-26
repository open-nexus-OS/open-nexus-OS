<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DB Queries (DSL)

This doc describes optional DSL-level query objects and paging tokens.

See also: builder-spec pattern guidance in `docs/dev/dsl/patterns.md`.

Principles:

- determinism (stable ordering/tie-breaks)
- boundedness (caps on results, paging)
- no DB required for core DSL semantics (host-first; OS-gated)

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
