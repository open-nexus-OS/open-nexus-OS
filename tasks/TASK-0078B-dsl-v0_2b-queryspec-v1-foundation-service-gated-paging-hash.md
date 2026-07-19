---
title: TASK-0078B QuerySpec v1: typed query values + canonical hash + keyset paging + the pure-Rust engine (nexus-query/queryd)
status: Done
owner: @ui @runtime
created: 2026-04-03
updated: 2026-07-06
depends-on:
  - tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
follow-up-tasks:
  - tasks/TASK-0274-dsl-v0_2c-db-query-objects-builder-defaults-paging-deterministic.md
  - tasks/TASK-0275-ui-v5c-lazy-data-loading-virtual-list-paging-contract.md
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Query contract: docs/dev/dsl/db-queries.md
  - Persistence substrate the engine sits on: source/services/statefsd (journaled KV over
    virtio-blk, ADR-0023/RFC-0018)
  - Permission gate: source/services/abilitymgr (KNOWN_PERMISSIONS + fail-closed)
  - Consumers: tasks/TASK-0083 (document picker), TASK-0086 (files), master-detail demo (0078)
  - Testing contract: scripts/qemu-test.sh
---

## Context (updated 2026-07-06)

**Engine decision (masterplan 2026-07-06, supersedes the open "libSQL-style backend"
question):** the platform query engine is **pure Rust** — no C SQL engine. A C engine
is unworkable in no_std riscv64 services and its query planner breaks the determinism
charter. Instead:

- `source/libs/nexus-query` (no_std+alloc): typed table schemas (Bool/Int/Fx/Str/Enum
  columns), storage behind a `Kv` trait (get/put/ordered scan-prefix), primary rows +
  secondary index keys maintained transactionally, **order-preserving key encoding**
  for Int/Fx/Str (the core correctness artifact — property-tested);
- `source/services/queryd`: hosts the engine over statefsd (journal gives atomicity),
  capnp IPC (`tools/nexus-idl/schemas/queryspec.capnp`), per-app namespaces from
  bundle identity, gated by a new `nexus.permission.QUERY` registered in abilitymgr;
- host tests run the **identical engine** over an in-memory ordered map ⇒ host/OS
  parity is structural.

The QuerySpec **contract** stays engine-agnostic (SQL-style semantics behind the
service boundary; engine swappable later without observable difference — canonical
ordering fully specified, page tokens opaque).

This task is host-first: engine + contract + DSL integration + queryd skeleton.
Booting queryd into the topology rides with Phase 6 (TASK-0080C).

## Goal

1. **QuerySpec v1 value** (IR + runtime, per `db-queries.md`):
   - source/table handle (generated typed handles), predicates = **conjunction of
     typed comparisons** (`=, <, <=, >, >=`; at most one range column), `orderBy` one
     column asc/desc, `limit ≤ budget`, opaque page token;
   - built purely (reducers/helpers); executed only via the `query` effect step;
   - immutable value semantics; **canonical form + hash**: identical logical queries ⇒
     identical bytes/hash across runs (the identity for caches and later
     subscriptions).
2. **Keyset paging floor**: page token = capnp-encoded (queryHash, last sort key,
   last pk) + integrity digest, opaque to apps; token from a different queryHash is
   rejected; no offset paging anywhere.
3. **DSL syntax floor**: build/pass QuerySpec with source selection, comparisons,
   `orderBy`, `limit`, page-token passing (builder ergonomics grow in TASK-0274).
4. **nexus-query engine**: schemas, write path (row + index maintenance, transactional
   via the journal), query path (index selection for the v1 shape is trivial and
   deterministic — no planner), scan bounds, key-encoding module.
5. **queryd skeleton**: opcodes `CREATE_TABLE/PUT/DELETE/QUERY/QUERY_PAGE` over
   `[opcode u8][capnp]`; namespace derivation; permission check contract
   (fail-closed); host-loopback tested. Boot wiring deferred to Phase 6.
6. **Consumer path**: master-detail (0078) list source switches to QuerySpec paging;
   fixture lines up with the shared list/data proof target.

## Non-Goals

- v2 ergonomics/defaults (TASK-0274); lazy/virtual providers (TASK-0275).
- OR, joins, aggregation, full-text, arbitrary SQL/query strings (documented
  non-goals with the intended v2 shape).
