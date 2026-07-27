# RFC-0084: DSL slots v1 — component content regions, caller-scope bodies, IR v1.5

- Status: Draft
- Owners: @ui @runtime
- Created: 2026-07-27
- Last Updated: 2026-07-27
- Links:
  - Tasks: `tasks/TASK-0308-dsl-slots-and-window-scaffold.md` (execution + proof)
  - ADRs: none required (compiler/wire change inside one toolchain). One
    amendment paragraph below covers the `device.sizeClass` contract touch.
  - Related RFCs: `docs/rfcs/RFC-0067-windowd-compositor-boundary.md`
    (window UI is a widget, windowd is a service),
    `docs/rfcs/RFC-0082-type-ramp-v2-on-glass-surface-tokens.md`
    (the glass/`Panel` paint SSOT this builds on)

## Status at a Glance

- **Phase 0 (ratchet unblock)**: ⬜
- **Phase 1 (syntax → AST → checker)**: ⬜
- **Phase 2 (IR v1.5 + lowering + validator)**: ⬜
- **Phase 3 (runtime emit)**: ⬜
- **Phase 4 (`WinAppWindow` scaffold)**: ⬜

Definition: "Complete" means the **contract** is defined and the **proof gates**
are green. It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs
live in `tasks/TASK-0308-*`.

