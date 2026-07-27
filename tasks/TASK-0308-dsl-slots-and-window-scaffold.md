---
title: TASK-0308 DSL slots + WinAppWindow scaffold + Stash design parity
status: In progress (2026-07-27) — Phase 0 running
owner: @ui @runtime
created: 2026-07-27
links:
  - RFC: docs/rfcs/RFC-0084-dsl-slots-component-content-regions.md
  - Design: userspace/apps/stash/docs/design_handoff_files_window/
  - Predecessor: tasks/TASK-0307-settings-distribution-v2.md
  - Related: tasks/TASK-0291-vfs-readdir-svc-files-stash-real-listing.md
---

## Context

The window design handoff at
`userspace/apps/stash/docs/design_handoff_files_window/` specifies a
slot-based `AppWindow` scaffold (nine slots, three-zone body, responsive
overlays). Stash must look exactly like it.

Two things block that:

1. **The panel background must be configurable per region.** The handoff
   hard-codes a panel surface behind content and properties
   (`reference/components/AppWindow.jsx:168/185`) and denies one to the
   sidebar (`Sidebar.jsx:61-64`). Open Nexus wants the developer to decide —
   zero, one, or several stacked panels per region (a settings pane), and the
   sidebar must be able to have one.
2. **The DSL has no children/slots for components.** `ComponentDecl`
   (`userspace/dsl/core/src/ast.rs:363`) takes scalar props only, so
   `window-kit` has five leaf components and no scaffold — `StashPage.nx`
   (512 LOC) and `SettingsPage.nx` (416 LOC) hand-write the same skeleton.

RFC-0084 defines the language contract. This task executes it and rebuilds
Stash on top.

## Goal

`.nx` components take named content slots; `window-kit` ships a
`WinAppWindow` whose regions are transparent by default with panel as a
per-region opt-in; Stash reaches full parity with the design handoff
including the settings mode and responsive overlays.

## Non-Goals

Slot forwarding · typed/required/default slots · per-instance component state
(all RFC-0084 non-goals) · migrating `SettingsPage.nx` (separate follow-up —
it is the second consumer that proves the scaffold generalises).

## Constraints / invariants

- Every phase lands alone green: `just check` + `just test-host`.
- **Phase 0 must prove byte-identical `.nxir` for every shipped app** — the
  splits touch the toolchain's densest files, and without that proof the slot
  work is not bisectable.
- Zero warnings under host + os + kernel cfgs. No blanket `#[allow]`.
- No fake parity: items the platform cannot express are documented as
  knowingly unmet (see Known non-parity below), never approximated into a
  claim.

## Verified blockers (found during planning, not negotiable)

**LOC ratchet.** These sit exactly on their `config/loc-baseline.txt` entry
and may not grow by a line (`scripts/check-structure.sh`):

| File | LOC / baseline |
|---|---|
| `userspace/dsl/core/src/check/names.rs` | 620 / 620 |
| `userspace/dsl/core/src/fmt.rs` | 760 / 760 |
| `userspace/dsl/core/src/lower/mod.rs` | 751 / 751 |
| `userspace/dsl/core/src/lower/views.rs` | 809 / 809 |
| `userspace/dsl/runtime/tests/layout_viewport.rs` | 1005 / 1005 |

`userspace/dsl/runtime/src/emit.rs` is 458 and below the 600 scan threshold —
~142 lines of headroom before the gate engages.

**Two pre-existing fail-open holes on this path**, closed here:

- `lower_component_ref` (`lower/views.rs:761-783`) silently drops
  `widget.children` **and** `widget.modifiers`.
- `ComponentRef.component` is never bounds-checked
  (`ir/src/validate.rs:165` no-op; `runtime/src/emit.rs:186` unguarded `get`).

**`ViewNode.nodeId` is never read** (`grep get_node_id` = empty) — it exists
for goldens/AOT. Emit-time correctness rides entirely on `ctx.path` →
`interact::path_to_box_id`.

**`device.sizeClass` is dead for desktop-profile apps**
(`source/services/app-host/src/probe/env.rs:97-108` pins `"wide"`) —
responsive arms would be dead code without the Phase 7 fix.

## Phases (stop conditions + proofs)

### Phase 0 — RFC seed + ratchet unblock

RFC-0084 + index entry + this ledger. Then four **behavior-neutral** splits:

- `check/names.rs` → `check/queries.rs` (query checks, ~125 lines)
- `lower/views.rs` → `lower/effects.rs` (`lower_effect_steps` + `fill_call` +
  `fill_dispatch`, ~175 lines)
- `lower/mod.rs` → `lower/symbols.rs` (`collect_symbols`, ~285 lines)
- `fmt.rs` → `fmt/expr.rs` (expression printer, ~250 lines)

