---
title: TASK-0312 Shell panels design handoff — topbar + six drop-downs 1:1
status: In Progress (2026-07-30) — Phase 0-5 + repair round 1 done (`just check`, `just test-host`, OS build green; 12 panel proofs); OPEN: re-boot to confirm round 1
owner: @ui
created: 2026-07-30
links:
  - Design contract: userspace/apps/desktop-shell/docs/design_handoff_panels/README.md
  - Visual truth: userspace/apps/desktop-shell/docs/design_handoff_panels/OpenNexusPanels.html
  - Layout SSOT: docs/dev/ui/patterns/shell-launcher.md
  - App layout convention: docs/dev/dsl/project-layout.md
  - Prior handoff port (pattern): tasks/TASK-0311-settings-design-handoff-window.md
  - Playbook: CLAUDE.md
---

## Context

`userspace/apps/desktop-shell/docs/design_handoff_panels/` is a complete
handoff for the shell's **top bar and its six drop-downs** — Control Center ·
Mitteilungen · Kalender · WLAN · Ton · Batterie: a `README.md` spec, a
`panels.css` geometry contract, resolved `tokens/*.css`, and an
`OpenNexusPanels.html` that is the interactive visual truth.

The shipped shell had rough sketches of all six (49–256 LOC each) and none of
the handoff's building blocks — no toggle tile, radio row, glass switch, meter,
signal bars or counter badge. The top bar was written out **inline twice**,
once per `ShellPage`, and the panel selection was a four-level nested
`if/else`, also twice. The Control Center carried TWO appearance buttons
because the DSL could not read the active theme.

Scope decision (with the user, 2026-07-30): rebuild the LOOK completely;
**only** the tablet/desktop switch and dark/light must be functional; the rest
is a mockup but **locally interactive**, so every state can be checked on a
real device.

## Decisions taken with the user

| Question | Decision |
|---|---|
| Pixel fidelity | Fixed sizes + radii exact; padding/gap snapped to the 4px grid |
| Mockup depth | Locally interactive, no `svc.*` |
| Theme readback | Add `device.theme` to the DSL |
| Type ladder | Bake the 11px Latin rung |
| Panel blur | Raise `glassPanel` to the handoff's 72 |

## Phase 0 — platform enablers (done, all append-only)

- **`.rounded()` also takes raw Int px.** The checker never rejected it (a
  numeric arg is skipped by `check_token_vocabulary`); the RUNTIME dropped it
  — `radius("")` fell through to 0, so `.rounded(18)` compiled clean and
  painted a square. One arm in `emit/modifiers.rs`. The handoff's radii
  (30/26/24/20/18/15/13/11) do not fit a five-rung token scale, and
  `.width`/`.height` already set the token-or-px precedent.
- **`device.theme` (`dark`|`light`)**, DEVICE_FIELDS id 9 + `FixtureEnv` +
  `device_for(…, theme)` threaded through all 12 app-host call sites. The
  host already had the packed byte one line above every one of them
  (`tokens_for(self.theme_mode)`) and simply never passed it on. This is what
  makes ONE appearance button possible.
- **Motion token `slideDown`** (id 8). Five exhaustive matches in
  `MotionToken` plus — the trap — `primary_prop`'s `_ => Opacity` wildcard,
  which would have swallowed a missing arm silently. The host's `target_for`
  keyed the travel sign on the PROPERTY, so without an edit there `slideDown`
  would have been a byte-identical alias for `slideUp`.
- **11px caption rung** (`FONT11`/`FONT11_SEMI`, Latin) in `text-baked`.
  Appending is safe (atlas offsets are order-based and these go last), but it
  put `sm` (12) on an exact 11/13 tie for the first time and the tie-break
  rounded DOWN — every 12px label in every shipped app would have silently
  shrunk. Tie now rounds **up**, which is behaviour-preserving for all ten
  type tokens; `tests/type_ladder_resolution.rs` pins the whole mapping so the
  next rung has to face the same question.