- **This RFC owns**:
  - The `.nx` surface grammar for slot declaration, the `Slot` placeholder and
    the callsite slot block.
  - The **scoping law** for slot bodies (they are lexically part of the caller).
  - The **splice-not-wrap law** (a bound slot contributes N nodes at the
    placeholder's real tree position; the runtime never inserts a container).
  - The IR v1.5 wire surface (`Component.slots`, `ViewNode.slot`, `SlotRef`,
    `ComponentRef.slots`, `SlotArg`), its canonical ordering, and the node-id
    path encoding.
  - The validator invariants that make slot programs fail closed.
- **This RFC does NOT own**:
  - The `WinAppWindow` component's prop set or the Stash visual parity work —
    those are the task's business, this RFC only guarantees the mechanism.
  - Per-instance component state (still one implicit store per stateful
    component, unchanged).
  - `svc.files` query filtering, or the `device.sizeClass` breakpoint values.

### Relationship to tasks (single execution truth)

- `tasks/TASK-0308-dsl-slots-and-window-scaffold.md` defines the stop
  conditions and proof commands for every phase below.

## Context

`.nx` components take **scalar props only** (`Str`/`Bool`/`Int`;
`userspace/dsl/core/src/ast.rs:363`) and lower to separate IR entries
referenced by index (`userspace/dsl/core/src/lower/mod.rs:106`) — they are
never inlined. There is no way to pass *content* to a component.

The consequence is visible in the shipped apps. `userspace/apps/window-kit/`
contains five leaf components (`WinTopBar`, `WinSideItem`, `WinMenuItem`,
`WinActionItem`, `WinPropRow`) and **no window scaffold**, because a
three-zone body (sidebar · content · properties) is by definition a container
that takes content. So `userspace/apps/stash/ui/pages/StashPage.nx` (512 LOC)
and `userspace/apps/settings/ui/pages/SettingsPage.nx` (416 LOC) hand-write
the same skeleton, and every future app would too.

It also blocks a concrete design requirement. The window design handoff
(`userspace/apps/stash/docs/design_handoff_files_window/`) hard-codes a panel
surface behind the content and properties regions and explicitly denies one to
the sidebar (`reference/components/AppWindow.jsx:168/185`,
`Sidebar.jsx:61-64`). Open Nexus wants the opposite: **the panel background is
a per-region decision the app developer makes** — zero panels, one, or several
stacked (a settings pane), and the sidebar must be able to have one. That is
only expressible if the scaffold hands its regions to the app as content
slots rather than painting them itself.

Two pre-existing fail-open holes sit directly on this path and are closed here
rather than left behind new syntax:

1. `lower_component_ref` (`userspace/dsl/core/src/lower/views.rs:761-783`)
   reads only `positional` and `props` — it **silently discards**
   `widget.children` and `widget.modifiers`. `WinSideItem { Text("x") }.grow(1)`
   compiles today and both the child and the modifier vanish.
2. `ComponentRef.component` is **never bounds-checked**:
   `userspace/dsl/ir/src/validate.rs:165` is a documented no-op and
   `userspace/dsl/runtime/src/emit.rs:186` calls `components.get(idx)`
   unguarded.

## Goals

- A component may declare named content regions; a callsite may fill them.
- A slot body behaves exactly as if it had been written where the placeholder
  stands — same scope, same layout participation.
- Zero cost when unused: existing programs lower byte-identically.
- Fail closed: every new index is bounds-checked and every new node counts
  against the existing traversal budgets.

## Non-Goals

- **Slot forwarding** (`Slot x` inside a callsite slot body). Rejected in v1;
  the seam is named in §Alternatives.
- Default/unnamed slots, typed slots (`slot rows: List<T>`), per-slot
  `required`.
- Per-instance component state. The "a stateful component is instantiated
  exactly once" rule (`lower/mod.rs:257-263`) is preserved, not relaxed.
- Dynamic dispatch of store events through props (still impossible —
  `HandlerAction::Dispatch` takes a static `Ident`, `ast.rs:456`).

## Constraints / invariants (hard requirements)

- **Determinism**: slot bodies are lowered **once**, at the callsite, in
  declaration-index order; `ComponentRef.slots` is sorted ascending by slot
  index. Recompiling the same source produces byte-identical `.nxir`
  (`sourceDigest`, `lower/mod.rs:661`).
- **Bounded resources**: slot-body nodes count against `maxViewNodes` /
  `maxChildren` — the validator's `count_view_nodes` **must** recurse into
  `ComponentRef.slots[*].body`, which it does not do for any ComponentRef
  content today.
- **No fake success**: an unbound slot emits **zero** nodes, never an empty
  box. A missing binding is silence, not a placeholder.
- **Security floor**: no new untrusted input path — `.nxir` is already
  bounds-read. Every new index (`SlotRef.slot`, `SlotArg.slot`,
  `Component.slots[i]`) is validated before use, and the pre-existing
  unchecked `ComponentRef.component` is fixed in the same pass.
- **Stubs policy**: none. Phase 1 lands with lowering rejecting slots
  explicitly (`LoweringUnsupported`), which is an honest error, not a stub
  that claims success.

## Proposed design

### Contract / interface (normative)

#### 1. Declaration — `slot <name>`

```
Component WinAppWindow {
    props: { contentPanel: Bool, propsPanel: Bool, }
    slot content
    slot properties
    Stack { ... }
}
```

`slot` is a **contextual identifier**, not a lexer keyword — the established
precedent in this parser (`parser/decls.rs:189/203` matches
`Ident("params"|"where"|"orderBy")`). Adding a keyword would retroactively
break any `.nx` using `slot` as a prop, state field or local name.

Slot declarations appear after the optional `props:` and `state:` blocks and
before the component's single root view node. **Declaration order is wire
order** — slots are addressed by index, not by name, in the IR.

`Slot` is a reserved component name.

#### 2. Placeholder — `Slot <name>`

A view node valid only inside a `Component` (never a `Page`, which has neither
props nor slots):

```
Stack { Slot content }.grow(1)
```

#### 3. Callsite — a second brace block

```
WinAppWindow { contentPanel: true, propsPanel: false } {
    content    { Panel { ... }  Panel { ... } }
    properties { WinPropRow { ... } }
}
.grow(1)
```

Unambiguous by construction: after the prop block closes
(`parser/view.rs:167`) the grammar admits only `.modifier(…)`, `on …`, or the
next sibling node — and a sibling always starts with `Ident`/`if`/`for`/
`match`, never `{`. A `LBrace` in that position is a hard parse error today.

Order in `widget_like`: positional sugar → prop block → **slot block** →
modifiers → handlers.

A slot block is legal **only** on a component reference, only for slots that
component declares, and each slot at most once.

#### 4. The scoping law (normative)

> **A slot body is lexically part of the caller.** `$props`, `$state`, loop
> locals and `.key()` inside a slot body resolve in the **caller's** frame.
> The callee's props are never visible to a body it receives.

This is enforced, not emergent: the checker rejects `$props.<n>` in a slot
body when `<n>` is not a prop of the *enclosing* component, and the runtime
restores the caller's `params` **and a snapshot of the caller's locals**
before emitting a body.

The locals snapshot is what makes this airtight rather than usually-right:
`locals` are shared across the component boundary today
(`emit.rs:203-220`), so a `for` inside the callee can clobber a caller loop
binding — and a slot body is emitted *after* that could have happened.

#### 5. The splice-not-wrap law (normative)

> **A bound slot contributes its N nodes at the placeholder's real position
> among its parent's children. The runtime never inserts a container.**

Wrapping would reset the flex context and break `.grow(1)` / `.width(240)` on
the receiving region — the exact hazard already documented at
`runtime/src/emit.rs:162` (transparent single-node branch bodies) and `:358`
(the `ForEach` splice). Slots follow the same rule for the same reason.

An unbound slot contributes **zero** nodes.

For a `Slot` that is not a direct widget child (e.g. a lone branch-arm body)
the branch precedent applies: 1 node returns transparently, 0 or N wrap in a
`Stack`.

#### 6. IR v1.5 (additive; `SCHEMA_MINOR` 4 → 5)

`tools/nexus-idl/schemas/ui_ir.capnp`:

```capnp
struct Component {
  name   @0 :UInt32;
  isPage @1 :Bool;
  props  @2 :List(FieldDef);
  view   @3 :ViewNode;
  slots  @4 :List(UInt32);      # v1.5: declared slot name symbol ids, DECLARATION order
}

struct ViewNode {
  nodeId @0 :UInt64;
  union {
    widget       @1 :Widget;
    forEach      @2 :ForEach;
    branch       @3 :Branch;
    componentRef @4 :ComponentRef;
    slot         @5 :SlotRef;   # v1.5
  }
}

struct SlotRef   { slot @0 :UInt16; }   # index into the ENCLOSING component's `slots`

struct ComponentRef {
  component @0 :UInt32;
  args      @1 :List(PropInit);
  slots     @2 :List(SlotArg);  # v1.5: bound slots only, ASCENDING by `slot`
}

struct SlotArg { slot @0 :UInt16; body @1 :List(ViewNode); }
```

Canonicalization rules (required for byte determinism):

- `Component.slots` is in **declaration order** (not sorted) — it defines the
  index space.
- `ComponentRef.slots` contains only bound slots, **sorted ascending** by
  `slot`, mirroring how `lower_component_ref` already name-sorts `args`.

**Node identity.** Slot bodies are lowered once at the callsite, so their ids
derive from the *caller*: `static_node_id(caller, caller_path ++ [SLOT_TAG,
slot_idx, j])` with `SLOT_TAG = 0xF5_0000`, disjoint from the branch tags
(`(i<<8)|j`, `0xff00|j`) and from the caller's own child indices. Ids are
therefore disjoint from the callee's body ids (different first segment) and
from the caller's siblings (the tag prefix).

**Version compatibility, stated honestly.** Adding a member to an existing
capnp union is wire-legal, and an old reader still reads old programs. But an
old reader meeting a v1.5 program that *uses* `slot` gets `NotInSchema` →
`RtError::Malformed`. `SCHEMA_MAJOR` gates only the major, so this is a
**documentation contract, not an enforced gate**: *a program using `Slot`
requires a runtime at minor ≥ 5.* Everything in-tree builds together, so no
`minSchemaMinor` field is introduced; if `.nxir` is ever shipped independently
of its runtime, that decision must be revisited.

#### 7. Validator invariants (fail closed)

`userspace/dsl/ir/src/validate.rs` must, before any slot program is trusted:

- `ComponentRef.component < component_count` — **closes the pre-existing
  hole** at `:165`;
- `SlotArg.slot < components[callee].slots.len()`, strictly ascending;
- `SlotRef.slot < components[enclosing].slots.len()`;
- every `Component.slots[i]` symbol id in bounds;
- `count_view_nodes` recurses into `ComponentRef.slots[*].body` so slot bodies
  cannot escape `maxViewNodes` / `maxChildren`.

#### 8. Checker rules

Diagnostic codes are a stability contract, so the vocabulary stays small:
two new codes — `NX0209 UnknownSlot` (a slot that is not declared) and
`NX0411 SlotShape` (any structural misuse) — plus the existing
`NX0202 DuplicateDefinition` for name collisions and `NX0303 UnknownField`
for the scope rule. Every rule below is individually diagnosable by
code + span + message, and individually tested.

| # | Rule |
|---|---|
| 1 | callsite binds a slot the component does not declare |
| 2 | same slot bound twice at one callsite |
| 3 | slot block on a widget (not a component reference) |
| 4 | slot block on a component that declares no slots |
| 5 | `Slot x` with no matching `slot x` in the enclosing component |
| 6 | `Slot` inside a `Page` |
| 7 | `slot x` declared twice |
| 8 | slot name collides with a prop or state field name |
| 9 | plain children on a component reference — *closes the silent drop* |
| 10 | modifiers on a component reference — *closes the silent drop* |
| 11 | `Slot` inside a callsite slot body (forwarding) — v1 restriction |
| 12 | two `Slot x` as **siblings under the same parent** |
| 13 | `$props.<n>` in a slot body where `<n>` is not a prop of the enclosing component |

Rule 12 is deliberately narrow: `Slot x` in **both arms of an `if`** stays
legal, because the per-region panel opt-in needs exactly that shape:

```
if $props.contentPanel { Panel { Slot content }.grow(1) }
else                   { Stack { Slot content }.grow(1) }
```

(The `if` is unavoidable — modifiers take tokens, not expressions, so
`.material($props.x)` does not exist.) Duplicate placeholders are cheap
`SlotRef` leaves and bodies are lowered once, so the only real hazard is two
*simultaneously live* placeholders; same-parent-siblings is the honest,
cheap approximation of that.

### Phases / milestones (contract-level)

- **Phase 0**: ratchet unblock — four behavior-neutral file splits, proven by
  byte-identical `.nxir` for every shipped app. No contract change.
- **Phase 1**: surface grammar + checker rules 1–13; lowering rejects slots
  with an explicit error. Contract: the syntax and the diagnostics are frozen.
- **Phase 2**: IR v1.5 + lowering + validator invariants. Contract: the wire
  surface and canonical ordering are frozen.
- **Phase 3**: runtime emission — the scoping law and the splice law become
  executable and proven.
- **Phase 4**: `WinAppWindow` in `window-kit` proves the mechanism carries a
  real scaffold with per-region panel opt-in.

### Amendment: `device.sizeClass` for desktop-profile apps

Not a slot concern, but consumed by the same task and touching a documented
cross-app contract, so it is recorded here rather than in an ADR of its own:

`source/services/app-host/src/probe/env.rs::device_for` assigns
`size_class` from the real surface width **only for touch profiles**;
`PROFILE_DESKTOP` returns `FixtureEnv::desktop()`, which pins `"wide"`
(`runtime/src/fixture_env.rs:53`). `main.rs:625` does fire
`reemit_for_size_class` on a width crossing, but `device_for` hands back
`"wide"` again — so `if device.sizeClass` arms in a desktop-profile app are
**dead code**.

The fix is that the desktop arm derives `size_class` from the real width, on
the existing 640/1024 breakpoints. The design handoff's 560/820 numbers are
**not** adopted: changing platform breakpoints would touch every app and the
documented mobile-first contract (ADR-0035,
`docs/dev/ui/foundations/layout/profiles.md:51`), and a per-app `device.width`
axis would defeat the design system's tier discipline. Apps map the design's
three tiers onto the platform's three (desktop→`wide`, compact→`regular`,
mobile→`compact`).