A fifth split fell out of the same pass: `lower/views.rs` still sat at 630
after `effects.rs`, so the state tables (`build_state` +
`resolve_reducer_store`) moved to `lower/state.rs` and took it to 392 —
off the >600 scan list entirely, with real headroom for the slot work.

**Stop condition**: `just check` green (structure gate satisfied), and every
shipped app's `.nxir` byte-identical to before the splits.

```bash
just check && just test-host
```

**Result (2026-07-27): green.** `just check` passes (fmt · clippy · deny ·
arch · structure-gate), `just test-host` 611 suites ok / 0 failures.

| File | before | after |
|---|---|---|
| `check/names.rs` | 620 | 496 (+ `check/queries.rs` 138) |
| `fmt.rs` | 760 | `fmt/mod.rs` 557 (+ `fmt/expr.rs` 216) |
| `lower/mod.rs` | 751 | 421 (+ `lower/symbols.rs` 353) |
| `lower/views.rs` | 809 | 392 (+ `effects.rs` 196, `state.rs` 252) |

Byte-neutrality proof — FNV-1a over each app's canonical `.nxir`, identical
before and after all five splits:

```
calculator     len=13016   fnv=9eb3d9086bb3c317
chat           len=17200   fnv=07221b334c24f83d
desktop-shell  len=114024  fnv=9e9c5e48bff84533
greeter        len=15792   fnv=06982470a321e154
ime-ui         len=20632   fnv=0c312bd4a628da90
settings       len=84536   fnv=995b0b9c0914e28f
stash          len=102960  fnv=ccca7de40f9311a1
```

(`window-kit` is a library bundle — no store, so it does not compile
standalone; it is exercised through `settings` and `stash`.)

`config/loc-baseline.txt` was regenerated with the sanctioned
`scripts/check-structure.sh --update-loc-baseline`. Besides dropping the four
split files it TIGHTENED 25 stale entries (files that had shrunk since the
last regeneration) and dropped `dsl/runtime/src/emit.rs` + `registry.rs` +
`dsl/core/src/project.rs`, which are under the 600 limit and were therefore
dead entries — the scanner only ever compares files above the limit.

### Phase 1 — syntax → AST → checker

`ast.rs` (`ComponentDecl.slots`, `WidgetNode.slot_bodies`, `ViewNode::Slot`,
`SlotBinding`) · `parser/decls.rs` slot-declaration loop · `parser/view.rs`
`Slot` guard + `slot_block()` in `widget_like` · new `check/slots.rs` with
RFC-0084 §8 rules 1–13 · `check/names.rs` recursion into slot bodies ·
formatter. Lowering rejects slots with `LoweringUnsupported`.

**Stop condition**: all 13 rules have a reject test; `format_file` is
idempotent over the slot syntax.

```bash
cargo test -p dsl_v0_1a_host --test slots
just check && just test-host
```

**Result (2026-07-27): green.** 21 tests in
`tests/dsl_v0_1a_host/tests/slots.rs`; `just check` passes; `just test-host`
611 suites ok / 0 failures.

Deviations from the plan, both deliberate:

- **Two diagnostic codes, not thirteen.** Codes are a stability contract and
  CLAUDE.md keeps such vocabularies deliberately small, so the rules map onto
  `NX0209 UnknownSlot` and `NX0411 SlotShape` plus the existing
  `NX0202 DuplicateDefinition` / `NX0303 UnknownField`. Each rule still has
  its own test and its own message.
- **A sixth split.** The two new `explain` entries pushed
  `userspace/dsl/cli/src/main.rs` from 644 to 646 against a 644 baseline, so
  `cmd_explain` moved to `cli/src/explain.rs` (main.rs → 593).

Rules 9 and 10 (children / modifiers on a component reference) are the two
pre-existing SILENT drops turned into diagnostics. Verified against every
shipped app first — `dsl_apps_conformance` stays 8/8, so no app was relying
on the old silence.

`Slot` reads as a placeholder only before a BARE identifier; `Slot Text("x")`
still parses as two sibling widgets, which is what keeps the contextual
keyword safe.

### Phase 2 — IR v1.5 + lowering + validator

`ui_ir.capnp` per RFC-0084 §6 · `SCHEMA_MINOR = 5` · `Ctx.component_slots` ·
`build_components` writes `Component.slots` · `lower_view` `Slot` arm ·
`lower_component_ref` lowers bodies **in the caller's `Env`** with the
`SLOT_TAG` path prefix · `collect_symbols` + `count_component_usage` walk slot
bodies (else a stateful component in a slot body bypasses the
one-instance guard) · `validate.rs` bounds + budget recursion + the
`ComponentRef.component` fix.

**Stop condition**: golden `slots.nxir` + byte determinism; five fail-closed
validator tests (OOB `SlotRef.slot`, OOB `SlotArg.slot`, OOB
`ComponentRef.component`, non-ascending `slots`, body blowing `maxViewNodes`);
node-id disjointness.

