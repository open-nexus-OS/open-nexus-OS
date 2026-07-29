---
title: TASK-0308 DSL slots + WinAppWindow scaffold + Stash design parity
status: In progress (2026-07-28) — Phases 0-7 + 9 done; Phase 8 (docs/tooling parity) open
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

### Phase 9 — design parity: colours, materials, icons, hover ✅ (2026-07-28)

Phase 5 reached STRUCTURAL parity; this phase closes the visual gap. Almost
none of it was in Stash — five defects that share one shape: a thing was
authored, and nothing above it could tell that it never arrived.

**Theme (`resources/themes/*`, `ui/theme-tokens`, `dsl/{core,runtime}`)**

- Six roles (`divider`, `glassHover`, `glassActive`, `toggleOnBg`,
  `toggleOffBg`, `notifDot`) were authored in every theme with no `ColorToken`
  and therefore unsayable from `.nx`. Hairlines used `border` — dark `#262626`,
  OPAQUE — where the handoff wants `rgba(255,255,255,.10)`. The gate was
  `theme-tokens/build.rs::ROLES`, not the TOML.
- New levels `glassWindowPane` (dark `#484a54@.48 → #34363e@.32`) and
  `glassWindowBar`. Panes had been inheriting `glassPanel` (`#121214@.40`) —
  the level tuned for a tile on the WALLPAPER, not a pane on window glass.
  **No wire change**: `glass_level` only selects a blur radius (windowd), while
  tint/shine/hairline are painted app-side, so the two levels ride the existing
  card/panel buckets.
- On-glass alphas pulled to the handoff (`.80/.40/.92`, dark `.90/.45/.95`);
  the previous `.95/.68/1.0` was undocumented drift that flattened the
  primary/secondary step. Light `divider` was opaque `#d4d4d4` → `rgba(0,0,0,.10)`.
- **Proof**: `dsl/runtime/tests/token_vocabulary_lockstep.rs` asserts the
  checker's `COLOR_TOKENS` and the runtime's `color_token` are a BIJECTION —
  both drift directions fail, and `canonical_name` refuses to compile when a
  role is added without a name.

**Icons (`resources/themes/base.nxtheme.toml`)**

