<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DB Queries (DSL)

This doc is the reference for DSL-level query objects (QuerySpec), the paging
contract, and the platform query engine (`source/libs/nexus-query`).

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
- and keep any alternative backend behind the same service/query contract.

For example, browser history/bookmarks need deterministic filter/order/search behavior, but they do not require the DSL or
UI to depend directly on a relational engine.

---

# The platform engine: `nexus-query` (v1 reference)

## Engine decision (2026-07-06)

The platform query engine is **pure Rust** (`source/libs/nexus-query`,
`no_std`+alloc, zero dependencies). A C SQL engine was rejected: it is
unworkable inside `no_std` riscv64 services, and a cost-based query planner
breaks the determinism charter (same query, same data ⇒ same plan, same order,
always). The v1 shape needs no planner at all: index selection is syntactic.

The engine is swappable behind the service contract — nothing observable
(error codes, ordering, tokens) identifies it.

Layering (one concern per module):

| Module | Concern |
| --- | --- |
| `encoding` | order-preserving key codec + deterministic row codec |
| `kv` | ordered-storage seam (`Kv` trait); host backend `MemKv` |
| `spec` | the QuerySpec value, canonical bytes/hash, opaque page tokens |
| `engine` | table schemas, index-maintained writes, query execution |

On the OS the engine runs inside `queryd` over statefsd's journaled KV
(atomicity from the journal); on the host it runs over `MemKv`. It is the
**identical engine**, so host proofs transfer structurally.

## Key encoding (the correctness core)

Keys are compared **byte-wise** by the KV layer, so the codec must make byte
order equal value order (property-tested exhaustively, including sign, prefix
and NUL cases):

- `Int`/`Fx` (i64): sign-bit flip, then big-endian — negative values sort
  before positive, magnitude order preserved.
- `Bool`: one byte `0`/`1`.
- `Str`: UTF-8 with `0x00` escaped as `0x00 0xFF`, terminated by `0x00 0x00`.
  Self-terminating ⇒ tuple concatenation preserves lexicographic tuple order
  (a string that is a prefix of another sorts first).

Row payloads use a separate tagged, length-prefixed codec (`encode_row`) that
is deterministic but not order-preserving — keys carry the order, rows carry
the data. Decoding is bounded and rejects truncation and trailing bytes.

## Storage layout

```
primary row:  q <table u16 BE> r <pk_key>                      → row bytes
index entry:  q <table u16 BE> i <col u16 BE> <col_key> <pk_key> → pk_key
```

The index **value** repeats the `pk_key`, so resuming a scan or fetching the
row never parses the composite index key apart. `put` with an existing primary
key replaces the row and removes stale index entries first; `delete` removes
row + index entries. Index maintenance is exercised by property-style tests
(entry counts and reachability after replace/delete).

## QuerySpec v1 shape

A QuerySpec is an immutable value:

- `table` — table id (typed handles at the DSL surface),
- `eq` — conjunction of equality predicates `(column, value)`,
- `range` — at most **one** range, and it must be on the **order column**,
- `order_col` + `descending` — exactly one order column,
- `limit` — mandatory (> 0), caps every page.

The v1 execution rule that keeps everything deterministic and bounded: the
order column must be indexed (primary key or secondary index), and the range
rides that same index. The scan therefore streams rows already in final order
— **no post-sort, ever**. Equality predicates are filtered on decoded rows
during the scan. Total keys touched per execution are hard-capped.

Ordering ties on the order column break by primary key (ascending pk within
equal order values; fully reversed under `descending`). This tie-break is part
of the contract, not an implementation detail.

Stable error vocabulary (wire-mappable, no stringly failures):
`UnknownTable`, `UnknownColumn`, `TypeMismatch`, `Unsupported` (shape outside
v1), `BadToken`, `Corrupt`.

## Canonical form and hash

Two logically identical specs must produce identical bytes and hash across
runs and platforms (the identity for page tokens, caches, and later
subscriptions). Canonicalization: equality predicates sorted by column index;
fixed field order (table, eq, range, order, limit); values via the
deterministic row codec. The hash is FNV-1a 64 over the canonical bytes —
**identity, not integrity**. The hash value is pinned by a golden test;
changing canonical bytes is a documented, versioned event (like `.nxir`
schema bumps in `ir.md`).

## Keyset paging contract

No offset paging anywhere. A page token is opaque to apps and contains
`(query hash LE u64) ∥ (last emitted scan key)`:

- resuming ascending scans starts strictly after the last key
  (`last_key ∥ 0x00`); descending scans end exclusively at it;
- a token minted by a **different** query (hash mismatch) is rejected with
  `BadToken`; malformed tokens fail to parse;
- **no duplicates, no gaps**: a full token walk equals the one-shot result
  (integration-proven for ascending/descending, filtered, ranged, and
  pk-ordered queries at many page sizes);
- under interleaved writes the keyset semantics hold: rows inserted behind
  the cursor do not resurface, rows ahead of it are picked up (an offset
  cursor would duplicate or skip here — that is why offsets are banned).

`Page { rows, next }`: `next = None` means exhausted; a present token means
the caller may continue. Tokens survive wire round-trips byte-identically.

## v2/v3 non-goals (v1)

`OR`, joins, aggregation, full-text, arbitrary query strings, multi-column
order, ranges off the order column — all out of scope for v1 and rejected
with `Unsupported` rather than half-supported. The staged roadmap above
(builder ergonomics v2, lazy providers v3) grows on top of this floor without
touching canonicalization or paging rules.
