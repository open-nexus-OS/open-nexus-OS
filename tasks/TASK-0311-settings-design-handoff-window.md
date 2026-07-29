---
title: TASK-0311 Settings design handoff — full information architecture on WinAppWindow
status: In Progress (2026-07-29) — Phase 0+1 + repair round boot-proven end-to-end (landing, taps, menus, appearance live-accent, fullscreen); Phase 2 (sub-pages, view options, search) open
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

## Repair round (2026-07-29, user-reported: freeze, seam, glass, landing)

The user's real-session test found four failures the host suite could not see.
Two deep code analyses located every cause; all were PLATFORM bugs plus two
app-level deviations. Fixed at the owning layer:

- **Tap misses (the "freeze" half 1)** — the platform assumed ONE
  `.scroll(vertical)` container: `scroll_region_axis` took the FIRST (the
  sidebar) and `interact::hit_scrolled` rejected every clipped box outside
  that one viewport — 100 % of content-pane handlers dead. Fixed: per-box
  clip testing (active / nested / foreign viewport), largest container wins
  the band + `apphost: N scroll containers` marker. The round-1 "wrapper
  Stacks get no hit box" diagnosis was WRONG (the 8-box dump was a
  `.take(8)` artifact) — memory + project-layout.md corrected.
- **Invisible overlays (the "freeze" half 2)** — `render_band` paints
  unclipped boxes only into header/footer rows, while the hit-test uses
  their full rect: an open menu was invisible everywhere and swallowed every
  tap. Root fix: the 3-slice band model only supports FULL-WIDTH viewports —
  `negotiated_band` now refuses pages whose statics share rows with the
  viewport (`apphost: statics beside the viewport, plain-path fallback`);
  settings renders on the plain path, where overlays work.
- **The hard freeze** — windowd's `tick()` returned before
  `finish_window_transitions()` when the last spring emitted 0 updates
  (a `from == target` spring dies silently on its first step — pinned by a
  new `animation` test): `pending_wm` stuck forever, exit transform stuck at
  gpud, input still routing. Fixed (finish runs on empty ticks) + the
  `CONTROL_WIN_MOVE` sticky-drag trigger gated on the button being down +
  the same-size fullscreen early-return resets a stale transform.
- **Glass showed only the wallpaper** — `scene_raster` REPLACED pixels for
  every glass box. Now only glass ROOTS replace (the engine stamps
  `LayoutBox::glass_nested` by real ancestry); nested glass blends src-over,
  the glyph-drop rule keys on roots, and only roots become compositor
  regions (settings: ~27 → 4 layers, under the 16 cap, with a marker on
  overflow).
- **The seam** — statics vanished below `header_h` in the band (see plain-
  path gate above); the fixed band slices also composited UNBLURRED next to
  the frosted body (now share the backdrop treatment), and `band_map`
  truncated straddling regions (now splits into up to three slices).
- **Post-fullscreen garbage** — gpud's GL atlas alias was 4000 rows while
  windowd allocates 6400 (bands past row 7200 sampled foreign rows);
  aligned + sample clamp + freshly allocated band rows are zeroed. A
  WM-maximized freeform window also KEEPS its blur band now (it used to be
  translucent-unblurred tracing paper).
- **`.overlay()` id drift** — the engine visits absolute children LAST while
  `path_to_box_id`/`collect_texts`/`caret_input` counted declaration order:
  every handler/text behind a non-last open overlay resolved to the wrong
  box. All three walkers now mirror the visit order.
- **App:** landing = `browse`/`connections` (the handoff's initial state —
  the overview is an entered mode, not the landing page); the sidebar
  dropped its `.scroll` (single-band rule) and uses compact single-line
  items so all 12 sections fit the 526px frame.

Known cosmetic rest (polish backlog): overview grid's third column can kiss
the pane edge at 960px; the sidebar header row clips under the pane top;
`windowd: STALL present stuck ~530ms` once during the fullscreen re-create
(self-healing).

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
