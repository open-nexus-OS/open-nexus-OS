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
- A `Component` exposes exactly its `props` and the events it emits — nothing else.
  There is no way to reach into another component's internals.
- A `Store` is only readable through `$state.field` bindings and only writable through
  events handled by its reducers. No component can mutate foreign state directly.
- Services are only reachable through typed `svc.*` adapters from effects. Apps cannot
  observe *how* a service is implemented, only its contract.

## 2. Abstract data types (Liskov & Zilles, 1974)

**Rule:** data changes only through defined operations; clear data contracts instead of
direct access.

**Mechanism:**
- State fields are typed (`Int`, `Fx`, `Str`, `Enum`, `Record`, `List<T>`); every mutation
  is a reducer arm — a named, typed operation on that state.
- Query access is a first-class `QuerySpec` value with typed fields — never a string.
- The compiler rejects writes to `$state` outside reducers.

## 3. Structured programming (Dijkstra, 1968)

**Rule:** minimize uncontrolled control flow.

**Mechanism:**
- The view language is declarative: `if/else`, exhaustive `match`, bounded `for`,
  and collection templates. There is **no** `goto`, no unbounded `while`, no recursion
  in reducers, no callbacks-into-callbacks.
- Reducers and effects are represented in the IR as **total expression trees** — the
  representation cannot express a back-edge, so termination holds by construction.

## 4. Contracts and invariants (Hoare, 1969)

**Rule:** describe programs by pre-/postconditions; let tooling prove them.

**Mechanism:**
- Reducer purity is a compile-time contract (no IO, no `svc.*`, no time/randomness) —
  violation is an error, not a lint suggestion.
- Effects must handle both `Ok` and `Err` of every service call (error = stable code,
  never a formatted string).
- Budgets (max list length, max expression size, result caps) are part of the IR and
  re-validated at load time — a tampered bundle fails closed.

## 5. No accidental complexity (Brooks, 1986)

**Rule:** essential complexity cannot be removed; accidental complexity must not be added.

**Mechanism:**
- Every language feature must earn its place by solving a real app's problem
  (the track's "app-driven capability expansion" rule). Convenience-only features are
  rejected in design review.
- One state model, one effect model, one query model — no alternates to choose between.

## 6. Simple data first (Wirth, 1976)

**Rule:** good programs come from simple algorithms over fitting data structures.

**Mechanism:**
- The data vocabulary is deliberately small and cache-friendly: flat records, typed
  lists, fixed-point numbers. Deep object graphs and pointer webs are not expressible.
- Collections render through keyed templates whose identity is stable — the runtime can
  diff, reorder, and virtualize without user code.

## 7. Deep modules (Ousterhout, 2018)

**Rule:** few powerful building blocks with simple APIs beat many shallow special cases.

**Mechanism:**
- The widget set is a curated catalog with a uniform modifier surface — not an open
  plugin zoo. Rare needs go through one blessed escape hatch (`NativeWidget`) with the
  same determinism/boundedness contract.
- The implementation itself follows the rule: few deep crates
  (`core`, `ir`, `runtime`, `cli`, `codegen`), each with a small public API.

## Derived rules the compiler enforces

- Components own their state completely; there is no global mutable state.
- APIs (props, events, service contracts) are small and stable; additive evolution only.
- Declarative over imperative; the little imperative code (reducer bodies) is pure.
- Immutable by default: the previous state is never observable mid-transition; the UI
  sees committed snapshots only.
- Efficient data layout: numerics are `Int`/`Fx` (fixed-point, no float nondeterminism);
  strings and lists carry explicit caps.
- No hidden heap allocation or copies: the runtime allocates arenas at mount; a
  steady-state event dispatch performs zero heap allocation (tested).
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
