---
title: TASK-0077B DSL v0.2a DevX: keyed per-instance `$state` + complete two-way bindings + async recipes (host)
status: Draft (end-state rewrite 2026-09-09; ~2/3 shipped, residual = keyed state spine + 3 bindings + recipes + lint promotion)
owner: @ui @runtime
created: 2026-01-26
updated: 2026-07-06
depends-on:
  - tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
follow-up-tasks: []
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Language reference: docs/dev/dsl/{state,syntax,patterns}.md
  - Principles this task serves: docs/dev/dsl/principles.md (encapsulation without magic)
---

## End-state rewrite 2026-09-09 (binding; supersedes older sections where they differ)

**Ground truth 2026-09-09 (verified in code):** shipped — local `$state` → implicit store
`__local_<Component>` (`userspace/dsl/core/src/lower/state.rs`, `docs/dev/dsl/state.md`
§49-70, corpus `component_local_state_via_state_block_and_binding`); effect cancellation
latest-wins with generations (`runtime/src/effects.rs`), multi-step `EffectPlan` lowering
(`core/src/lower/effects.rs`); `timeoutMs` lint NX0409; env fixtures
(`runtime/src/fixture_env.rs`, profile-matrix goldens); auto-bind for Toggle/Checkbox (Tap)
and TextField/TextArea (Change) via `Runtime::write_binding`. Missing — the single-use rule is
still a build error (`lower/mod.rs:240-269`, `lower/symbols.rs:339`, corpus
`stateful_component_used_twice_is_rejected`); Slider/Select have no bind arm and **Stepper is
not in the DSL registry at all** (`userspace/ui/widgets/stepper` exists, unreachable from `.nx`);
`patterns.md` has no async-recipes chapter; NX0407/NX0409 are lints, not errors; the named
proof home `tests/dsl_v0_2a_devx_host/` was never created.

### Goal (end system)

Per-instance keyed `$state` (a stateful component works inside any keyed collection and keeps
its state across reorders), two-way bind sugar for all seven value controls, async recipes as
documented and fixture-proven store shapes, NX0407/NX0409 as build errors, one proof home.

### Non-goals

Generics / type-system growth; IO in reducers or views; new triggers; focus traversal (Tab).

### Invariants

- ONE mutation path: bindings and local state reduce through dispatch → reduce → commit
  (`write_binding` = the reducer path); visible in IR; no runtime special cases.
- Bounded: `LOCAL_INSTANCES_MAX = 256` per program; an instance's storage is evicted when its
  keyed row disappears; zero-alloc steady state unchanged.
- No fake success: recipe fixtures assert real state transitions.

### Decisions

- **D1 Keyed store family.** The implicit store gains `Store.keyed: Bool` (IR v1.3, append-only;
  shared schema bump with TASK-0074). Instance key = enclosing ForEach key chain + component
  ordinal, emitted by lowering as `Widget.stateKey`; runtime `store.rs` keeps
  `KeyedLocal { map: BTreeMap<InstanceKey, Fields> }`, reconciled on every collection diff.
- **D2 Single-use rule DELETED.** `lower/mod.rs:240-269` and `count_component_usage`
  (`lower/symbols.rs:339`) go; gate = the conformance case "two instances rejected" is REPLACED by
  "two instances + reorder keep their own state" (a lowering that reintroduces the restriction
  fails that fixture). `state.md` §66-70 restriction text removed.
- **D3 Bind table complete.** `lower/views.rs:258-268` += `("Slider","value")`,
  `("Select","value")`, `("Stepper","value")` → `Change`; `Stepper` added to
  `core/src/registry.rs` and `runtime/src/registry/widgets.rs` (wrapping the existing crate).
