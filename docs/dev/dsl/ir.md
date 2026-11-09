<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The Canonical IR (`.nxir`)

The compiler lowers `.nx` source into a deterministic, bounded, binary IR. The IR is
**the contract of the whole system**: the app-host process and the host harness (CLI,
conformance and golden suites) execute the same IR with the same runtime crate
(`userspace/dsl/runtime`) — nothing may be implemented in only one place. How the
language maps onto it: [syntax.md](syntax.md), [state.md](state.md). How it runs:
[runtime.md](runtime.md).

## Formats

| Format | Role |
|---|---|
| `.nxir` | **Canonical.** Cap'n Proto, schema `tools/nexus-idl/schemas/ui_ir.capnp`; chosen for zero-parse bounded reads — a mount validates and indexes, it never parses text or compiles. |
| `.nxir.json` | Derived summary written by `nx-dsl build --emit-json` ([cli.md](cli.md)): program hash, byte length, schema version, entry page, table sizes and the symbol list. For debugging only; never read at runtime. |

Rust bindings and the loader live in `userspace/dsl/ir` (`read`, `validate`, `node_id`,
`hashing`); the version constants are `SCHEMA_MAJOR` / `SCHEMA_MINOR` in its `lib.rs`.

## Top-level structure

```capnp
struct UiProgram {
  schemaVersionMajor :UInt16;        # readers reject any other major
  schemaVersionMinor :UInt16;
  programHash        :Data;          # SHA-256 over the canonical bytes with this field zeroed
  sourceDigest       :Data;          # SHA-256 of the canonical source set (provenance)
  symbols            :List(Text);    # interned, sorted, unique; every reference is a u32 index
  budgets            :Budgets;
  types              :List(TypeDef); # program-declared enums/records
  stores             :List(Store);
  events             :List(EventDecl);
  reducers           :List(Reducer); # (store, event) -> exhaustive arms over the event's cases
  effects            :List(EffectPlan);
  components         :List(Component); # pages are components with isPage
  routes             :List(Route);
  i18nKeys           :List(I18nKey);
  querySpecs         :List(QuerySpec);
  assets             :List(AssetRef);  # name, kind (svg | localeCatalog | raw), digest
  entryPage          :UInt32;        # index into components; the "/" route's page, else the first page
  window             :WindowIntent;  # the app's Window declaration (see window.md)
}
```

`Budgets` carries seven caps: `maxViewNodes`, `maxExprNodes`, `maxListLen`, `maxStrLen`,
`maxEffectSteps`, `maxLocals`, `maxChildren`. The compiler writes one default set
(`DEFAULT_BUDGETS` in `userspace/dsl/core/src/lower/mod.rs`: 4096 · 1024 · 1024 · 4096 ·
16 · 32 · 64). See *Validation on load* for which of them the loader enforces.

## Reducers and effects: total expression trees

Reducer bodies and effect plans are **typed expression/statement trees**, not bytecode.
The representation has no back-edges, so termination holds by construction — no verifier,
no fuel counter.

| Construct | IR form |
|---|---|
| Statements | `set` (`=`, `+=`, `-=` on a state path), `let`, `if/else`, `match` over an enum |
| Expressions | literals (bool, int, Q32.32 fixed, string, enum case, list), `fieldGet`, `localGet`, `paramGet`, unary and binary operators (`+ - * / %`, comparisons, `&& ||`, string concat), record field read, `fmtI18n`, `deviceGet` |
| List operations | `len`, `tail`, `skip`, `take` (the ones the compiler lowers and the runtime evaluates); `+` concatenates strings and lists |

The schema also declares `get`, `append`, `removeWhere`, `map`, `filter`, `findFirst`,
`isEmpty` and `contains` list operations; the runtime does not evaluate them and the
compiler does not emit them. The runtime evaluates `recordMake` and `optionSome` /
`optionNone`, which the compiler does not emit either. There are no loops: iteration is a
list operation.

- The interpreter walks the trees directly over Cap'n Proto readers, the same code on the
  host and in the app-host process; the `(state, event) → state'` corpus in
  `tests/dsl_conformance` pins the semantics ([testing.md](testing.md)).
- Every expression node carries a `TypeRef` (`bool`, `int`, `fx`, `str`, `enumType`,
  `record`, `list`, `option`, `result`, `eventRef`, `idType`, `unit`, `opaque`). The
  compiler lowers whatever it cannot name to `opaque` (see [types.md](types.md)); the
  loader does not re-typecheck expressions.

Effect plans are bounded step lists — `call` (service call with `onOk` / `onErr`
dispatches and a result slot), `dispatch`, `query` (a `QuerySpec` page request) — not
general code. A call binds its result on Ok and continues; on Err it dispatches `onErr`
and stops. A service call carries no client timeout ([services.md](services.md)).

## Stable node identity

Every layout-relevant view node carries a persisted 64-bit id:

```text
nodeId = FNV-1a-64(length-prefixed component name ∥ structural child indices)
```

The algorithm is part of the IR contract (`userspace/dsl/ir/src/node_id.rs`) and never
changes within a schema major. It is identity, not integrity — integrity is
`programHash`. Collection items derive their id at runtime as
`keyed_item_id(template nodeId, key bytes)` from their `.key(expr)` value. Consequences:

- the retained instance tree and the golden snapshots agree on identity across rebuilds;
- equivalent recompilations never change identity because of formatting or file order;
- keyed items keep their per-instance `state:` across reorders ([state.md](state.md)).

Path segments are disjoint by construction: a plain child is its index, a branch arm is
`(arm << 8) | j`, the else body is `0xff00 | j`, and a component slot body bound at a
call site is `caller_path ++ [0x00F5_0000, slot_index, j]`, so ids never collide.