- **`Slider` rebuilt** to the handoff's 28px pill track: fill from the left as
  a flex WEIGHT (the layout equivalent of `width: 70%`, so the control has no
  fixed width), `min-width: 28` because the fill's leading edge IS the handle,
  and the glyph riding inside the fill at a fixed inset. The glyph arrives as
  a ready-made node (`leading`), so the widget crate needs no icon dependency.
- **Three new colour roles** — `sliderTrack` · `sliderFill` · `sliderIcon`.
  A slider is the one control whose track is a recess in BOTH themes while its
  fill is bright in both; no existing role has that polarity, and borrowing
  `scrim` would tie a modal backdrop to a volume track.
- **`ProgressBar` honours `.fg()`** (as `Spinner` already did), so a battery
  meter is not progress-blue.
- **Theme**: `glassPanel.blurRadiusDp` 40 → 72; ten new `[icons.symbols]`.
- **`anim.rs` split** — the ratchet caught the 7-line growth, so interaction
  motion (hover/press/toggle-thumb, ~120 LOC) moved to a CHILD module
  `anim/interaction.rs`. A child reaches the parent's private `AnimState`
  helpers, so nothing widened to `pub(super)` for the move. 892 → 776 LOC.

## Phase 1-5 — the app (done, host-proven)

~40 files. Twelve shared parts in `components/panels/parts/`, the six panels
with their own sub-component folders, ONE `TopBar` with seven pill components,
ONE `PanelHost`, five demo stores, 70 new i18n keys in **all five** catalogs
(the weekday names were previously raw German strings and never translatable).

Really writes: `ui.shell.mode` (Ansichtsmodus tile) and `ui.theme.mode`
(Erscheinungsbild button) — both directions, both reading their current value
from `device.*`.

## Deliberately inert, recorded not faked

| Handoff | Here | Why |
|---|---|---|
| Ellipsis on the one-line body | wraps | `.truncate` is a declared no-op; nothing in the painter clips with a marker |
| Panel exit animation | entrance only | `.transition` is enter-only platform-wide |
| Dynamic Island hover 288×64 | stays collapsed | a hover-driven size change is not an expressible motion token |
| Draggable sliders | render, do not drag | the `Change` trigger carries no payload — a drag has no value to deliver |
| padding 9/10/11/13/15 | 8/12/16 | spacing step is 4px (agreed) |
| Card blur 8 behind panel blur 72 | blends into the panel backdrop | nested glass gets no compositor region of its own (16-region cap) |
| Inset shadow on the slider track | 1px inset line | the row painter has no offscreen buffer to blur in |
| Live networks/devices/notifications/appointments | demo state | no radio, audio, power, notification or calendar service exists |
| A computed month grid | three authored months | no date arithmetic, and a reducer cannot build a list |

## Proofs

- `cargo test -p dsl_apps_conformance` — **11 new** `shell_panels.rs` proofs:
  all six panels open at their handoff widths in desktop and only three exist
  in touch; one-at-a-time; the backdrop closes each; the view-mode tile writes
  the OTHER `ui.shell.mode`; the appearance button writes both theme modes AND
  renders differently per theme (which is what proves `device.theme` works);
  every demo control reaches NO service; mute moves the volume fill; the
  calendar pages and clamps; the dimmed network list carries no handlers.
  Plus the pre-existing shell dead-zone, hit-slop and reemit tests, unchanged
  and still green against the rebuilt tree.
- `budget_probe::probe_shell_budgets` — payload 182 KB of the 512 KB ceiling
  (36 %), worst panel 264 nodes of the 4096 cap (6 %, the calendar). Both
  ceilings fail silently in production, so both are asserted with headroom.
- `nexus-text-baked`, `nexus-dsl-runtime` (`token_vocabulary_lockstep` now
  covers MOTION and the two slide tokens too — that seam had no test),
  `ui_v10_goldens` (slider regenerated + a new `slider_icon` case).
