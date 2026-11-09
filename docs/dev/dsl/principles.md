<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Design Principles

The DSL is built so that **writing the obvious program produces a well-architected app**.
Users never have to study these principles — the language makes following them the path of
least resistance and makes violating them impossible or a compile error.

Each principle below names its intellectual root, states the rule, and shows the language
mechanism that enforces it.

## 1. Information hiding (Parnas, 1972)

**Rule:** modules hide their implementation; neighbors depend only on small, stable interfaces.

**Mechanism:**
- A `Component` exposes exactly its `props` (including `EventRef` props for the events it
  raises) — nothing else. There is no way to reach into another component's internals
  ([patterns.md](patterns.md)).
- A `Store` is read through `$state.field` and written by the arms of its `reduce`
  declaration; a two-way-bound control writes through the same store mutation path
  ([state.md](state.md)). A component's own `state:` block is per instance.
- Services are only reachable through typed `svc.*` calls from effects. Apps cannot
  observe *how* a service is implemented, only its contract ([services.md](services.md)).

## 2. Abstract data types (Liskov & Zilles, 1974)

**Rule:** data changes only through defined operations; clear data contracts instead of
direct access.

**Mechanism:**
- State fields are declared with a type ([types.md](types.md)); every mutation is a reducer
  arm — a named operation on that state.
- Query access is a first-class `Query` declaration lowered to a bounded `QuerySpec` —
  never a string ([db-queries.md](db-queries.md)).
- A state assignment inside an `@effect` is not lowered: state changes only through
  reducers.

## 3. Structured programming (Dijkstra, 1968)

**Rule:** minimize uncontrolled control flow.

**Mechanism:**
- The view language is declarative: `if/else`, exhaustive `match`, `for` over a statically
  bounded iterable (`NX0404`), and keyed collection templates. There is **no** `goto`, no
  `while`, no recursion in reducers, no callbacks-into-callbacks ([syntax.md](syntax.md)).
- Reducers and effects are represented in the IR as **total expression trees** — the
  representation cannot express a back-edge, so termination holds by construction.

## 4. Contracts and invariants (Hoare, 1969)

**Rule:** describe programs by pre-/postconditions; let tooling prove them.

**Mechanism:**
- Reducer purity is a compile-time contract (no IO, no `svc.*`, no query execution, no
  `dispatch`: `NX0405`) — violation is an error, not a lint suggestion.
- Effects must handle both `Ok` and `Err` of every service call (`NX0407`; error = stable
  code, never a formatted string).
- The IR carries its budgets and the loader validates structure, references and node
  counts before mounting — a tampered bundle fails closed ([ir.md](ir.md)).

## 5. No accidental complexity (Brooks, 1986)

**Rule:** essential complexity cannot be removed; accidental complexity must not be added.

**Mechanism:**
- Every language feature must earn its place by solving a real app's problem.
  Convenience-only features are rejected in design review.
- One state model, one effect model, one query model — no alternates to choose between.

## 6. Simple data first (Wirth, 1976)

**Rule:** good programs come from simple algorithms over fitting data structures.

**Mechanism:**
- The data vocabulary is deliberately small: `Bool`, `Int`, fixed-point `Fx`, `Str`,
  lists, enum cases. Pointer webs are not expressible ([types.md](types.md)).
- Collections render through keyed templates whose identity is stable — the runtime can
  diff and reorder them without user code ([ir.md](ir.md), *Stable node identity*).

## 7. Deep modules (Ousterhout, 2018)

**Rule:** few powerful building blocks with simple APIs beat many shallow special cases.

**Mechanism:**
- The widget set is a curated catalog with a uniform modifier surface — not an open
  plugin zoo ([widgets.md](widgets.md), [modifiers.md](modifiers.md)). Rare needs go
  through one declared escape hatch, `NativeWidget` ([syntax.md](syntax.md)).
- The implementation itself follows the rule: few deep crates
  (`core`, `ir`, `runtime`, `cli`), each with a small public API.

## Derived rules

- State has an owner: a store (one per domain) or a component's per-instance `state:`
  block; nothing else holds mutable state.
- APIs (props, events, service contracts) are small and stable; additive evolution only.
- Declarative over imperative; the little imperative code (reducer bodies) is pure.
- The scene is emitted from committed state, after the reducer has finished.
- Numerics are `Int` and `Fx` (Q32.32 fixed point): no float nondeterminism.
- No hidden per-frame heap churn: frame phases allocate from generation arenas, and the
  arena invariant is tested against the real apps ([runtime.md](runtime.md)).
- Dataflow instead of shared state: state flows down as bindings, events flow up as
  dispatches — nothing else crosses component boundaries.

## Reading list

- D. L. Parnas, *On the Criteria To Be Used in Decomposing Systems into Modules* (1972)
- B. Liskov, S. Zilles, *Programming with Abstract Data Types* (1974)
- E. W. Dijkstra, *Go To Statement Considered Harmful* / structured programming (1968)
- C. A. R. Hoare, *An Axiomatic Basis for Computer Programming* (1969)
- F. P. Brooks, *No Silver Bullet* (1986)
- N. Wirth, *Algorithms + Data Structures = Programs* (1976)
- J. Ousterhout, *A Philosophy of Software Design* (2018)
