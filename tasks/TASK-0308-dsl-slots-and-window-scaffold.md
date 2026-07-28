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

**Stop condition**: golden + byte determinism; fail-closed validator tests
(OOB `SlotRef.slot`, OOB `SlotArg.slot`, OOB `ComponentRef.component`,
non-ascending `slots`, body over budget); node-id disjointness.

**Result (2026-07-27): green.** 25 tests in
`tests/dsl_v0_1a_host/tests/slots.rs` (the six new ones cover declaration-order
`Component.slots`, ascending `ComponentRef.slots`, unbound slots absent from
the wire, byte determinism, node-id disjointness, validator acceptance) plus
6 fail-closed tests in `userspace/dsl/ir/tests/validate_slots.rs`, which build
tampered `.nxir` messages by hand — the compiler cannot produce them, which is
the point.

The `proof_surface.nxir` golden was regenerated. It moved by **exactly 33
bytes**: offset 10 (`schemaVersionMinor` 4 → 5) and the 32-byte `programHash`.
Same length, no structural drift — an empty `Component.slots` costs nothing in
canonical form, which is the additive guarantee actually holding rather than
being assumed. Recorded in `docs/dev/dsl/ir.md#changelog` under v1.5.

### Phase 3 — runtime emit

`EmitCtx<'a, 'p>` (two lifetimes so the design does not depend on capnp reader
covariance) · new `runtime/src/emit/slots.rs` with `SlotFrame` +
`emit_slot_items` · ComponentRef arm pushes the frame with a caller-locals
snapshot · splice in the `emit_widget` child loop **before** the generic path
· standalone `Which::Slot` arm following the branch precedent.

**Stop condition**: the seven RFC-0084 proofs, in
`userspace/dsl/runtime/tests/slots.rs` (new file — `layout_viewport.rs` is at
its baseline).

**Result (2026-07-27): green.** 7/7.

The capnp-lifetime risk did not materialise: `EmitCtx<'a, 'p>` compiled
without a fight, so the owned-index-handle fallback was never needed.

The two proofs that would have caught a wrong design both do their job:

- *caller props* — two components each declaring a prop `tag`; the body renders
  the CALLER's, never the callee's;
- *caller locals* — a slot body inside a `for`, with the callee running its own
  `for` over a different list before reaching the placeholder. This is the one
  that fails without the locals snapshot, because `locals` are shared across
  the component boundary and the callee's loop clobbers the caller's binding
  before the body is emitted.

One walker needed slot bodies that the plan did not name:
`runtime/src/initial.rs::collect_handler_dispatches`. Slot bodies live only
inside the `ComponentRef`, so nothing else visits them — a handler in a body
dispatching an event would otherwise look like a ROOT effect and fire at mount.

### Phase 4 — `WinAppWindow` + window-kit chrome

New `userspace/apps/window-kit/ui/components/WinAppWindow.nx` (slots
`sidebar`/`content`/`properties`/`actionBar`; props
`sidebarPanel`/`contentPanel`/`propsPanel: Bool`, `propsTitle: Str`,
`showSidebar`/`showProps: Bool`) · `WinActionItem.act: Bool` (mirrors
`WinMenuItem`; `false` = no handler, the app wraps and owns the tap) ·
`WinTopBar` `tool*Active` + dividers + app-chip chevron · `WinPropRow` on the
design metrics.

**Stop condition**: a region-configuration test asserting zero/one/many panels
per region.

**Result (2026-07-27): the scaffold is green** —
`userspace/apps/window-kit/ui/components/WinAppWindow.nx` + 6 tests in
`tests/dsl_apps_conformance/tests/window_kit.rs`. The remaining Phase-4 items
(`WinActionItem.act`, `WinTopBar` chrome, `WinPropRow` metrics) ride with the
Stash rebuild in Phase 5, where their effect is visible.

Three regions, not four: the `actionBar` slot was dropped (see the RFC's
resolved-questions section — an app puts its bar at the end of its `content`
body after a `Spacer`, which is what boots today).

What the tests actually pin:

- **every region is transparent by default** — glass count 0 with all flags
  off. This is the ask, stated as an assertion.
- **panels are opt-in per region** — each flag alone paints exactly one panel;
  all three paint three.
- **a region takes several stacked panels** — `contentPanel: false` plus three
  `Panel`s in the body = three, the settings case.
- **flipping a panel on keeps the region rects** — `(0,0,240,800)`,
  `(240,0,780,800)`, `(1020,0,260,800)` in both configurations.

That last test was initially written to assert that NOTHING moves, and it
failed — correctly. A `Panel` brings its own padding, so content inside a
panelled region is inset by 12px more, and the panel adds its own box. The
honest contract is the one now asserted: the region rects are identical, the
content inside a panel is inset by the panel. The over-strong version would
have been a false claim.

