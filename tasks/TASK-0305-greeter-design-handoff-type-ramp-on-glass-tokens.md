---
title: TASK-0305 Greeter design handoff — type ramp v2, on-glass tokens, glass paint primitives
status: In Progress (2026-07-26) — host + visible-boot proven (dark); light theme + error state not visually proven
owner: @ui
created: 2026-07-26
links:
  - Contract seed: docs/rfcs/RFC-0082-type-ramp-v2-on-glass-surface-tokens.md (seeded by this task)
  - Design contract: userspace/apps/greeter/docs/design_handoff_os_login/README.md
  - Token SSOT: docs/rfcs/RFC-0070-ui-design-system-ssot-convergence.md
  - Shared atlas (size budget): docs/rfcs/RFC-0080-shared-atlas-ro-vmo.md
  - Session authority (unchanged): docs/dev/ui/shell/session.md
  - Token gap list: docs/dev/ui/design-token-audit.md
  - Playbook: CLAUDE.md
---

## Context

The login greeter is a DSL app (`userspace/apps/greeter`, mounted by windowd via
`compositor/runtime/session.rs → launch_app("greeter")`). The new handoff
(`docs/design_handoff_os_login/`) builds its entire hierarchy out of typography
and glass-on-wallpaper: a 132 px / weight-300 clock, a 21 px / 600 display name,
a 44 px glass pill with the placeholder and submit arrow inside, text shadows
everywhere for legibility.

None of that is renderable today. Verified in the code, not inferred:

- `source/services/app-host/src/probe/paint.rs` picks the face with
  `if font_size >= 15 { Body } else { Small }`, and only 13 px + 16 px are baked
  (`userspace/ui/text-baked/build.rs`). Every DSL text is one of two sizes.
- `.fontWeight` (modId 33) has no `apply_modifier` arm — one weight exists.
- `TextShadow` is a type with zero producers and zero consumers.
- `GlassSurface.border` is resolved from the theme and dropped by
  `Mods::visual()`; `.border()`/`.borderColor()` are no-ops.
- `LineHeight` is a fixed `Absolute(20px)` regardless of font size.
- No wallpaper tint/vignette concept exists.

So this task is mostly **platform work with the greeter as its first consumer**,
not a page rewrite. The contract lives in RFC-0082.

The session flow is explicitly unchanged: pick the user in the footer → password
→ arrow. Authority stays in `sessiond`.

## Goal

1. **Type ramp** (RFC-0082 Phase 0): `nexus-text-baked` bakes a (size, weight)
   ladder with an explicit per-face charset; `face(px, weight)` replaces the
   `>= 15` threshold in `probe/paint.rs` (3 call sites) and `measure_text.rs`.
2. **Text metrics in the DSL** (Phase 1): line height derived from font size;
   `.fontWeight` (+ `light`), `.leading` (+ `flat`), `.textAlign`, and
   `.paddingTop/Bottom/Leading/Trailing` stop being no-ops.
3. **Global theme tokens** (Phase 1): the on-glass color roles, `[typography]
   hero`, `[leading] flat`; `glassPanel`/`glassCard` re-tuned to the handoff's
   login values in both themes.
4. **Paint primitives** (Phase 2): glass hairline from `GlassSurface.border`,
   1 px inset top-shine, `.textShadow(none|soft|strong)` (modId 50),
   `.bgFade(topToken, bottomToken)` (modId 51), `.border`/`.borderColor`.
5. **Widgets**: `GlassTextField` gains `FieldVariant::{Boxed,Glass,Bare}` +
   `pill()`; `Avatar` gains a `name` prop (initials derived in the widget) and
   stops discarding its modifiers.
6. **Greeter**: `svc.session.active()` so the *authority* names the active user
   (the DSL cannot express `users[0]`, and guessing client-side would break the
   session contract); store, i18n and page rebuilt to the handoff; localized
   weekday/month names in `app-host/src/probe/clock.rs`.

