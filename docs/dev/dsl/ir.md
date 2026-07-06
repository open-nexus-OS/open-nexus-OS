<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The Canonical IR (`.nxir`)

`nx dsl build` lowers `.nx` source into a deterministic, bounded, binary IR. The IR is
**the contract of the whole system**: the interpreter, the app-host, and the AOT
codegen all execute the same IR under one written semantics — nothing may be
implemented in only one tier.

## Formats

- **Canonical**: `.nxir` — Cap'n Proto (schema: `tools/nexus-idl/schemas/ui_ir.capnp`),
  chosen for zero-parse bounded reads (an app mount validates and indexes; it never
  parses text or compiles).
- **Derived**: `.nxir.json` — deterministic JSON view for host goldens and debugging
  only. Never consumed at runtime.

## Top-level structure

```capnp
struct UiProgram {
  schemaVersion :UInt16;   # major.minor; readers reject unknown majors
  programHash   :Data;     # SHA-256 over canonical bytes with this field zeroed
  sourceDigest  :Data;     # digest of the canonical source set (build provenance)
  symbols       :List(Text);      # interned, sorted, unique; all refs are u32 ids
  stores        :List(Store);
  events        :List(EventDecl);
  reducers      :List(Reducer);   # (store, event) -> Body
  effects       :List(EffectPlan);
  components    :List(Component); # includes Pages; view templates
  routes        :List(Route);
  i18nKeys      :List(I18nKey);
  querySpecs    :List(QuerySpec);
  conditions    :List(DeviceCond);# device.* branch conditions, deduplicated
  assets        :List(AssetRef);  # name, kind, digest
  budgets       :Budgets;         # caps: nodes, expr size, list len, str len, ...
}
```

## Reducers and effects: total expression trees

Reducer bodies and effect plans are stored as **typed expression/statement trees**, not
bytecode. The representation has no back-edges, so **termination holds by
construction** — no verifier, no fuel counters. Iteration exists only as capped
collection combinators (`map`, `filter`, `findFirst`, `removeWhere`, `append`-with-cap).

- The interpreter walks the trees directly over Cap'n Proto readers (zero-parse).
- AOT lowers each tree to straight-line native code.
- Both implement the same small-step semantics document; a shared conformance corpus
  (`(state, event) → state'` fixtures) is executed by both and must agree exactly.
- Every expression node carries its type; the loader **re-typechecks on mount**
  (fail-closed against tampered bundles).

Effect plans are bounded step lists (`call` with timeout + `onOk`/`onErr` dispatches,
`dispatch`, `query`), not general code.

## Stable node identity

Every layout-relevant view node carries a persisted 64-bit id:

```text
nodeId = hash64(component symbol ∥ structural path ∥ optional user key)
```

Collection items derive ids at runtime from their `.key(expr)` value with the same
hash. Consequences:

- the retained instance tree, AOT output, golden snapshots, and a11y references all
  agree on identity across rebuilds;
- equivalent recompilations never change identity because of formatting or file
  traversal order;
- keyed items keep their local state across reorders.

## Field classification

Every widget property and modifier is classified in the compiler SSOT
(`userspace/dsl/core/modifiers.toml`, mirrored in `modifiers.md`):

- **layout** — constraints, spacing, text content/typography, visibility: dirties
  measurement + placement;
- **paint** — colors, tokens, opacity, shape, motion: repaint only, never remeasure;
- **semantics** — labels, roles, hints: a11y tree only, no pixels.

Each binding site in the IR records which classes it can dirty; the runtime's
dependency index (store field → binding sites) turns a dispatched event into a minimal,
class-partitioned dirty set. This is the mechanism behind microsecond-scale reactive
updates.

## Canonicalization (determinism)

- symbols sorted and unique; all order-insensitive lists sorted by symbol id;
- modifier lists in catalog order; duplicate modifiers are a build error;
- canonical Cap'n Proto encoding; no host timestamps, paths, or environment leakage;
- `programHash` computed over the canonical bytes;
- **invariant proven in CI**: building the same source twice yields byte-identical
  `.nxir` (`build; build; cmp`).

## Schema evolution rules

1. Field numbers are **append-only**; nothing is renumbered or reused.
2. **Minor** version bump = additive fields with defaults; older readers keep working.
3. **Major** version bump = readers reject; requires a migration note.
4. Every schema change requires (a) an entry in the changelog below, (b) regenerated IR
   golden fixtures (byte-compared in CI).
5. The validator and the runtime must agree exactly: the runtime never tolerates what
   the validator rejects.

## Budgets & validation on load

`Budgets` caps the whole program (node count, expression sizes, list/string caps,
effect steps, route count). The app-host validates budgets, types, symbol references,
and the program hash before mounting; any failure is a deterministic launch error —
never a partial mount.

## Changelog

- **v1.3 (2026-07-06, TASK-0078B)** — additive: `QuerySpec` grows the v1
  shape (`paramCount`, `preds` (col/op/value; `QueryOp` = eq/ge/le),
  `orderCol`, `descending`, `limit`); `QueryStep` grows `token` (page-token
  expression), `rowsSlot`, `nextSlot` (the Ok path binds rows + next token;
  the Err path binds the stable error code into `rowsSlot` — only one path
  ever runs). Predicate values are const literals or `paramGet` only, bound
  from `QueryStep.args` in declaration order. Strict `<`/`>` comparisons are
  reserved for the v2 builder; lowering rejects them today (NX0501).
- **v1.2 (2026-07-06, TASK-0077B)** — additive: `Handler.bind` (two-way
  binding write target). Auto-synthesized at lowering when an interactive
  kind's primary prop is `$state`-bound (`Toggle { checked: $state.on }` ⇒ a
  Tap-bind flipping the Bool; `TextField { value: $state.q }` ⇒ a Change-bind
  writing the text). The write goes through the store's compare-and-mark path
  — the same single mutation machinery reducers use.
- **v1.1 (2026-07-06, TASK-0077)** — additive: `Handler.navigate` (a Str-typed
  route-path expression; `on Tap -> navigate("/detail/7")`). Readers of 1.0
  see an unknown union variant and must treat such handlers as inert.
- **v1.0 (2026-07-06, TASK-0075)** — initial schema
  (`tools/nexus-idl/schemas/ui_ir.capnp`): `UiProgram` with interned sorted
  symbols, budgets, expression-tree reducers, linear effect plans
  (call/dispatch/query steps; a call binds its result on Ok and continues,
  dispatches `onErr` and stops on Err), components/pages with persisted
  NodeIds, routes, i18n keys, QuerySpec skeleton, assets. `TypeRef` includes
  an `opaque` placeholder for not-yet-statically-known types (service/domain
  schemas replace it in v0.2b); the loader skips re-typecheck for
  opaque-typed nodes. Golden: `tests/dsl_v0_1a_host/goldens/proof_surface.nxir`.