`desktop-shell` branches on `sizeClass` and runs fullscreen at 1280 → still
`wide`, so no behavior change is expected — but that is a claim to prove, not
assume: `tests/systemui_bootstrap_shell_host` and
`tests/dsl_apps_conformance/tests/shell_*.rs` gate the change, which lands as
its own commit.

## Security considerations

- **Threat model**: a malformed or hostile `.nxir` reaching the runtime — the
  same surface `validate.rs` already defends. Slots add three new index spaces
  (`SlotRef.slot`, `SlotArg.slot`, `Component.slots[i]`) and a new place for
  nodes to hide (`SlotArg.body`).
- **Mitigations**: every index bounds-checked before use (§7); slot-body nodes
  counted against the existing traversal budgets; the pre-existing unchecked
  `ComponentRef.component` fixed in the same pass. No `unwrap`/`expect` on any
  of it.
- **Open risks**: none identified beyond the schema-minor compatibility note
  in §6, which is a build-configuration concern, not an attack surface.

## Failure model (normative)

| Condition | Required behavior |
|---|---|
| Unbound slot at emit time | **Zero** nodes. Not an error, not an empty box. |
| `SlotRef` with no enclosing slot frame (component emitted outside a ref) | Zero nodes. |
| Any out-of-bounds slot/component index in `.nxir` | `IrError::Malformed` at validation — the program never mounts. |
| Non-ascending `ComponentRef.slots` | `IrError::Malformed` (canonical-form violation ⇒ not a program this toolchain produced). |
| Slot bodies exceeding `maxViewNodes`/`maxChildren` | `IrError::Malformed`. |
| Slot forwarding | Compile error (rule 11). The runtime *also* makes it structurally impossible by clearing the frame, but the error is what the author sees. |