- A DB authority in UI/runtime code; direct storage access from apps — never.
- Boot-topology wiring (Phase 6). Kernel changes.

## Constraints / invariants (hard requirements)

- Pure-build / effect-execute split (compiler-enforced: `query` step is the only
  execution site).
- Determinism: canonical bytes/hash stable across runs and platforms; fully specified
  ordering incl. tie-breaks (pk as final key); cross-page no-dup/no-gap under
  interleaved writes per the written contract.
- Bounds: row/byte/time caps at the service boundary; scan work bounded by
  limit+index (no full-table scans for indexed v1 shapes).
- Engine swappability: nothing observable identifies the engine (error codes,
  ordering, tokens all contract-defined).
- No `unwrap/expect`; no godfiles; no company/product names.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_queryspec_v1_host/` + engine unit/property tests:

- **key-encoding order property tests**: encoded-key order == typed-value order for
  Int (incl. negatives), Fx, Str (prefix cases) — the correctness core;
- same logical QuerySpec ⇒ identical canonical bytes + hash across runs (golden);
- paging: full walk via tokens = no duplicates/no gaps; token round-trip; token with
  foreign queryHash rejected; interleaved-write fixture matches the written contract;
- index maintenance: put/delete keep secondary indexes consistent (property test);
- effect gating: QuerySpec built purely, executed only via the `query` step (lint
  fixture rejects execution elsewhere);
- queryd loopback: opcode round-trips; namespace isolation fixture; permission-denied
  fixture (fail-closed);
- master-detail consumer fixture green on QuerySpec paging.

### Docs — required (reference grade)

- `docs/dev/dsl/db-queries.md` to full reference: the engine decision + rationale,
  canonicalization spec, paging contract, v1 shape + v2/v3 non-goals;
- `docs/dev/dsl/services.md` query-step section.

## Touched paths (allowlist)

- `source/libs/nexus-query/` (new), `source/services/queryd/` (new, skeleton)
- `tools/nexus-idl/schemas/queryspec.capnp` (new) + idl-runtime module
- `userspace/dsl/{core,ir,runtime}/` (QuerySpec value/syntax/effect step)
- `source/services/abilitymgr/` (register `nexus.permission.QUERY`)
- `examples/dsl/masterdetail/` (consumer), `tests/dsl_queryspec_v1_host/` (new)
- `docs/dev/dsl/{db-queries,services}.md`

## Plan (small PRs)

1. key encoding + property tests (the correctness core, standalone)
2. nexus-query schemas/write/query paths over Kv + in-memory host Kv
3. queryspec.capnp + canonical hash + paging tokens
4. DSL value/syntax/effect step + gating lints
5. queryd skeleton (loopback) + permission registration
6. master-detail consumer + docs

---

## STATUS / PROGRESS LEDGER (updated 2026-07-06)

### ✅ DONE — plan items 1–3 (the `nexus-query` engine, host-proven)

`source/libs/nexus-query` (no_std+alloc, **zero dependencies**, `forbid(unsafe)`,
riscv64 no_std check green, clippy clean, 21 tests):

- **`encoding`**: order-preserving key codec (Int/Fx sign-flip→BE; Str 0x00-escape +
  double-0x00 terminator, self-terminating for tuple concatenation) + deterministic
  bounded row codec. Exhaustive pair property tests (sign, prefix, NUL, tuples).
- **`kv`**: ordered `Kv` trait (get/put/delete/scan/scan_rev) + host `MemKv` —
  identical engine over statefsd (queryd) later; host proofs transfer structurally.
- **`spec`**: QuerySpec v1 value (eq conjunction, ≤1 range on the ORDER column,
  one orderBy, mandatory limit), canonical bytes (eq sorted by column) + FNV-1a64
  hash **pinned by a golden** (0x724d_3c50_22ec_6e82 — recompute only on a
  documented canonicalization change), `Page` + hash-bound opaque `PageToken`.
- **`engine`**: `TableDef` catalog; `put` replace-semantics with stale-index removal;
  `delete`; index-driven `query` (order col must be indexed; range rides the index;
  **no post-sort**; pk tie-break part of the contract; MAX_SCAN=4096 hostile bound);
  keyset resume asc=strictly-after / desc=exclusive-end; stable `QueryError` enum
  (UnknownTable/UnknownColumn/TypeMismatch/Unsupported/BadToken/Corrupt).
- **Integration proof** (`tests/engine_paging.rs`): paged walk == one-shot result at
  many page sizes (asc/desc/filtered/ranged/pk-ordered — no dups, no gaps);
  interleaved-write keyset contract (behind-cursor inserts don't resurface, ahead
  ones appear); foreign/malformed token rejection; token wire round-trip; hash
  order-independence + pinned golden.
- **Docs**: `docs/dev/dsl/db-queries.md` extended to reference grade — engine
  decision + rationale, key-encoding spec, storage layout, v1 shape, canonical
  form/hash, keyset paging contract, v1 non-goals.

### ✅ DONE (second increment, 2026-07-06 — plan items 3–6: wire + DSL + queryd + consumer)

- **IR v1.3** (`ui_ir.capnp`, changelog in ir.md, goldens regenerated):
  `QuerySpec` grows paramCount/preds(col,op∈{eq,ge,le},value)/orderCol/
  descending/limit; `QueryStep` grows token/rowsSlot/nextSlot.
- **DSL surface end-to-end**: top-level `Query Name on source { params:/where/
  orderBy/limit }` declaration (ALL clause words contextual — `query` stays a
  valid field name) → parser/fmt/checker/lowering; execution ONLY as
  `match Name(args…, token: t) { Ok(rows, next) => …, Err(e) => … }` inside
  effects. New stable codes: **NX0410** QueryShape (strict ops, range off the
  order column, computed pred values, limit outside 1..=1000), NX0405 extended
  to query execution in reducers; call-site named-param coverage (NX0302/0303).
- **Runtime**: `EffectHost::query` (default = deterministic
  `ERR_QUERY_UNSUPPORTED`), spec+args flattened into a resolved `QueryCall`
  (eq conj + inclusive bounds + token), Ok binds rows+next / Err binds the
  stable code and stops the plan (call-step semantics).
- **`queryspec.capnp`** wire contract + `nexus-idl-runtime::queryspec_capnp`
  module: QVal/preds/QueryRequest/PageResult, typed `QueryErr` vocabulary
  (engine errors + denied/badRequest), opcodes CREATE_TABLE/PUT/DELETE/QUERY.
- **`source/services/queryd`** skeleton: `[opcode u8][capnp]` frame handler
  over the engine; namespaces DERIVED from caller identity (prefix-scoped Kv
  view — nothing on the wire selects one); `Caps` gate **fail-closed** before
  payload parse. 5 loopback tests: opcode round-trip, wire keyset paging walk,
  namespace isolation, denial (DenyAll + non-listed identity), typed errors.
- **abilitymgr**: `nexus.permission.QUERY` registered in KNOWN_PERMISSIONS.
- **masterdetail consumer**: library.store.nx loads via `Query LibraryItems`
  + token in state; conformance proof `tests/dsl_conformance/tests/query.rs`
  (6 cases) runs the REAL engine behind the host seam (`EngineHost`): 3-page
  walk through DSL state, param→range flow, error path, purity + shape lints.
- **Docs**: db-queries.md gains the DSL syntax reference + queryd boundary
  section; services.md query-step section; grammar.md QueryDecl production.

### ⬜ OPEN (deferred per Non-Goals / later phases)

- Boot/topology wiring + statefsd-journal `Kv` behind queryd (Phase 6,
  TASK-0080C); abilitymgr gate over real IPC.
- DSL-side generated typed table handles (v2 ergonomics, TASK-0274); strict
  `<`/`>` bounds (engine Range exclusivity — a documented canonical-bytes
  change).

## Closure (2026-07-19) — Reconciliation
Delivered + host-proven: `source/libs/nexus-query/` (1264 LOC, zero-dep, `forbid(unsafe)`): order-preserving `encoding.rs`, `spec.rs` with pinned canonical-hash golden `0x724d_3c50_22ec_6e82`; paging correctness in `tests/engine_paging.rs` (paged-walk == one-shot, no dup/gap, foreign/malformed token rejection, hash order-independence). Service path `source/services/queryd/` (634 LOC) + `tests/loopback.rs` (opcode round-trip, wire keyset paging, namespace isolation, fail-closed). Status → Done. Only OPEN item = boot/topology wiring, an explicit Phase-6 Non-Goal.
