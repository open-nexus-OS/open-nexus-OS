---
title: TASK-0311 Settings design handoff — full information architecture on WinAppWindow
status: In Progress (2026-07-29) — Phase 0+1 host-proven + boot-proven for RENDERING and sidebar navigation; wrapper-Stack tap targets miss (see Open)
owner: @ui
created: 2026-07-29
links:
  - Design contract: userspace/apps/settings/docs/design_handoff_os_settings_window/README.md
  - Metrics contract: userspace/apps/settings/docs/design_handoff_os_settings_window/LAYOUT.md
  - Window scaffold: docs/rfcs/RFC-0084-dsl-slots-component-content-regions.md
  - Scaffold execution: tasks/TASK-0308-dsl-slots-and-window-scaffold.md
  - Settings authority: tasks/TASK-0307-settings-distribution-v2.md
  - App layout convention: docs/dev/dsl/project-layout.md
  - Token gap list: docs/dev/ui/design-token-audit.md
  - Playbook: CLAUDE.md
---

## Context

`userspace/apps/settings/docs/design_handoff_os_settings_window/` is a complete
handoff: component contract, a `LAYOUT.md` written explicitly "for a native
reimplementation", resolved tokens, and `source/OsSettingsWindow.dc.html` whose
`_tree()` IS the information architecture — 12 sections, ~64 top-level rows,
~20 sub-pages with ~60 more rows, plus an overview grid, a search mode and the
appearance page.

The shipped app (417 LOC, one page, one god-store) showed roughly a sixth of
it: 8 sidebar entries, 3 section bodies, no overview, no sub-pages, no
appearance page, no scrolling, chip rows instead of grouped list rows. It also
hand-wrote the three-zone window chrome that `WinAppWindow` exists to provide —
RFC-0084 names settings as the second consumer that would prove the scaffold
generalises, and that had not happened.

Scope decision (with the user, 2026-07-29): rebuild the LOOK completely; keep
only what is functional TODAY functional (appearance + the region keys); defer
the sub-pages and the search mode.

## What the platform could and could not do (verified, not assumed)

Carried the work: flat `else if` (the old app's four-level nesting was legacy
style, not a limit), components with props + RFC-0084 slots, components reading
`$state` across stores, multiple stores, `Toggle`'s auto two-way binding,
`.scroll(vertical)`, and `.bgGradient(a, b)` taking hex EXPRESSIONS — the one
sanctioned literal-colour path, which is exactly what the handoff's accent
swatches, folder glyphs and mode previews need.

Shaped the work:

- **One `Event` binds to one `Store`.** A `reduce` may be declared once per
  event, must cover every case, and all arms together may touch one store. So
  fields live where their writers live (`menu` in `WindowStore`, `picker` in
  `RegionStore`), and cross-domain changes travel through an `@effect` that
  dispatches the other domain's event.
- **Effects cannot branch** — no `if` statement form in an effect plan. Every
  bridge fires unconditionally, which is why the toolbar renders back but not
  forward or refresh: `WinNav` must have exactly one meaning.
- **No records, no maps, no record literals** — the tree is literal markup,
  modular through components rather than data.
- **RFC-0084 rule 11 forbids `Slot x` inside a slot body**, so a Nuxt-style
  layout component wrapping `WinAppWindow` is a compile error. One page holds
  the window; the modes are view components.
- **No `Grid`** → wrapping rows with `minWidth` + `grow`.
- **`ListItem` does not wire leading/trailing** → rows composed from `Stack`.

## Phase 0 — platform (done)

- `nexus-theme-tokens::ACCENT_PALETTE` 6 → 9 (append-only: teal, amber,
  graphite) + settingsd's `is_theme_accent`; new lockstep test so the
  validator's literal list cannot drift from the palette, plus a
  `test_reject_unknown_theme_accent`.
- `Select` + `Breadcrumbs` registered in the DSL (`dsl/core/src/registry.rs`)
  with runtime arms calling the existing kit builders. Render tests in
  `dsl/runtime/tests/layout_viewport.rs`.
- 17 icon symbols in `resources/themes/base.nxtheme.toml`.

## Phase 1 — the app (done, host-proven)

~30 files: one page, chrome/rows/overview/appearance/sections components, five
domain stores, 251 i18n keys in `en`+`de` (navigation-level in ja/ko/zh).