No silent fallback anywhere: every failure above is either an explicit compile
diagnostic or a fail-closed mount rejection.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && just check
cd /home/jenning/open-nexus-OS && just test-host
```

The proofs that would actually catch a wrong design:

1. **Caller scope** — `$props.tag` in a slot body renders the *caller's* tag,
   never the callee's (`tests/dsl_conformance`).
2. **Caller locals** — a slot body inside a `for`, with the callee containing
   its own `for` over a different list; each item renders its own. Fails
   without the locals snapshot.
3. **Splice, not wrap** — two `Panel`s in one body inside
   `Stack { Slot content }.grow(1)`; laid out at 1280×800 the panels are
   *direct* children and the region grew (`tests/dsl_goldens`).
4. **Handler paths** — `on Tap -> dispatch(E)` inside a slot body; hit-test
   resolves and the dispatch fires (`userspace/dsl/runtime/tests/slots.rs`).
5. **Byte determinism + golden IR** — `compile(src) == compile(src)`, and
   `slots.nxir` matches (`tests/dsl_v0_1a_host`).
6. **Unbound slot = zero nodes** — assert the region's child count.
7. **Node-id disjointness** — slot-body ids ≠ callee-body ids ≠ caller-sibling
   ids (unit test over `static_node_id`).

Plus `test_reject_*` for all 13 checker rules and the five validator
invariants (`tests/dsl_conformance`, `userspace/dsl/ir`).

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just test-all
cd /home/jenning/open-nexus-OS && just start   # visual: Stash in the design window
```