`square.grid.2x2`, `arrow.clockwise`, `arrow.uturn.backward` and `calendar`
were used without an `[icons.symbols]` entry and painted the honest grey
placeholder box. A symbol name travels as a prop STRING, so the checker never
sees it. Added those four plus `ellipsis.vertical` (the toolbar ⋯ is a
different glyph from the action bar's), `doc.text`, `shippingbox`,
`square.pencil`. **Proof**: `tests/dsl_apps_conformance/tests/icon_symbols.rs`
scans every shipped `.nx` and reports file, line and symbol; verified to FAIL
with a mapping removed.

**Hover (`app-host`)**

`probe/paint.rs` passed a hard `None` for the `HoverWash`. The stated reason
was real — the wash follows the HANDLER box's radius, and the kit's documented
`Stack { WinActionFace { … } } on Tap` shape has none, so "every hovered circle
wore a white square". Fixed by aiming it, not deleting it:

- `app-host/src/hover_wash.rs` (new, host-testable because `probe/` is
  RISC-V-only) picks the child that actually fills the wrapper.
- The WASH's size rule split from the GROW's. Sharing `interaction_sized`
  (≤160px both axes) meant a 680-px file row was excluded from BOTH — list rows
  and sidebar entries had no hover at all. Growing a full-width row would
  displace its content; washing one is what the design asks for.
- Colour from `glassHover` (was an Accent tint, which read as selection).

**Window geometry (`app-host`, `windowd`, `abilitymgr`)**

Stash declares `mode: freeform` — the ONLY way to a translucent, blurred window
(`fullscreen` forces `base_alpha = 255` and skips the blur band). Three latent
defects surfaced on that path:

- app-host mounted freeform windows at the 320x240 probe fallback; it now asks
  the compositor, like desktop/overlay/fullscreen already did.
- windowd's intent reply for a non-fullscreen window returned the next free
  SLOT's frame, which for an unframed slot is the allocator CEILING
  (1280x3072). It now returns the same centred ¾ frame `set_window_mode`
  applies. Nothing caught it because nobody asked on that path before.
- A cascaded origin is clamped onto the work area
  (`surface_presentation::clamp_frame_origin`, host-tested): the second
  window's 960-wide frame at cascade x=364 hung 44px off a 1280 display and
  took the properties pane with it.
- **App stacks 8 → 16 pages.** `app-host` recurses over its own scene (lower →
  layout → paint), so depth scales with the page a designer wrote. Stash
  overflowed 8 pages by FORTY-EIGHT bytes on a six-row listing — a
  `[USER-PF] STORE` register dump with no hint that it was the stack. A
  guard-page marker that says "stack" is a recorded follow-up.

**Stash + window-kit**

- `WinAppWindow` lost `sidebarPanel`/`contentPanel`/`propsPanel` (7 → 4 props,
  169 → 128 LOC). Regions are transparent, FULL STOP; a region that wants a
  surface gets one because the app wrote `Panel { … }` in the slot body. That
  deletes six duplicated `if` arms, makes "several panels in one region" the
  ordinary case instead of a flag trick, and lets a panel pick its own LEVEL —
  which is what `windowPane` needed. Slots renamed `sidebarLeft` /
  `contentArea` / `sidebarRight` (structural, not semantic).
  **Consequence, asserted not hidden**: the kit no longer wraps overlay panes
  either, so a region without a panel floats unframed.
- An `@effect on SearchToggle` for an event NOTHING dispatches made it a ROOT
  effect — and `run_initial_effects` dispatches roots through the REDUCER, so
  Stash opened its search field on every launch. The effect moved to `WinTool`,
  which is what the toolbar actually emits (and which had no effect at all, so
  closing the search never restored the unfiltered listing). The old test fired
  the orphan directly and therefore could never fail; it now fires
  `WinTool("magnifyingglass")`, plus a new test asserts the initial effects do
  not change what the user sees.
- Design details: 3-level breadcrumb (from the existing `backLabel`, no new
  state), the object COUNT before the noun, `stash.noResults` finally rendered
  (the key shipped in five locales referenced by nothing), `Avatar` in the
  sidebar footer, the ⋯ dropdown anchored UNDER its button instead of at y=0,
  the search field as a raised strip, dropdowns on the `overlay` level, chrome
  buttons at the handoff's 30px.
- `SearchBar` → `TextField { variant: "bare" }`: `SearchBar` is absent from the
  auto-bind list, so `focus_text_at` cleared focus and the field could not be
  typed into at all.

**Proofs**: `just check` · `just diag` (host + os + kernel) · `just test-host`
· `dsl_apps_conformance` 8 stash / 9 window_kit / 2 icon_symbols ·
`nexus-dsl-runtime` 3 lock-step · `app-host` 15 · `windowd` frame policy 3 ·
visible boot (login → Stash: floating glass over the wallpaper, row hover,
"6 Objekte", ⋯ dropdown under its button, zero `[USER-PF]`).

**Known non-parity, unchanged**: exit animations (`.transition` is enter-only),
`.truncate()` is a no-op, `.scroll()` ⊥ `.overlay()` so the listing does not
scroll, monospace size column, 9px action label, breakpoints 640/1024 vs the
design's 560/820 — at a 960-wide default window that puts the properties pane
in the `regular` tier, i.e. as an overlay rather than inline.
(Per-keystroke filtering WAS on this list; R1 below makes it expressible.)

### Phase 10 (R1-R4) — what Phase 9 uncovered ✅ (2026-07-29)

Freeform + resize + a real text field ran four paths that had never executed.
Every fix is below the app layer; Stash only showed them first.

**R4 — the hit box was not the strip (`ui/layout`, `ui/widgets/text_field`)**

The one that cost the most to find, because the whole chain fails SILENTLY.
`update_box_geometry` writes a flex-grown row child's box at its allocation,
but `place_node` laid the SUBTREE out against the hugged measurement. Stash's
search strip was therefore 732px painted over a 180px `TextInput` — and the
input IS the hit box (`View::focus_text_at`). A click right of the placeholder
resolved to nothing, so app-host never sent `OP_SURFACE_TEXT_FOCUS`, windowd
recorded no `text_focus` route, and `handle_imed_push` dropped every commit for
that surface.

What made it expensive: `windowd: text input on` + `filter list ok` still fire
(that is windowd's OWN shell text state, not the app), and so does
`cursor: shape=text`. The only tell is the ABSENCE of `apphost: text focus set`,
which reads like an ordinary missed tap.

- `userspace/ui/layout/src/constraints.rs` (new): `row_child_constraints` —
  a grown child gets `definite(child_width, Some(measured.height))`, i.e. the
  main size the parent already recorded. Split out of `engine.rs`, which is at
  its LOC baseline.
- `TextField::fill_row()` + `GlassTextField`'s outer column `Align::Start` →
  `Align::Stretch`, so `.grow(1)` reaches the input instead of stopping at the
  widget's wrapper.
- The page's wrapper must be `.direction(row)`: a DSL `Stack` defaults to a
  COLUMN, where `.grow(1)` grows HEIGHT. This is a standing trap.
- **Proof**: `stash.rs::tapping_the_open_search_field_claims_text_focus` taps
  three quarters across the STRIP (not the box centre — a centre tap passed
  the whole time). Verified to fail with the engine fix reverted:
  `the input covers only x 293..473 of a 264..996 strip`.

**R1 — `on Change -> dispatch(...)` was dead while typing (`dsl/runtime`)**

`insert_text`/`backspace_text` wrote the binding and fired no Change handler;
`Change` was only used for focus resolution and the I-beam. `focus_text_at` now
also resolves the enclosing Change DISPATCH once per focus, by handler path
prefix (so `revalidate_text_focus` can re-resolve it after a re-emit), and the
edit path runs it. Platform-wide: both launchers get live search from this.
Stash also regained the `on Change` line lost in the `SearchBar` → `TextField`
conversion. Proof: `dsl/runtime/tests/text_change_dispatch.rs` (4 tests, 3 fail
against the old code).

**R2 — a resized surface inherited the old glass rectangles (windowd, app-host)**

`handle_surface_create` reset content/header/footer but not `layers`, and
app-host's re-create block never re-announced. Invisible while the list
scrolled (`scroll_id != 0` takes the 3-slice path, which ignores layers);
at 1280x744 the content fits, `scroll_id` becomes 0, and the stale rectangles
finally got read — anchored at `win_x + l.x`, i.e. the top-left. Now
`clear_app_layers` fires on create (fail-closed: a new surface declares no
glass until it says so) and the re-create path re-submits with a full present.

**R3 — app chrome behind the shell status bar (windowd, app-host, dsl/runtime)**

There was no safe-area mechanism at all: `DEVICE_FIELDS` is purely enum-based
and `Window { }` knows only style/mode/level/resizable, so apps carried
hard-coded spacers (Settings' was 40 against a 36px bar). Meanwhile `input.rs`
skips app windows above `SHELL_TOPBAR_H`, so a maximized window's 40px chrome
had FOUR usable pixels.

The contract, phone-style rather than desktop-style: freeform frames start
BELOW the bar; fullscreen frames start at y=0 and carry a top INSET, so the
window's own glass reaches under the bar while its controls sit below it. The
bar stays readable and operable in both.

- `surface_presentation.rs::bar_geometry` decides (host-tested, 7-row truth
  table); `clamp_frame_origin` gained a minimum y — with `.max()`, because
  `i32::clamp` PANICS when min > max on a short work area.
- Transport needs no wire change: `OP_SURFACE_RECT` has always been
  `(x, y, w, h)` with `y` encoded 0 and read as `_`.
- `LayoutNode::inset_top` adds to the ROOT's `padding.top`. Not a wrapper node:
  ids are pre-order, so one extra node shifts every id and breaks
  `path_to_box_id`, `collect_texts` and anim keying, which each walk with their
  own counter. Padding also costs zero allocations per emit, which matters on a
  bump heap that never frees. Proof: `dsl/runtime/tests/safe_area.rs`, incl.
  `the_inset_changes_no_node_ids`.
- `SettingsPage.nx` loses its `Stack { }.height(40)`.

**Also**: `files.count` re-read the directory `files.list` had just read (two
blocking vfsd round-trips per search, both parked in the UI thread). app-host
caches the last `readdir_page` per path; all three write paths invalidate it.
The DSL's `timeoutMs` is no longer discarded (`svc_call.rs::call_reply_within`).

**Boot-proven**: maximize → panes fill the window, no top-left rectangle (R2);
chrome sits below the status bar and the ⋯ dropdown opens from there (R3).
**Not boot-proven**: R1/R4 end-to-end typing — reproduced and fixed against the
real `.nx` on host with negative controls, but the last boot ran under
`-display gtk` (no VNC), so the live-filter click-and-type was not driven.

**Recorded, not fixed**: `effect_ime.rs` truncates `surface_id & 0x0F` for
window CONTROLS; every resize mints a new id, so after ~15 re-creates an app
addresses a foreign window. Latent today.

### Phase 11 (W1-W5) — the phone windowing model, built once ✅ (2026-07-29)

Phase 9/10 fixed defects one at a time; the user called the result out as a
regression bundle ("Fenster nicht mehr draggable, Fenster hinter der Topbar
nicht schließbar, ein 'App'-Fenster taucht auf") and asked for the model to be
built properly — iOS/Android, not desktop. The archaeology first, because
"wann wurde das gelöscht?" deserved a real answer:

| Symptom | Wann/Wo |
|---|---|
| Stash nicht draggable | P9 (`c93dcb71`): `style: plain` = kein WM-Titel = keine Drag-Zone; kein Ersatzkanal gebaut. Chromed Fenster zogen weiter. |
| Fenster hinter der Topbar gefangen | R3 (`cc126a29`) klemmte nur PLATZIERUNG; `drag_to`/Top-Resize klemmten weiter auf `0..display`. |
| Panes "auf dem Wallpaper" | Nie gebaut: der 3-Slice-Scroll-Arm übersprang ALLE Material-Regionen; Freeform-Stash ist gebandet. dst-so-far selbst lebt (TASK-0070 P4). |
| Blur teuer | Der GL-Pfad verwarf die `BackdropCache`-Felder seit je am `Command`-Encoding; unsichtbar bei 1-2 Glasschichten, ruinös ab P9s 5-10 (34ms-Presents). |
| Teal "App"-Fenster | Alt (Probe-Fill-Fallback), durch intermittierenden Payload-Timeout sichtbar. |

**W1 — Drag-Envelope** (`nexus-widget-window::DragBounds`): grab strip in
`[SHELL_TOPBAR_H, work_area_h)`, Body frei (hinter die Taskleiste, rechts bis
auf 64px raus; links bleibt 0 — negative dst-x kann der Compositor noch
nicht, notiert). EIN Envelope für Titel-Drag UND Top-Edge-Resize
(`drag_bounds` in intent.rs), Host-Tests in frame.rs.

**W2 — `CONTROL_WIN_MOVE` (7)** (`nexus-display-proto/control.rs` — der
CONTROL-Block ist dabei aus dem Baseline-file `client_surface.rs` in ein
eigenes Modul gezogen, Re-Export erhält die Pfade): window-kit `WinTopBar`
dispatcht `WinAct("move")` auf leeren Chrome-Strecken; app-host mappt
`"move"`; windowd raised + `begin_drag` am aktuellen Cursor; Release nimmt
den bestehenden Snap-Pfad. Fullscreen ignoriert. Alle Kit-Apps erben das.

**W3 — Fail-closed Mount**: `probe/mod.rs` präsentiert bei `app == None`
NICHTS mehr (`APPHOST: FAIL dsl mount (no window, fail-closed)`, Prozess-
Exit). Der Teal-Fill (`FILL_BGRA`) ist gelöscht. Offene Wurzel: WARUM der
Payload-Grant intermittierend >8s braucht (bundlemgrd unter Boot-Last) —
eigener Track.

**W4 — Regionen in Scroll-Fenstern** (`windowd/band_map.rs`, 6 Host-Tests):
Szene-Rect → gepackte Band-Zeile pro Slice (Header 1:1 hinter WM-Titel,
Footer hinter dem Header-Block, Content um `scroll_rows` verschoben +
Viewport-geclippt; Slice-Straddler werden übersprungen). Der Scroll-Arm in
`scene.rs` komponiert die Regionen NACH den drei Slices — Backdrop = das
gezeichnete Fenster darunter (Schichtenmodell: Panel sieht Fenster, Fenster
sieht Fenster darunter).

**W5 — GL-Blur-Cache** (`gpud/backend/blur_cache.rs`, 5 Host-Tests): pro
Glasschicht ein FNV-Key über alles darunter (effektive Draw-Inputs nach
Transform/Scroll-Overrides + Wallpaper-Generation + Display-Maße) + eigene
Rect/Radien. Hit = EIN maskierter Draw aus einer gepackten Cache-Textur
(1280×4096; der Gauss-Shader mit radius=0 degeneriert zum Center-Tap =
maskierte Kopie, gleiche SDF-Kante); Miss = Blur wie bisher + RT→Cache-Copy
(vor dem Content-Draw). Slot-Identität = Walk-Index; Packing-Offset im Key
verhindert Fremd-Zeilen-Reads; Überlauf fällt auf Live-Blur zurück (Kosten-,
nie Korrektheitsgrenze). Drag invalidiert genau die Schicht selbst + alles
darüber; alles darunter bleibt Cache-Hit.

**Gates**: check/diag PASS, test-host 619 Suiten, windowd 160+6, gpud 5,
frame.rs 7, Cross-Builds (os-lite und os-lite+virgl) grün.

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