**Region widths are constants (240 / 260), not props.**
`.width($props.n)` is a SILENT no-op: `runtime/src/emit/modifiers.rs::px_arg`
reads only `TokenArg::Int`, so an expression argument resolves to `None`, and
the checker does not object because `width` has no closed token vocabulary.
That defect predates slots (`Text("x").width($state.w)` has always compiled and
always done nothing) and fixing it means deciding the semantics of dynamic
sizing. Recorded as a follow-up rather than smuggled into this task.

### Phase 5 — Stash rebuild, structural parity

`StashPage.nx` onto `WinAppWindow`; the ~120 inlined action-bar lines
(`:144-262`) deleted; 3-level breadcrumb; count + three sort buttons in the
content header; search-field row; settings mode; `FileRow`/`FileTile`
metrics; store fields/events for search/settings/hidden/sortDir.

**Stop condition**: interaction tests (selection → properties populate · tool
tap → view mode · ⋯ → Einstellungen → content swaps, pane + bar hidden, back
arrow returns). Visual: `just start`.

**Result (2026-07-27): host-green.** `just check` passes, `just test-host`
615 suites / 0 failures (up from 613: `stash.rs` + `window_kit.rs`). 7 tests in
`tests/dsl_apps_conformance/tests/stash.rs` — the app had NO conformance test
before this, it only compiled as a side effect of the boot lane.

`StashPage.nx` now states its panel choice once, at the callsite:

```
WinAppWindow {
    sidebarPanel: false, contentPanel: true, propsPanel: true,
    showSidebar: $state.leftOpen == 1,
    showProps: $state.rightOpen == 1 && $state.settingsOpen == 0,
} { sidebar { … } content { … } properties { … } }
```

Settings mode drops the properties pane through `showProps` on the scaffold —
the pane is *gone*, not covered, which the test asserts by absence of the
"Properties" title rather than by z-order.

**The ~120 inlined action-bar lines are gone**, but not via the `act: Bool`
prop the plan sketched. `WinMenuItem` gets its two dispatch targets by
duplicating its entire body across the `act` arms; copying that would have
duplicated the action button's look too. Instead the look moved into a new
`WinActionFace` (pure visual, no handler) and `WinActionItem` became a thin
`WinAct(id)` wrapper around it. An app button dispatching its own event wraps
the face directly:

```
Stack { WinActionFace { label: …, icon: "plus", kind: "primary" } }
on Tap -> dispatch(NewFolder)
```

One body, no new prop, no contract change — and it works because a dispatch
target is a static case name that cannot be passed as a prop, so the honest
split is "kit owns the look, app owns the tap".

Other deltas closed: `WinTopBar` `tool*Active` + toolbar dividers + app-chip
chevron · search field with `TextField` auto-bind + ✕ · 3-level breadcrumb
with the leaf in `onGlassStrong`/semibold · object count + three sort controls
in the content header (sort left the action bar, per the design) · `FileRow`
fixed 80/68px columns + per-row hairline + 30px icon · `FileTile` on
`minWidth(124).grow(1)` with a raised bordered surface · `WinPropRow` at 50px
with the value in `onGlassStrong` · action bar as a `rounded(full)` pill with
a divider before "Mehr" · settings mode with the uppercase section header and
a `Toggle` card. New i18n keys in all four locales.

Two traps worth recording:

- `.textAlign(trailing)` does not exist — the vocabulary is `left|center|right`
  (`registry.rs:440`). The checker catches it, but only because `textAlign`
  HAS a closed vocabulary; `width` does not, which is why `.width($props.n)`
  fails silently (Phase 4).
- The conformance harness resolved every `@t(…)` to `""` until the program's
  own i18n KEY table was fed to `IdentityLocale` alongside the symbol table.
  `common::program_i18n_keys` now does that; without it a scene full of real
  labels reads back as a list of blanks and every text assertion is vacuous.
  The baked default locale is **English**, so the tests assert English.

### Phase 6 — search filter (platform)

`svc.files.list` gains a query parameter; app-host applies the substring
filter the same way it already applies `sortBy`; store effect fires on
`SearchSet`.

**Stop condition**: host test over the filter, then counter +
"Keine Objekte gefunden" proven in `stash.rs`.

### Phase 7 — responsive + `probe/env.rs` ✅ (2026-07-28)

`WinAppWindow` is tier-aware via `device.sizeClass` (breakpoints 640/1024):
wide keeps all three zones inline; regular moves properties into an OVERLAY
pane; compact moves the sidebar too — content alone owns the width. Overlay
panes ride the proven menu-overlay pattern (`.overlay()`, backdrop tap →
`WinPaneClose`, pane eats taps via `WinNoop`) and always float on a Panel
(the per-region panel choice governs the inline arms only). A slot may
legally appear in BOTH the inline and the overlay arm — rule 12 only
refuses two placeholders under the same parent, and the arms are exclusive.