- **D4a `timeoutMs:` is retired (handed over by TASK-0054C P2-a, 2026-09-15).** The app-host
  no longer bounds a service call with a client clock (RFC-0093 §7, RFC-0096: the exchange ends
  with the reply or the service's death; `EffectHost::call` ignores `timeout_ms`). This task
  removes the argument from the language: the parser rejects `timeoutMs:` on `svc.*` calls,
  NX0409 becomes the ERROR "no client timeout on a service call" instead of `MissingTimeout`,
  `set_timeout_ms`/`get_timeout_ms` and the IR field go with IR v1.3, `EffectHost::call` loses
  the parameter, docs (`docs/dev/dsl/db-queries.md` and every `timeoutMs` example) follow.
- **D4 Lint promotion.** NX0407 (`UnhandledResult`) and NX0409 (`MissingTimeout`) → `Error`
  in `diag.rs` + `cli/src/explain.rs` + docs; corpus fixtures updated.
- **D5 One proof home.** `tests/dsl_v0_2a_devx_host/` created (root `Cargo.toml` member —
  approval zone) for keyed state, the seven-control binds, async recipes and env variants;
  cases already in `tests/dsl_conformance` stay there (no duplication).
- **D6 Async recipes chapter.** `docs/dev/dsl/patterns.md` "Async recipes": Loading / Loaded /
  Error / Empty store shape, retry, latest-wins cancellation — each backed by a D5 fixture.

### Packages

- **P0** IR v1.3 changelog in `docs/dev/dsl/ir.md` (`Store.keyed`, `Widget.stateKey`;
  `ViewNode.overlayKind` from 0074 rides the same bump if 0074 is next). Blast: app-host build
  (IR bump), `dsl_goldens` regenerate.
- **P1** Keyed lowering + runtime + reorder fixtures. Blast: `dsl_conformance`, `dsl_goldens`,
  `dsl_apps_conformance` (every app still lowers).
- **P2** Binds + Stepper. Blast: `ui_v10_goldens`, settings/stash apps that use Slider/Select.
- **P3** Lint promotion + corpus. Blast: every `.nx` in `userspace/apps` must pass.
- **P4** Docs + proof crate. Blast: paper + `just check` (workspace member).

### Definition of Done

Host (`tests/dsl_v0_2a_devx_host/` + `tests/dsl_conformance`): IR golden shows the keyed store;
reorder fixture keeps per-row state; seven-control bind fixtures update state deterministically
via the narrow-invalidation path; three async-recipe transition goldens (loading→loaded,
loading→error→retry, cancellation); env-variant snapshots; NX0407/NX0409 error fixtures.
No new QEMU markers (host-only ledger); the app-host boot lanes (visible, smp1) must stay green
after the IR bump. Docs: `state.md`, `patterns.md`, `syntax.md`, `ir.md`, `services.md`, `cli.md`.

### Touched paths

`userspace/dsl/{core,ir,runtime}/src/**`, `tools/nexus-idl/schemas/ui_ir.capnp`,
`userspace/ui/widgets/stepper`, `tests/dsl_v0_2a_devx_host/` (new), `tests/dsl_conformance/`,
`docs/dev/dsl/{state,patterns,syntax,ir,services,cli}.md`, root `Cargo.toml`.

### Dependencies

None on TASK-0324 (host work). Precedes 0077C and 0074 (both build on the keyed spine / IR v1.3).

## Rebase (2026-08-14) — ~60-70% shipped — historical, superseded by the end-state rewrite above

### Shipped — do NOT re-implement

- **Local `$state` → implicit per-instance stores**: `state:` block on
  components, lowered to an implicit store through the one mutation path
  (`userspace/dsl/core/src/ast.rs:363-368` — `ComponentDecl.state`;
  reference chapter `docs/dev/dsl/state.md:49-70`).
- **Effect cancellation, latest-wins**: per-(event, case) generations;
  follow-up dispatches tagged with their trigger's generation and dropped at
  dequeue if it advanced (`userspace/dsl/runtime/src/effects.rs:23-28`;
  `docs/dev/dsl/state.md:72-79`).
- **`timeoutMs` first-class** on `svc.*` calls + lint **NX0409** when it is
  missing (`userspace/dsl/core/src/check/lints.rs:173-178`).
- **Change/Submit/Focus/Blur triggers** shipped.
- **Two-way binding mechanism** shipped (IR v1.2 `Handler.bind`,
  `Runtime::write_binding` — same compare-and-mark store path as reducers):
  Toggle/Checkbox (Tap-flip) and TextField/TextArea (Change-write) auto-bind
  at lowering.

### Honest residual scope (Size S-M) — spine first

1. **Keyed per-instance state storage** (the spine): lift the v1 restriction
   "a stateful component is instantiated exactly once … until per-instance
   keyed storage lands" (`docs/dev/dsl/state.md:66-70`, enforced as a build
   error at lowering). The headline proof — **"local state survives keyed
   reorder"** — is blocked exactly on this.