- `just check` and `just test-host` green (623 test groups).
- **OPEN: a visible `just start` boot.** The design is a visual contract; host
  layout is necessary, not sufficient. To check against the HTML: anchors,
  one-panel-at-a-time, outside-click, the mode switch actually re-laying the
  bar, live dark/light — and whether `glassPanel` at 72 reads right on the
  dock and taskbar, which share the level.

## Repair round 1 (2026-07-30, from a visible boot)

Five findings, four of them platform bugs rather than markup mistakes.

- **The mode switch felt dead.** The two connectivity columns were `.grow(1)`,
  which is flex-grow over an AUTO basis: it splits only the FREE space, so the
  column with the longer label came out 17px wider. The narrower tile's box
  stopped short of the card it sat in and every tap in that gap fell through to
  the panel's `PanelNoop` absorber — which by design changes nothing. Columns
  are now stated at 144 = (328 − 2×16 − 8)/2, the handoff's `1fr 1fr`.
  `the_view_mode_tile_is_tappable_across_its_whole_width` taps both edges,
  because a centre-only assertion passes with the bug present.
- **Panels read too dark, in both themes.** Two independent causes, and the
  user named both:
  1. **The shadow was painted UNDER the panel.** CSS paints an outer
     `box-shadow` "as if the border box were opaque" — never beneath the
     element. Ours filled the interior at full alpha, which is invisible under
     an opaque box and very visible under a translucent one: every glass
     surface composited over its own shadow. Fixed by knocking the casting
     shape out of the shadow, in BOTH painters (the `FS_SHADOW` TGSI shader
     gains the unshifted shape as `CONST[3]`; `paint_shadow_row` gains the
     same test with a 1px feather). The `toast_*` and `glass_cards` goldens
     moved because the toast and cards are glass — the fix landing.
  2. **The dark tint was the wrong colour.** `design_handoff_panels` specifies
     `rgba(255,255,255,0.10)` dark / `0.50` light; ours was `#121214@0.40` —
     opposite polarity, four times the coverage. That value came from the
     LOGIN handoff (RFC-0082) and still holds for card/window, which sit on a
     surface; a panel floats over the wallpaper and takes its depth from
     blur(72) + saturate. Light already matched; its border and top-shine
     moved to the handoff's .75/.22. **This moves the dock and taskbar too** —
     one panel material, one depth.
- **Icon circles went oval** under a long label: the text column won the flex
  negotiation. `.shrink(0)` on every fixed-size lead (tile circle, radio lead,
  notification app icon) and `.minWidth(0)` on the text columns beside them.
- **Cut text was invisible.** The platform does not wrap — `layout_lines`
  always yields one line — so an overlong run was clipped mid-glyph and read
  as a shorter label. `…` is now baked into every face's EXTRAS tail and
  `ellipsis_cut` decides where to cut so the prefix PLUS the ellipsis fits;
  the painter marks any run wider than its own box. This retires the
  "no ellipsis" entry from the table above.
- **The notification peek looked squeezed.** The handoff overlaps two cards by
  translating the back one 7px down; without transforms the sliver is a real
  box, and at 7px its corner radius clamps to 3.5 so it read as a pill bar.
  12px with a 12px radius reads as a card edge; dimming pushes it back.

Two files crossed the 600-LOC line and were split by responsibility:
`text-baked` gained `metrics.rs` (run measurement + the ellipsis cut — nothing
there touches a pixel), `paint.rs` gained `paint/collect.rs` (the three scene
walkers whose node numbering must agree with `path_to_box_id`).

**Process note.** Both module moves compiled clean under `cargo check` and
failed the OS cross-build on visibility: `pub(super)` narrows when an item
moves a level deeper, and the app-host BIN is only built under the OS cfg. Run
`scripts/build.sh` after any move, not just `just check`.

## Follow-ups

- Value-carrying `Change` (or a drag trigger) would make brightness/volume
  real without touching a line of this markup.
- A `svc.notif` feed turns `NotificationsPanel`'s `if` into a `List`; the card
  is already a props component for exactly that.
- The three authored months collapse to one data-driven grid the day a reducer
  can assign a list literal — worth re-checking before anyone adds a fourth.