### Deterministic markers

None new. Slots are a compile/emit concern; they change no boot marker.

## Alternatives considered

- **`slots: { a, b, c }` block** (parallel to `props:`/`state:`). More
  grammar-consistent and has strictly zero contextual-keyword risk, but
  `slot <name>` lines leave room for per-slot attributes later
  (`slot content required`, `slot rows: List<T>`) and read better at the
  declaration site. Rejected on ergonomics, not correctness.
- **Slot entries inside the single prop block** (`slot sidebar { … }` mixed
  with props). Also unambiguous, but muddles configuration and content in one
  brace and forces the reader to scan. Rejected.
- **Inline component expansion at lowering** (macro-style), which would make
  children "just work" without an IR change. Rejected: it duplicates the
  callee body per callsite (IR size, `maxViewNodes`), destroys the
  component-index identity the runtime and node-ids depend on, and would break
  the one-implicit-store-per-stateful-component rule.
- **Slot forwarding in v1.** Requires a frame *stack* rather than a frame,
  turns "which caller's scope" into a chain rather than a pair, and roughly
  doubles the emit test matrix. Nothing in `window-kit` or the design handoff
  needs it. The seam is `SlotFrame { parent: Option<&SlotFrame> }` — named
  here so v2 does not have to rediscover it.