2. **Two-way binding sugar completed across the seven controls**: the
   mechanism exists, the sugar doesn't for the value-carrying/options
   controls — **Slider, Select, Stepper** (the other four already auto-bind).
3. **Async-recipes chapter in `docs/dev/dsl/patterns.md`**: canonical
   loading/error/empty/retry Store shape. The cancellation machinery is
   implemented and documented in state.md; patterns.md must show the recipes.
4. **Create `tests/dsl_v0_2a_devx_host/`**: named as this ledger's proof home
   but never created; the cases proven so far live in `tests/dsl_conformance/`.

> The ⬜ OPEN list at the bottom is partially stale (it still lists
> "`$state` locals" as open although the second DONE increment above it
> records the landing). This rebase section is the authoritative residual.

## Context (updated 2026-07-06) — historical, superseded by the end-state rewrite above

The v0.2a core is powerful; this task makes the common cases feel effortless —
declarative-framework ergonomics — **without hidden magic**. The masterplan pins the
mechanism: local component state compiles to **implicit per-instance stores** using the
exact same reducer machinery (no second semantics, no hidden globals); keyed identity
means local state survives collection reorders (proven in TASK-0076).

Effects support short **multi-step plans** here (the IR `EffectPlan` step list grows
beyond single-call): call → dispatch chains with explicit timeouts and cancellation.

## Goal — historical, superseded by the end-state rewrite above

1. **Local state sugar**: component-level `state` field declarations lower to an
   implicit instance store + generated events for built-in mutations; `$state.field`
   read/write is the primary idiom; the store-vs-local posture documented (local =
   per-instance, ephemeral; shared/durable = named `Store`).
2. **Two-way bindings** complete + deterministic: TextField, TextArea, Checkbox,
   Toggle, Slider, Select, Stepper — bindings only update state (never IO); each is a
   dispatched built-in event through the normal reduce path.
3. **Async recipes** (documented + fixture-proven patterns, not new language):
   - loading/error/empty/retry as a canonical Store shape (`patterns.md` chapter);
   - effect **cancellation tokens**: a newer triggering event cancels the stale plan's
     pending dispatches deterministically;
   - explicit `timeoutMs` on every call step; stable error-code enums end-to-end.
4. **Environment ergonomics**: `device.*`/locale/theme reads are explicit, stable,
   fixture-injectable (no host-OS dependence anywhere).

## Non-Goals

- Generics or type-system growth (patterns.md composition instead). IO in reducers or
  views — never. Real service IPC (TASK-0078). Live-preview IDE (host snapshots are
  the loop). Kernel changes.

## Constraints / invariants (hard requirements)

- One mutation path: bindings and local-state sugar reduce through the same
  dispatch → reduce → commit pipeline (visible in IR, no runtime special cases).
- Deterministic scheduling incl. cancellation (a cancelled plan's dispatches never
  land; fixture-proven).
- Boundedness caps unchanged; zero-alloc steady state unchanged.
- **No fake success**: async recipe fixtures assert real state transitions, never a
  logged "ok" (fake-proof-marker rule).