Caller side: two new props (`sidebarOverlay`/`propsOverlay`) driven by new
`paneL`/`paneR` state in stash.store (default CLOSED — a phone mount must
not boot with a pane over the content); `WinLeft`/`WinRight` toggle both
the inline and the overlay sense, only the visible one matters per class;
`WinPaneClose` is the new kit-convention event (settings.store carries the
no-op arm since it mounts kit chrome).

`env.rs` (own commit): the desktop arm no longer pins `"wide"` — EVERY
profile derives `device.sizeClass` from the real surface width, so
responsive arms stop being dead code for desktop-profile apps (a narrow
desktop window takes the regular/compact tiers).

**Proof**: window_kit conformance grew two tier tests
(`responsive_tiers_move_panes_out_of_flow`,
`overlay_flags_bring_the_panes_back_as_panels`) exercising all THREE
`FixtureEnv` size classes; full conformance 8+5+2+1 green;
`systemui_bootstrap_shell_host` 7/7; `just check` green; test-host
615/615; headless ladder exit 0 incl. the gated `SELFTEST: ui resize ok`.
Kernel lanes unchanged this phase (proven green earlier the same day).

### Phase 8 — docs + tooling parity

`docs/dev/dsl/{grammar,ir}.md` (EBNF + changelog) ·
`tools/tree-sitter-nx/grammar.js` (+ `verify.sh`) ·
`/home/jenning/nx-dsl-vscode` (`syntaxes/nx.tmLanguage.json`,
`src/keywords.ts`, snippets) · `CHANGELOG.md` · RFC status → Complete.

## BLOCKER: the boot lane is red — gpud framebuffer VMO map fails

`just test-os` (headless, proof) fails on this tree and does NOT on
`2469cdd6` (Phase 1). Phase 5 is host-proven, **not boot-proven**. Do not
claim otherwise until this is resolved.

### What actually happens

```
baseline (2469cdd6)          this tree
─────────────────────────    ───────────────────────────────────────
gpud: recv OP_SET_FRAMEBUFFER_VMO   gpud: recv OP_SET_FRAMEBUFFER_VMO
execd: atlas vmo ready              gpud: resource vmo_map_page fail
gpud: gpu irq wake                  gpud: ERROR attach framebuffer failed
gpud: set_scanout ok                windowd: handoff attach ack bad status
…                                   → 13× `gpud: chain G4 scanout FAIL`
gpud: chain G4 scanout ok           → chain-marker contract 1/9 missing
```

In the baseline `execd: atlas vmo ready` lands BETWEEN gpud receiving the op
and gpud mapping the page. Here gpud maps immediately and the map fails. It is
an ORDERING hazard between gpud's framebuffer VMO and execd's shared atlas VMO
(RFC-0080), not exhaustion: the kernel pool and arena bounds are identical in
both runs (`pool=0x82000000 arena_end=0x91800000`); only `image_end` moved by
24 KB (`0x81adfb40` → `0x81ae5b40`, headroom 5249K → 5225K, i.e. still 5 MB
free).

Deterministic: 3 of 3 consecutive runs, all `G4=0`.

### What is mine and already fixed

`emit.rs` built the slot `SlotFrame` — including `ctx.locals.to_vec()`, a deep
clone of 64 `Option<Value>` — at **every** component reference in **every**
emit, whether or not the reference bound a slot. On a hot path in a service
whose bump allocator never frees. Now built only when `ComponentRef.slots` is
non-empty, so every slot-free reference (all of desktop-shell) pays nothing.

### What is NOT mine

The ordering hazard itself. gpud, execd and the kernel VMO arena carry no
dependency on any DSL crate; this task changed no file under `source/`. The
24 KB of image growth (window-kit compiles into every consumer, so `settings`
grew 84536 → 91968 and `stash` 102960 → 121992 bytes) only shifts the
interleaving that decides who reaches the arena first.

Seeded as `tasks/TASK-0309-gpud-framebuffer-vmo-map-ordering-hazard.md`: it is
a bringup contract across `source/drivers/gpud` + `source/services/execd` + the
kernel arena, outside RFC-0084's scope, and the fix is an allocation-order
decision — not something to patch blind from here. The VA allocator documents
the hazard itself (`gpud/src/backend/resources.rs:37`: monotonic slots, no
unmap primitive, "remap refused"), and the failing call
(`attach.rs:73`) currently discards the kernel's error code, so the first move
is to make it say what it hit.

### Also observed, pre-existing, NOT caused here

- `SELFTEST: metrics {security rejects, counters, gauges, histograms} FAIL` and
  `SELFTEST: tracing spans FAIL` appear IDENTICALLY in the baseline run and are
  tolerated by the lane.
- `SELFTEST: ime v2 candidates` is genuinely flaky: it flipped FAIL → ok
  between two runs of the same tree.

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