## Field classification

Every modifier has a class in the compiler catalog (`MODIFIERS` in
`userspace/dsl/core/src/registry.rs`, mirrored in [modifiers.md](modifiers.md); the row
index is the persisted `modId`):

- **layout** — constraints, spacing, text metrics, visibility: dirties measurement and
  placement;
- **paint** — colours, opacity, shape, motion: repaint only, never remeasure;
- **semantics** — labels, roles, hints: no pixels.

The runtime classifies at emit time: while it emits a scene it records, for every state
field an emitted site reads (a binding, a branch condition, a modifier argument, a
component prop), a dependency `(store, field) → Damage` with `Damage` one of `None`,
`Paint`, `Layout` (`userspace/dsl/runtime/src/emit/deps.rs`). `View::dispatch` returns the
strongest class among the fields the event changed. The dependency index is derived from
the IR on every emit; it is not stored in it.

## Canonicalization (determinism)

- symbols are sorted and unique; stores, events, components, queries and routes are
  ordered by name (routes by path); property and argument lists by symbol id;
- each modifier list is sorted by `modId` (catalog order); a duplicate modifier is the
  error `NX0403`;
- slots keep declaration order (that order is their index space); bound slot arguments
  are ascending by slot index and the validator rejects any other order;
- canonical Cap'n Proto encoding; no host timestamps, paths or environment leakage;
- `programHash` is computed over the canonical bytes with the hash field zeroed;
- building the same source twice yields byte-identical `.nxir`
  (`dsl_v0_1a_host::determinism::build_is_byte_deterministic`), and the golden
  `tests/dsl_v0_1a_host/goldens/proof_surface.nxir` is byte-compared.

## Schema evolution rules

1. Field numbers are **append-only**; nothing is renumbered or reused. A retired field
   keeps its ordinal under a `retired…` name.
2. **Minor** bump = additive fields with defaults (or a retirement that nothing in the
   tree reads); readers gate on the major only.
3. **Major** bump = readers reject; requires a migration note.
4. Every schema change adds a row to the table below and refreshes the IR goldens.
5. The validator and the runtime agree exactly: the runtime never tolerates what the
   validator rejects and never re-checks what it proved.

A program that uses a newer construct needs a runtime at or above its minor: an older
reader meeting an unknown union variant fails with `Malformed`.

## Validation on load

`validate_program` runs once at mount, before anything trusts the payload, and returns the
first violated invariant as a stable `IrError`:

| Check | Error |
|---|---|
| schema major equals `SCHEMA_MAJOR` | `UnsupportedMajor` |
| digest fields are 32 bytes | `BadDigest` |
| `programHash` equals the recomputed hash (feature `hash-verify`) | `HashMismatch` |
| symbols are sorted and unique | `SymbolsNotCanonical` |
| every index (stores, events, components, routes, entry page, slots, component refs) is in range | `DanglingRef` |
| view-node count against `maxViewNodes` and children per node against `maxChildren`, slot bodies included | `BudgetExceeded` |
| message decodes within its traversal budget | `Malformed` |

Only `maxViewNodes` and `maxChildren` are checked against the program's contents;
the other five caps are written into the IR and are not enforced by the loader.
`hash-verify` is a default feature of `nexus-dsl-ir`: the CLI and host tests recompute the
hash; the app-host build disables it and treats the payload as inside its trust boundary. A validation failure is a deterministic launch error, never a partial mount.

## Schema versions

Current writer: **1.8**.

| Minor | Change | Task |
|---|---|---|
| 1.0 | Initial schema: interned sorted symbols, budgets, expression-tree reducers, linear effect plans, components with persisted node ids, routes, i18n keys, `QuerySpec` skeleton, assets. Golden `proof_surface.nxir`. | TASK-0075 |
| 1.1 | `Handler.navigate` — a `Str` route-path expression (`on Tap -> navigate("/detail/7")`). | TASK-0077 |
| 1.2 | `Handler.bind` — two-way binding, synthesized at lowering when an interactive widget's primary prop is `$state`-bound. | TASK-0077B |
| 1.3 | `QuerySpec` v1 (`paramCount`, `preds`, `orderCol`, `descending`, `limit`; `QueryOp` = eq/ge/le); `QueryStep.token` / `rowsSlot` / `nextSlot`. Strict `<` / `>` are rejected at lowering (`NX0501`). | TASK-0078B |
| 1.4 | `UiProgram.window` (`WindowIntent`). | — |
| 1.5 | Component slots: `Component.slots`, `ViewNode.slot`, `ComponentRef.slots`, `SlotArg`. Bound slots splice at the placeholder; an unbound slot contributes zero nodes. A slot body is lowered once, in the caller's scope. | RFC-0084 |
| 1.6 | Subtractive: `CallStep.timeoutMs` retired (ordinal `@3` is `retiredTimeoutMs`); `timeoutMs:` on a call is `NX0412`. | TASK-0077B |
| 1.7 | `Store.keyed` — per-instance state addressed by node identity. | TASK-0077B |
| 1.8 | `Handler.bind` carries `BindWrite { target, value }` (ordinal `@4` is `retiredBind`). `BindValue` = `toggleBool` (invert the Bool), `text` (the interaction's string), `trackFraction` (position across the control's box, percent 0–100). | TASK-0077B |

The bind rule travels in the IR because the compiler already decides it from the widget
catalog and the runtime deliberately cannot see that catalog: a runtime table keyed by
widget kind would be a second answer to one question, and changing the runtime could
silently change what an already compiled `.nxir` does.

`WindowIntent.width`, `height`, `minWidth` and `minHeight` (RFC-0117) are additive;
`lib.rs` lists no minor of their own for them, so the writer stays at 1.8.