## Non-Goals

- Focus ring on the field — needs `Focus`/`Blur` triggers, which do not fire.
- The unlock transition (whole-layer blur + scale) — compositor-owned.
- Gaussian text shadow — see RFC-0082 failure model.
- Letter-spacing / tracking and OpenType tabular figures.
- MSDF text rendering (its own RFC if ever).
- New users. `jenning` stays the only entry in `sessiond`'s registry; the
  switcher hides itself below 2 users per the handoff rule.

## Constraints / invariants (hard requirements)

- The 13 px and 16 px `Full` faces stay **byte-identical** — no reflow anywhere.
- Line height must be unchanged for font sizes ≤ 15 px (`max(20px, px × 1.3)`).
- No `Full` charset above 16 px (atlas budget, RFC-0082).
- New modifier ids are **append-only** — the index is the wire `modId`.
- No new glass wire level; `GLASS_*` stays as-is.
- Marker ladder unchanged; no new `ok` markers.

## Security considerations

Unchanged surface. The atlas stays read-only (RFC-0080); the secure field keeps
substituting bullets before the value enters the layout tree; `sessiond` remains
the only authority over who exists and who is logged in — `svc.session.active()`
reads the authority's answer, it does not compute one.

## Contract sources (single source of truth)

- Type ramp + on-glass roles + paint primitives: `docs/rfcs/RFC-0082-*`
- Token values: `resources/themes/*.nxtheme.toml` → `nexus-theme-tokens`
- Modifier catalog (ids = wire): `userspace/dsl/core/src/registry.rs`
- Service surface: `tools/nexus-idl/schemas/dsl_services.capnp`
- Design values: `userspace/apps/greeter/docs/design_handoff_os_login/README.md`

## Stop conditions (Definition of Done)

- [x] `cargo test -p nexus-text-baked` green, incl. an **atlas size assert** and
      per-face metrics tests; the 13/16 LATIN span is byte-identical (proven by
      `cmp`, and frozen as measured widths rather than face lengths — face
      lengths legitimately move when the CJK charset grows).
- [x] `cargo test -p nexus-theme-tokens` green with the new roles in all layers.
- [x] `just test-host` green (incl.
      `tests/dsl_apps_conformance::greeter_compiles_and_mounts`). `just check` pending.
- [x] Unknown `.textSize`/`.fontWeight`/`.fg`/`.material`/`.textShadow`/`.bgFade`
      token arguments are **compile errors**, with a test
      (`dsl_v0_1a_host::checker_rejects_carry_stable_codes`).
- [x] Visible boot: greeter matches the handoff in **dark** — hero clock,
      glass pill with hairline + inset shine, avatar initial, session buttons,
      wallpaper tint. **Light theme and the shake/error state are NOT visually
      proven** (see Evidence).
- [x] Glass re-tune checked on the shell top bar, island, status pills and
      taskbar in **dark**. Control center / launcher / light mode: not
      visually checked.
- [x] Marker ladder green: `sessiond: session start (user=jenning
      product=default)` → `windowd: dsl login detected` → `windowd: session
      shell visible (product=default)`.
- [x] Docs sweep done (see below).

## Touched paths (allowlist)

- `userspace/ui/text-baked/**` (build.rs, lib.rs, measure_text.rs, tests)
- `userspace/ui/theme-tokens/**`, `resources/themes/*.nxtheme.toml`
- `userspace/ui/widgets/{text_field,avatar}/**`
- `userspace/ui/scene_raster/src/lib.rs`
- `userspace/dsl/core/src/{registry.rs,check/names.rs}`
- `userspace/dsl/runtime/src/{registry.rs,emit.rs}`
- `source/services/app-host/src/probe/{paint.rs,clock.rs}`,
  `source/services/app-host/src/effect_host.rs`
