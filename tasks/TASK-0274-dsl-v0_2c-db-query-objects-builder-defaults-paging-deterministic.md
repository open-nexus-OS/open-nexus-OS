---
title: TASK-0274 DSL v0.2c (host-first): db query objects (builder) + safe defaults + paging tokens + deterministic tests
status: Draft
owner: @ui
created: 2025-12-30
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v0.2a core (stores/effects/routes/i18n): tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - DSL v0.2b stubs + demo: tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
  - Content providers + query: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Virtualized list (lazy UI): tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
---

## Context

We want ergonomic, Nuxt-like “data objects” in UI code (composables/effects), but without turning the UI into a
database authority or creating injection risks.

This task introduces **db query objects** as a **typed, structured QuerySpec** created in DSL code via a builder:
`db.users.where_id(id)` style. Execution remains **service-gated** (typed stubs), and results remain bounded and
deterministic.

## Goal

Deliver host-first support for:

1. **QuerySpec IR node**:
   - `IrQuerySpec` (table, select, predicates, order, paging, limits, schema version)
   - stable canonicalization and hashing
2. **DSL surface (builder style; no stringly SQL)**:
   - `db.<table>` root
   - builder operations (v0.2c minimal):
     - `select(cols...)`
     - `where_<field>(value)` for equality predicates only
     - `orderby_<field>_asc()` / `_desc()`
     - `all()` (explicitly means “no user-specified limit”; still bounded by policy caps)
     - `take(n)` (explicit limit)
     - `one()` (implies `take(1)`)
     - `page(token)` / `next()` (paging token based)
3. **Clear defaults (safe + deterministic)**:
   - If no `orderby` specified:
     - use **canonical order** from schema manifest: `primary_key ASC`
     - if no primary key is declared, this is a compile-time error (query is not valid)
   - If no `take(n)` specified:
     - `all()` is assumed, but still bounded by policy caps (`max_rows_default`, `max_rows_hard`, `max_bytes_hard`)
4. **Paging token contract (cursor/continuation)**:
   - `PageToken` is opaque bytes/text, produced by the service, consumed by the client
   - token semantics are tied to `{orderby, last_key}` style (not ad-hoc offsets)
   - token is deterministic given the same snapshot + inputs
5. **Execution model (no new authority)**:
   - QuerySpec is executed only via typed service stubs (Cap’n Proto):
     - either via `contentd.query(...)` (when querying content providers)
     - or via an app/domain service that chooses to expose `Query( QuerySpec, Params )`
   - UI never opens DB files; it only sends QuerySpec to a service that is policy-gated

## Non-Goals

- Direct SQL in the DSL.
- Literals embedded in identifiers (no `...where-id-1...` pattern).
- Joins, OR, ranges, full text search (can be follow-ups once the base contract is proven).
- A new daemon named `datad`/`dbd` as a parallel “database authority”.

## Constraints / invariants (hard requirements)

- **Determinism**:
  - queries execute against a snapshot view (service-controlled), so paging is stable under concurrent writes
  - default order is explicit and documented (PK asc)
- **Safety**:
  - only equality predicates (v0.2c)
  - no string interpolation; all values are typed
  - results are bounded by policy caps (rows + bytes)
- **No fake success**:
  - if results are truncated due to caps, the response must indicate `truncated=true` with deterministic reason code
- **No authority drift**: execution is via existing service authorities (`contentd` or app services), not via UI runtime.

## Proof (Host) — required

`tests/dsl_v0_2c_host/`:

- canonicalization: the same query builder chain produces identical `IrQuerySpec` hashes across runs
- defaults:
  - missing `orderby` uses PK asc (manifest-defined)
  - missing `take(n)` uses `all()` but still applies policy caps
- paging tokens:
  - `next(token)` produces deterministic sequences over a fixture dataset
  - truncation produces deterministic `truncated` flag + reason

## Touched paths (allowlist)

- `userspace/dsl/nx_syntax/` (extend: `db.*` builder syntax)
- `userspace/dsl/nx_ir/` (extend: `IrQuerySpec`)
- `userspace/dsl/nx_interp/` (extend: value model + passing QuerySpec to stubs)
- `userspace/dsl/nx_stubs/` (extend: optional generic query call surface; no new authority)
- `tests/dsl_v0_2c_host/`
- `docs/dsl/db-queries.md` (new; builder + defaults + caps + paging token contract)