- No `unwrap/expect`; no godfiles.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_2a_devx_host/`:

- local-state examples lower deterministically; IR shows the implicit store (golden);
- two-way binding fixtures for all seven controls update state deterministically and
  render via the narrow-invalidation path;
- local state survives keyed reorder (collection fixture);
- async recipes: loading→loaded, loading→error→retry, and cancellation (stale plan
  superseded) each with deterministic transition goldens;
- environment fixtures produce stable snapshots across profile/locale/theme variants;
- conformance corpus extended (local-state + cancellation cases).

### Docs — required (reference grade)

- `docs/dev/dsl/state.md`: local-state chapter final; `patterns.md`: async recipes
  chapter with the canonical Store shapes; `syntax.md` examples current.

## Touched paths (allowlist)

- `userspace/dsl/{core,ir,runtime}/` (extend: sugar lowering, EffectPlan steps,
  cancellation)
- `tests/dsl_v0_2a_devx_host/` (new), `tests/dsl_conformance/` (extend)
- `docs/dev/dsl/{state,syntax,patterns}.md`

## Plan (small PRs)

1. local-state lowering (implicit stores) + IR goldens
2. bindings for the seven controls + fixtures
3. EffectPlan multi-step + cancellation + async-recipe fixtures
4. docs (state/patterns/syntax)

---

## STATUS / PROGRESS LEDGER (updated 2026-07-06)

### ✅ DONE (first increment)

- **Two-way bindings, end-to-end (IR v1.2 `Handler.bind`)**: an interactive kind whose
  primary prop is `$state`-bound gets a bind handler **auto-synthesized at lowering** —
  `Toggle { checked: $state.dark }` ⇒ Tap-bind (flips the Bool),
  `TextField/TextArea { value: $state.q }` ⇒ Change-bind (writes the text). The write
  goes through `Runtime::write_binding` = the SAME compare-and-mark store path reducers
  use (one mutation machinery, no side door); changed fields map onto the dep set via
  the shared `View::apply_changes` (dispatch + bindings, one damage pipeline).
  `View::text_input(boxes, x, y, text)` is the host/OS text entry point until focus
  lands. Scene test: tap flips the toggle → branch re-renders ("dark on"); text input
  writes the bound field → bound `Text($state.query)` shows it. Schema minor bump 1.2 +
  ir.md changelog + IR goldens regenerated.

### ✅ DONE (second increment, 2026-07-06)

- **`$state` locals**: `state:` block on components (grammar/AST/fmt) → **implicit store**
  appended after the named stores (`__local_<Component>`, canonical order); `$state.field`
  and auto-bindings resolve local fields through the same field→store map; defaults eval
  like store defaults. **v1 single-use rule enforced at lowering**: a stateful component
  instantiated ≠ 1× (incl. any use inside a collection template) is a build error —
  per-instance keyed storage rides with the retained-instance work. Conformance: toggle
  bound to a local flips the component's branch; two instances rejected.
- **Async cancellation (latest wins)**: per-(event,case) generations in the runtime;
  effect follow-ups are tagged with their trigger's generation and dropped at dequeue if
  it advanced. Conformance: double-fired Search in one cascade — the stale `Found("old")`
  is cancelled, only `"new"` lands.
- Checkbox joins the auto-bind table (same Tap-flip contract as Toggle).
- docs/dev/dsl/state.md: local-state + cancellation chapters current.
- TRAP (fixture): `on` is a keyword — state fields can't be named `on`.

### ⬜ OPEN (this task's remainder)

- Slider/Select/Stepper bindings (value-carrying interactions / options contract).
- **`$state` locals** (component-level state → implicit per-instance stores; needs a
  grammar `state:` block + instance identity from keyed NodeIds).
- **Async recipes**: multi-step EffectPlan chains with explicit cancellation tokens
  (a newer trigger cancels the stale plan's pending dispatches) + timeout/error-code
  promotion of NX0407/NX0409 to errors.
- Focus model (text input currently targets by hit point, not focus).