- `tools/nexus-idl/schemas/dsl_services.capnp`
- `userspace/apps/greeter/**` (page, store, i18n, manifest comment)
- `docs/rfcs/RFC-0082-*`, `docs/rfcs/README.md`, `docs/dev/dsl/modifiers.md`,
  `docs/dev/ui/**`, `tasks/TASK-0305`, `CHANGELOG.md`

## Plan (small PRs)

1. Phase 0 — RFC-0082 seed + this ledger. *(done)*
2. Phase 1 — text-baked size/weight ladder + face resolver.
3. Phase 2 — DSL text metrics + per-edge padding modifiers.
4. Phase 3 — theme tokens + glass re-tune. **Boot-check the shell here**, before
   touching the greeter, so the OS-wide change is isolated.
5. Phase 4 — paint primitives (hairline, inset shine, `.textShadow`, `.bgFade`).
6. Phase 5 — `GlassTextField` variants + `Avatar`.
7. Phase 6 — `svc.session.active`, store, i18n, clock locale, page rebuild.
8. Phase 7 — verification + docs sweep.

## Evidence

### Phase 1 — type ramp (host-proven 2026-07-26)

- `cargo test -p nexus-text-baked` — 15 green, incl.:
  - `counters_are_not_filled_nonzero_winding` — the instanced rasterizer uses
    the NONZERO rule (even-odd would fill the bowl of `o`).
  - `hero_digits_are_tabular` — `13:16` and `08:59` measure identically; the
    hero face is unkerned.
  - `latin_faces_fall_back_to_the_body_face_for_cjk` +
    `fallback_glyphs_share_the_primary_baseline`.
  - `existing_latin_metrics_are_frozen` — the invariant that matters:
    `"Handgloves 0123"` still measures 107/132 px at Small/Body, line heights
    16/20, ascents 13/16.
- **Byte-identity proof** (not just a test): `cmp -n 20000 <old atlas> <new>` —
  the entire Latin span of the 13px face is unchanged; the first differing
  byte is 41125, inside the CJK tail where the calendar glyphs were added.
- Atlas 4 249 605 → 4 406 180 bytes (+3.7%), under the 5 MB budget assert.
- Rendered ASCII dumps eyeballed at 13/16/21/36/120px: clean AA, real weight
  separation, correct counters.

### Phase 3/4 — glass re-tune + paint primitives (host-proven)

- `dsl_goldens` scene goldens: exactly 3 of 4 changed —
  `dsl_todo_{initial,loaded,locale_de}` (they contain `Card`);
  `dsl_todo_loading` (no cards) is untouched. Pixel analysis of the diff:
  row 13 goes `(48,48,48) → (172,172,172)` = the new `inset 0 1px 0` line;
  rows 14–24 go `228 → 218` = tint alpha `.60 → .50`; the border ring
  `228 → 224` = border alpha `.80 → .70`. Nothing else moved, so the goldens
  were regenerated against a verified cause rather than rubber-stamped.

### Phase 6 — greeter (host-proven)

- `greeter_login_flow_is_tappable_end_to_end` — drives the REAL app: the
  pre-selected user's submit button is tappable and `login()` reaches the host
  with the **id** (`jenning`), not the label.
- `greeter_layout_matches_the_handoff_geometry` — hero clock band 126px
  (120 × `flat` leading), 44px pill, 180px grown input, 88px avatar, three
  44px session actions.
- **Two defects this caught**, both mine, both silent:
  1. `.overlay()` for the wallpaper tint hoisted the backdrop OUT of flow and
     painted it LAST — over the whole UI, swallowing every tap. Backdrops are
     backgrounds on an in-flow wrapper, not layers.
  2. `.grow(1)` never reached kit widgets (each arm built its own node and
     dropped the flex item), so the password input laid out 0px wide.

### Phase 7 — full gate