- `Window { mode: freeform }` restores the handoff's floating glass window.
- `WinAppWindow` slots + responsive/overlay panes; scrolling content pane.
- Overview grid (12 cards) · breadcrumb trail · ⋯ menu with jump-to-section ·
  properties pane · all 12 section roots · the appearance sub-page.
- Really writes: `ui.theme.mode`, `ui.theme.accent` (all 9),
  `ui.locale`, `region.country`, `input.keymap`, `time.zone`, `time.format`,
  `ime.personalization`.

## Deliberately inert, recorded not faked

| Handoff | Here | Why |
|---|---|---|
| Modus "Automatisch" applies | drawn, writes nothing | `windowd::ThemeMode` is `dark\|light` |
| ~20 chevron sub-pages | rows render as value rows | a chevron that pushes nothing promises a screen that does not exist |
| Search mode | absent | the DSL has no string operations, so free text cannot filter |
| View options (`Beschreibungen anzeigen`, `Sidebar kompakt`, `Erweiterte Optionen`) | absent | they reveal Phase 2 content |
| Per-crumb navigation | trail shows, tap goes one level up | one widget = one node = one handler |
| Icon-style blend modes | approximated | the painter has no blend or filter stage |
| App-Icon-Stil / Ordnerfarbe | local selection only | nothing consumes them yet |

## Found by the visible boot (three fixed, one open)

The host tests were green while three of these were live — layout at
1280×800 in a test harness is necessary and not sufficient.

- **FIXED — `.wrap(true)` is a silent no-op.** It is documented in
  `docs/dev/dsl/modifiers.md` and reaches `Stack::flex_wrap`, but nothing in
  `userspace/ui/layout/src/engine.rs` reads that field. Twelve overview cards
  laid out in one overflowing row with the labels printed over each other.
  The grid is four explicit rows now; real flex-wrap is a platform follow-up.
- **FIXED — `.overflow(hidden)` collapsed the row groups.** A hidden-overflow
  container passes its own clipped constraints down, and with no explicit
  height that resolved to zero: the first boot showed three group captions
  and no rows.
- **FIXED — the breadcrumb separator was not baked.** `Breadcrumbs` joins
  crumbs with `›` (U+203A), which was not in `text-baked`'s Latin EXTRAS, so
  every trail printed a placeholder box. Added (with `‹`), and the EXTRAS
  field became a slice so the next charset addition is one line.
- **FIXED — squeezed sidebar icons.** `WinSideItem` let a long two-line label
  win the flex negotiation and crush the icon to a two-pixel sliver. The icon
  now sits in a pinned 16px box and the text column grows.
- **OPEN — a `Stack` wrapper around a component instance gets NO hit box.**
  `docs/dev/dsl/project-layout.md` documents wrapping an instance to carry
  `on Tap` as THE pattern; the handler registers and the row even highlights
  on hover, but the box never reaches the layout and every tap logs
  `apphost: input tap miss`. Every component that works today
  (`WinSideItem`, `WinMenuItem`, stash's `FileRow`) carries its handler
  INSIDE itself — which is why nobody hit this before. It costs this app the
  overview cards, the appearance controls, the picker options and the ⋯ menu
  rows; the sidebar, the section rows and the toggles are unaffected.
  Fix: move each dispatch into its component (a `target: Str` prop and
  `dispatch(Goto($props.target))`), splitting face-plus-wrapper the way
  window-kit does where one visual needs two dispatches. The doc has been
  corrected in the meantime.

## Proofs

- `cargo test -p dsl_apps_conformance` — compile + mount + 9 settings tests:
  overview completeness, all 12 section ids, the back chain, mode tiles →
  `ui.theme.mode`, all nine accents → `ui.theme.accent`, auto-mode writes
  nothing, the five region keys, picker closes on pick, demo section writes
  nothing.
- `cargo test -p settingsd` — accent palette lockstep + reject.
- `cargo test -p nexus-dsl-runtime` — `Select`/`Breadcrumbs` render.
- OPEN: `just check` and a visible `just start` boot (the design is a visual
  contract; host layout is necessary, not sufficient).

## Phase 2 (next)

The ~20 sub-pages behind the chevron rows, the ⋯ view options including the
`advanced` groups they reveal (Mono-Audio), and then the search mode — which
needs either string operations in the DSL or a `svc.search` index over the
tree.