### Phase 3 — runtime emit

`EmitCtx<'a, 'p>` (two lifetimes so the design does not depend on capnp reader
covariance) · new `runtime/src/emit/slots.rs` with `SlotFrame` +
`emit_slot_items` · ComponentRef arm pushes the frame with a caller-locals
snapshot · splice in the `emit_widget` child loop **before** the generic path
· standalone `Which::Slot` arm following the branch precedent.

**Stop condition**: the seven RFC-0084 proofs, in
`userspace/dsl/runtime/tests/slots.rs` (new file — `layout_viewport.rs` is at
its baseline).

### Phase 4 — `WinAppWindow` + window-kit chrome

New `userspace/apps/window-kit/ui/components/WinAppWindow.nx` (slots
`sidebar`/`content`/`properties`/`actionBar`; props
`sidebarPanel`/`contentPanel`/`propsPanel: Bool`, `propsTitle: Str`,
`showSidebar`/`showProps: Bool`) · `WinActionItem.act: Bool` (mirrors
`WinMenuItem`; `false` = no handler, the app wraps and owns the tap) ·
`WinTopBar` `tool*Active` + dividers + app-chip chevron · `WinPropRow` on the
design metrics.

**Stop condition**: new `tests/dsl_apps_conformance/tests/stash.rs` with
`stash_compiles_and_mounts` (missing today) + a region-configuration test
asserting zero/one/many panels per region.

### Phase 5 — Stash rebuild, structural parity

`StashPage.nx` onto `WinAppWindow`; the ~120 inlined action-bar lines
(`:144-262`) deleted; 3-level breadcrumb; count + three sort buttons in the
content header; search-field row; settings mode; `FileRow`/`FileTile`
metrics; store fields/events for search/settings/hidden/sortDir.

**Stop condition**: interaction tests (selection → properties populate · tool
tap → view mode · ⋯ → Einstellungen → content swaps, pane + bar hidden, back
arrow returns · hidden toggle → count changes). Visual: `just start`.

### Phase 6 — search filter (platform)

`svc.files.list` gains a query parameter; app-host applies the substring
filter the same way it already applies `sortBy`; store effect fires on
`SearchSet`.

**Stop condition**: host test over the filter, then counter +
"Keine Objekte gefunden" proven in `stash.rs`.

### Phase 7 — responsive + `probe/env.rs`

Tier mapping (desktop→`wide`, compact→`regular`, mobile→`compact`, platform
breakpoints 640/1024) in `WinAppWindow` + `StashPage.nx`; overlay panes via
`.overlay()` + scrim with a backdrop `on Tap` — the pattern the menu overlay
already uses (`StashPage.nx:301-383`). **Then** the `env.rs` desktop-arm
change as its own commit.

**Stop condition**: host tests at three `FixtureEnv` size classes;
`tests/systemui_bootstrap_shell_host` + `shell_*.rs` green; `just test-all`;
QEMU resize confirmation.

### Phase 8 — docs + tooling parity

`docs/dev/dsl/{grammar,ir}.md` (EBNF + changelog) ·
`tools/tree-sitter-nx/grammar.js` (+ `verify.sh`) ·
`/home/jenning/nx-dsl-vscode` (`syntaxes/nx.tmLanguage.json`,
`src/keywords.ts`, snippets) · `CHANGELOG.md` · RFC status → Complete.

## Known non-parity (documented, not approximated)

- **Monospace file sizes.** No `fontFamily` modifier and no mono face baked;
  the design's `--font-mono` column is not expressible.
- **9px action-bar label.** The smallest type-ramp step is `xs`.
- **`primary` inset top-shine** on action-bar buttons — RFC-0082 has the
  primitive seam but no per-button application.
- **560/820 breakpoints.** Platform tiers are 640/1024; adopting the design
  numbers would touch every app and the documented mobile-first contract
  (RFC-0084 amendment).

## Risks

- **capnp reader lifetimes** (high): if the two-lifetime `EmitCtx` fights the
  borrow checker in `emit/modifiers.rs`, fall back to an owned index handle
  instead of readers. Spike before committing Phase 3.
- **The four splits** (high): must be provably behavior-neutral before any
  slot code lands.
- **`.width($props.n)` with an `Int` prop** (medium): lowering produces
  `TokenArg.expr`; whether `apply_modifier` resolves expr args for sizing
  modifiers is unverified. Fall back to constant 240/260 in v1 if not.
- **`env.rs` change** (medium): the only platform dependency in the Stash
  work; gated by the shell regression suites.

## Proof commands

```bash
just check        # fmt + clippy + deny + arch + structure-gate
just test-host    # dsl_conformance / dsl_goldens / dsl_apps_conformance / runtime
just test-all     # + miri + kernel + QEMU SMP
just start        # visual: Stash in the design window
```