- `just check` green (fmt · clippy · deny · arch · structure).
- `just test-host` green.
- **`just diag` caught what `check` + `test-host` could not.** Two real
  breaks lived only under the OS cfg / test cfg and were invisible to the
  host build: `probe/clock.rs` read `self.locale` (the field is `locale_tag`)
  and `probe/paint.rs` used `BakedTextMeasure` without a path — both in
  `#[cfg(nexus_env = "os")]` code. Worth writing down: `just check` and
  `just test-host` do NOT compile the OS cfg. `just diag` does, and it is in
  `test-all`, not in `check`.
- Image budgets: `contract-windowd-size` (windowd-only) → `contract-image-budgets`
  (declared per-service table, arena math, catch-all bound). Measured that
  **537 KB of the windowd overshoot predates this task**; the ladder adds
  164 KB. Now wired into `just test-all`, which is how the original went red
  unnoticed. Full note in `tasks/TASK-0076B`.
### Visible boot (QEMU, virgl, 1280x800, dark theme) — 2026-07-26

Boot proof, VNC captures. What the screen actually showed:

- **Greeter matches the handoff.** Localized date "Sonntag, 26. Juli" (the
  clock's new calendar table), the hero clock at 120px Light with **tabular**
  digits (16:49 → 16:51 → 16:55 all measure identically — no sideways
  shuffle), the glass avatar with the derived initial "J", "Jenning" at 21px
  SemiBold, the 44px glass pill with its 1px hairline and `inset 0 1px 0`
  top-shine, the placeholder inside it, the submit arrow on a `glassFill`
  circle, the hint line, and three 44px session buttons bottom-right. No
  switcher row — one registered user, so the handoff's "hide below 2 users"
  rule falls out of the data rather than a flag.
- **Secure field + OSK**: typing masks to bullets and the on-screen keyboard
  opens on focus (RFC-0075), unchanged by the redesign.
- **Full login ladder**: `sessiond: session start (user=jenning
  product=default)` → `windowd: dsl login detected (session active)` →
  `windowd: session shell visible (product=default)`, and the desktop shell
  came up.
- **Glass re-tune blast radius**: the shell top bar, the island, the status
  pills and the taskbar all render correctly in dark mode with the darker
  tint — no washed-out chrome.
- **Atlas at its new size**: `execd: atlas vmo ready` → `atlas vmo granted` →
  `APPHOST: atlas mapped`. The 4.4 MB shared VMO maps fine; sizing is
  dynamic on both sides (`embedded_atlas().len()` / `atlas_len()`).

**Not proven visually, honestly stated:**

- **Light theme.** The Control Center did not open on that boot — by then it
  had logged `windowd: STALL present stuck 38196ms` ten times (a long-idle
  compositor state, unrelated to this track). The light values are covered by
  the theme golden tests, not by a screenshot.
- **The error/shake state.** `.effect(wiggle, trigger: $state.lastError)` is
  wired and compiles; nothing drove a wrong password on-screen because auth is
  not implemented yet (`sessiond` accepts any secret).
- A first boot under heavy load (concurrent cargo builds) failed at
  `APPHOST: FAIL no content rect` with `WARN windowd 26/26 11447ms slow`. On
  an idle machine the same image booted clean. Recorded because that failure
  mode reads like a code regression and is not one.

## Acceptance criteria (behavioral)

- A DSL page asking for `.textSize(hero).fontWeight(light)` gets a 120 px light
  numeral on screen, not a 16 px regular one.
- A `.material(panel)` surface has a visible 1 px hairline and a top-shine line
  in both themes.
- Text with `.textShadow(strong)` stays readable over a bright wallpaper.
- Switching light↔dark re-themes the greeter completely (tint, glass, text,
  shadows) with no restart.
- Removing the second/third user from `sessiond` hides the switcher instead of
  leaving an empty row.

## Follow-ups (not this task)

- `Focus`/`Blur` triggers → the handoff's focus ring.
- Unlock transition (compositor layer transform).
- Blurred text shadow (offscreen pass) if the 1 px offset proves too weak.
- Tabular figures / letter-spacing.
- MSDF text path (`userspace/ui/msdf` is built and wired to nothing).