- **Required slots.** Rejected for v1: "zero nodes for an unbound slot" is
  exactly what hidden regions want, and requiredness is better expressed by
  the app. `slot content required` is the named extension point.
- **Adopting the design's 560/820 breakpoints.** See the amendment above.

## Open questions

- Whether modifiers on a component reference (rule 10) should stay an error or
  eventually be *wired* onto the component's root node. Erroring is the
  smaller and more honest change; wiring means deciding which node they land
  on. Owner: @ui, revisit when a second scaffold component asks for it.

### Resolved during implementation

- **`.width($props.n)` does not work — and fails silently.**
  `runtime/src/emit/modifiers.rs::px_arg` reads only `TokenArg::Int`, so an
  expression argument yields `None` and the modifier is a no-op. The checker
  does not object either: `width` has no closed token vocabulary, so
  `check_token_vocabulary` skips it. `WinAppWindow` therefore pins its region
  widths as constants (sidebar 240, properties 260).

  This is a defect that predates slots — `Text("x").width($state.w)` has
  always compiled and always done nothing — and it is the same class as rules
  9 and 10. Fixing it means deciding the semantics of dynamic sizing (which
  damage class a size dep carries, whether `.width` may depend on state at
  all), which is a separate contract. Recorded as a follow-up, not smuggled in
  here.

- **The `actionBar` slot was dropped.** The plan sketched a fourth slot so the
  bar's bottom-centre position would belong to the scaffold. It is not needed:
  an app puts its action bar at the end of its `content` body after a
  `Spacer`, which is what `StashPage.nx` already does and what boots today.
  A fourth slot would have added an overlay layer and its hit-testing for no
  behavior an app cannot express itself. Three slots, one job each.

---

## Implementation Checklist

- [ ] **Phase 0**: four behavior-neutral splits — proof: `just check` green +
      byte-identical `.nxir` for every shipped app
- [ ] **Phase 1**: grammar + 13 checker rules — proof:
      `cargo test -p dsl-conformance` (`test_reject_*`) + format idempotence
- [ ] **Phase 2**: IR v1.5 + lowering + validator — proof: golden `slots.nxir`,
      byte determinism, five fail-closed validator tests
- [ ] **Phase 3**: runtime emit — proof: the seven tests above
- [ ] **Phase 4**: `WinAppWindow` — proof:
      `tests/dsl_apps_conformance/tests/stash.rs` region-configuration test
- [ ] Task linked with stop conditions + proof commands
      (`tasks/TASK-0308-*`)
- [ ] QEMU markers: none new (recorded as intentional)
- [ ] Security-relevant negative tests exist (`test_reject_*` for all 13
      checker rules + 5 validator invariants)
