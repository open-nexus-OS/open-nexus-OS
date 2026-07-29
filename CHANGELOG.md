# Changelog

All notable changes to Open Nexus OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added - 2026-07-29 (Settings design handoff, Phase 0+1)

The settings app was showing about a sixth of its handoff: 8 of 12 sidebar
entries, 3 of 12 section bodies, no overview, no appearance page, no
scrolling, chip rows where the design has grouped list rows — and it
hand-wrote the window chrome that `WinAppWindow` exists to provide. This
round rebuilds it against `docs/design_handoff_os_settings_window/`, in the
enterprise layout (`docs/dev/dsl/project-layout.md`): one page, ~25
components by domain, five domain stores.

- **Platform, so the app could be written honestly:**
  - `Select` and `Breadcrumbs` are reachable from the DSL. Both kit crates
    were built "DSL-emittable" and had no runtime arm, so any page needing
    them had to rebuild the look. `Select` is the closed trigger only, as its
    module doc specifies; the option panel stays an app-owned `.overlay()`.
  - The accent palette grows 6 → 9 (`nexus-theme-tokens::ACCENT_PALETTE` and
    settingsd's validator, both append-only): `teal`, `amber`, `graphite`.
    All nine handoff swatches now write for real. settingsd gained a lockstep
    test so the validator's literal list cannot drift from the palette.
  - 17 icon symbols for the settings rows (`[icons.symbols]`).
- **The app:** all 12 sections with their handoff rows and values, the
  12-card overview, the breadcrumb trail, the ⋯ menu with jump-to-section,
  the responsive/overlay panes from `WinAppWindow`, a scrolling content pane,
  and the full appearance page (mode tiles, 9 accents, icon styles, folder
  colours). `Window { mode: freeform }` restores the handoff's floating glass
  window — `fullscreen` forces an opaque page base and skips windowd's
  backdrop-blur band.
- **What is deliberately NOT functional** is recorded rather than faked: the
  ~34 demo switches sit in their own store and reach no service; Modus
  "Automatisch" is drawn but writes nothing (windowd's `ThemeMode` is
  `dark|light`); the ~20 chevron sub-pages are Phase 2, so those rows render
  as value rows instead of chevrons that push nothing. Non-parity table in
  `docs/dev/ui/design-token-audit.md`.
- **Test-helper fix:** `common::dispatch` in `dsl_apps_conformance` passed an
  empty i18n key table, so any scene re-emitted after a dispatch read back as
  a list of blank strings. Added `dispatch_with_keys` and documented the
  trap on both.

### Fixed - 2026-07-29 (TASK-0308 W1-W5: the phone windowing model, built once, correctly)

R1-R4 (below) fixed what a floating glass window RAN into; this round builds
the model it was always supposed to live in — iOS/Android, not desktop: the
shell status bar and taskbar are always usable, every window is draggable by
its grab strip, the strip can never leave the band between the bars, and glass
composites as layers over whatever is really beneath it.

- **Drag clamps** (`nexus-widget-window::DragBounds`, one envelope for title
  drags AND top-edge resizes): the grab strip — WM title bar, or the app's own
  chrome row on a chromeless window — stays in `[status bar, taskbar)`. The
  body is free: it may hang behind the taskbar or partly off the right edge
  (64px stay visible). Previously `clamp_pos` allowed y=0..display and a
  window dragged under the bar could never be closed again: `input.rs`
  refuses presses above the bar by design.
- **`CONTROL_WIN_MOVE` (7)**: chromeless (`style: plain`) windows are
  draggable again. Stash lost its title bar in P9 (`plain` is the only route
  to window glass) with no replacement channel. Now the window-kit chrome row
  itself is the grab strip: a press on any empty stretch dispatches
  `WinAct("move")` → the existing `window.control` path → windowd raises the
  window and runs the SAME drag it runs for a WM title bar (clamp, release
  snap, fullscreen excluded). Every app on the kit inherits this.
- **A failed DSL mount shows NO window** (fail-closed). It used to present
  the 320×240 teal probe fill — bring-up proof turned fail-open ghost: an
  intermittent payload-grant timeout made a window titled "App" pop up that
  the user never launched. The failure stays loud in the log; the process
  exits for execd to reap.
- **Scrollable windows keep their pane glass** (`windowd/band_map.rs`,
  host-tested): the 3-slice scroll composite skipped ALL material regions
  ("scroll takes priority"), so the moment a freeform window became banded its
  panes sat flat on the window blur — read as "panel floats on the wallpaper".
  Scene rects now translate into the packed band per slice (header/footer
  fixed, content shifted by scroll and clipped to the viewport), and the
  regions composite AFTER the slices — so a pane frosts the window content
  beneath it, the window frosts the windows beneath it (the GL path has been
  destination-so-far since TASK-0070 Phase 4; `gpud: rt backdrop dst ok`).
- **Glass blur is cached again** (`gpud/backend/blur_cache.rs`): the GL RT
  build-up re-renders every present, so every glass layer paid snapshot + two
  gaussian passes on every cursor move — the windowd-side `BackdropCache`
  never survived into this path (its fields are dropped at the `Command`
  encoding), invisible at 1-2 glass layers, ruinous at the 5-10 the app panels
  brought (34ms avg presents). Now a per-layer FNV key over everything
  composited below (post transform/scroll, wallpaper generation included)
  gates the blur: unchanged backdrop = one masked draw from a packed cache
  texture (the gaussian shader at radius 0 doubles as the masked copy);
  changed = blur live + refill. Pure key/packing state is host-tested.

### Fixed - 2026-07-29 (TASK-0308 R1-R4: what Phase 9 uncovered — all below the app layer)

Making Stash a floating glass window ran four paths that had never executed.
None of the fixes is in an app: Stash only showed them first.

- **A text field's hit box was not the strip it painted.** A row child grown by
  `flex_grow` had its BOX written at the allocation while its SUBTREE was laid
  out against the hugged measurement — so a 732px search strip sat over a 180px
  `TextInput`. Since `View::focus_text_at` resolves a tap against that input's
  rect, a click right of the placeholder claimed no focus, app-host sent no
  `OP_SURFACE_TEXT_FOCUS`, windowd recorded no text route, and every imed
  commit for the surface was dropped — typing did nothing, silently, with no
  error anywhere in the chain. Fixed in the engine (`row_child_constraints`,
  split out to `userspace/ui/layout/src/constraints.rs`), plus
  `TextField::fill_row()` and a stretched `GlassTextField` column so the
  painted strip and the hittable strip are one rectangle.
- **`on Change -> dispatch(...)` was dead while typing, in every app.**
  `insert_text`/`backspace_text` wrote the binding and never fired the
  enclosing Change handler; `Change` was only ever consulted for focus
  resolution and the I-beam cursor. `focus_text_at` now resolves the enclosing
  dispatch ONCE per focus (by handler path prefix, so a re-emit can re-resolve
  it) and the edit path runs it. Both launchers get live search from the same
  change.
- **A resized surface inherited the old surface's glass rectangles.**
  `handle_surface_create` reset content/header/footer but not `layers`, and
  app-host never re-announced after a re-create — visible as a rectangle stuck
  in the top-left of a maximized window, because a window whose content fits
  takes the compositor path that actually reads `layers`.
- **App chrome sat behind the shell status bar.** There was no safe-area
  mechanism at all: apps carried hard-coded spacers (Settings' was 40px against
  a 36px bar) and windowd skips presses above the bar, so a maximized window's
  chrome had four usable pixels. windowd now decides
  (`bar_geometry`: freeform frames start below the bar, fullscreen frames start
  at y=0 and carry a top INSET), ships it in the long-unused `y` of
  `OP_SURFACE_RECT`, and the scene root absorbs it as padding — the app's
  background still reaches y=0 so the translucent bar sits on it, only the
  content moves down. Padding rather than a wrapper node because node ids are
  pre-order: an extra node shifts every id and breaks handler box-ids, text
  collection and animation keying alike.
- **Two blocking vfsd round-trips per search**: `files.count` re-read the same
  directory the listing had just read. app-host caches the last `readdir_page`
  per path and every write path invalidates it; the DSL's `timeoutMs` is no
  longer discarded.

### Changed - 2026-07-28 (TASK-0308 Phase 9: Stash design parity)

The file manager now matches its design handoff. Most of the work was below
the app — five defects that all shared one shape: something was authored, and
nothing above it could see that it never arrived.

- **Six theme roles were unreachable.** `divider`, `glassHover`, `glassActive`,
  `toggleOnBg`, `toggleOffBg` and `notifDot` were in every `.nxtheme.toml` but
  had no `ColorToken`, so no `.nx` page could name them — hairlines were drawn
  with the OPAQUE `border` role instead. `token_vocabulary_lockstep.rs` now
  asserts the checker's list and the runtime's map are a bijection.
- **New glass levels `windowPane` + `windowBar`** carry the handoff's
  `--glass-window-pane-*` / `--glass-window-bar-*`. Window panes previously
  inherited `glassPanel` — the near-black dock-tile level, which is correct on
  a wallpaper and wrong on window glass. No wire change: `glass_level` is a
  blur bucket, and the tint is painted app-side.
- **Four icon symbols rendered as grey placeholder boxes** (`square.grid.2x2`,
  `arrow.clockwise`, `arrow.uturn.backward`, `calendar`): a symbol name travels
  as a prop string, so the compiler never sees it.
  `tests/dsl_apps_conformance/tests/icon_symbols.rs` scans every shipped `.nx`
  file and names the file, line and symbol.
- **The hover wash was switched off platform-wide** because it followed the
  handler box's corner radius and a handler is often a square wrapper around a
  pill. Fixed by aiming it (`app-host/src/hover_wash.rs`, host-tested) instead
  of deleting it, and by separating the WASH's size rule from the GROW's — the
  shared ≤160px rule had excluded every full-width list row.
- **An orphaned event opened Stash's search field on every launch.** An
  `@effect` on an event nothing dispatches is a ROOT effect, and
  `run_initial_effects` dispatches roots through the REDUCER. The test that
  covered it fired the orphan directly, so it could never fail.
- **Window geometry**: `mode: freeform` now asks the compositor for its frame
  (it used to mount at the 320x240 probe fallback), windowd answers with the
  same centred three-quarter frame the WM applies on a mode switch (it used to
  answer with an unframed slot's 1280x3072 ceiling), and a cascaded origin is
  clamped onto the work area. App stacks went 8 → 16 pages: `app-host` recurses
  over its own scene, and Stash overflowed 8 pages by 48 bytes.

### Changed - 2026-07-28 (RFC-0085: the kernel owns every virtual address)

Userspace no longer invents virtual addresses anywhere in the tree. New
syscalls `vm_map` (53), `vm_unmap` (54) and `mmio_map_auto` (55) map whole
ranges at KERNEL-CHOSEN addresses inside a managed per-process window
(`[0x5000_0000, 0x8000_0000)`, first-fit, superpage-phase-aware — ≥2 MiB
ranges promote to 2 MiB leaves). The fixed-VA ancestors `map` (4) and
`mmio_map` (27) and their nexus-abi wrappers (`vmo_map`, `vmo_map_page`,
`vmo_map_page_sys`, `mmio_map`) are DELETED; both numbers are retired and
never reused.

- gpud's ~12 000-syscall framebuffer map loop became ONE call
  (`handoff_to_ready_ms` 63–69 → 0); its two fixed VA arenas, hidrawd's
  nine windows, the six copies of `0x2000_e000` and virtio-blk's
  module-global windows are gone. `release_resource` really unmaps, and
  `vmo_destroy` refuses EBUSY while any address space still maps the range.
- Map errors keep their identity across the ABI (ADR-0054):
  `AbiError` gained `OutOfMemory`/`NoSpace`/`NotFound`/`Busy`, and the
  historic ENOMEM/ENOSPC→`SpawnFailed` collapse is gone (spawn wrappers
  translate locally).
- First gated mm markers ever: `KSELFTEST: vm map ok / vm unmap ok /
  vm map reject ok` plus the userspace `SELFTEST: vm map roundtrip ok`.
- SMP fixes shaken out by the first real shootdowns: the S_SOFT doorbell
  is consumed BEFORE the mailbox checks (a check-then-clear race lost
  shootdown requests), the boot hart now enables `sie.SSOFT` (it was deaf
  to secondary-initiated shootdowns), every trap entry acks pending
  shootdowns, and the vm_unmap shootdown wait runs with the BKL dropped
  (phased like `vmo_create`) so `bkl budget ok` holds at SMP≥2.
- Torn-marker fix: 13 services' per-byte `emit_line` fallbacks now emit
  one atomic `debug_write` line (a preemption mid-`rngd: ready` dropped
  the tail and failed the ladder).


### Fixed - 2026-07-27 (three visual defects the greeter showed first: hover plate, square blur, stretched wallpaper)

All three were platform faults surfaced by the login screen, not greeter code —
the greeter's `.nx` page is unchanged.

- **Hover drew a white box where only the icon should grow.** The DSL paint path
  stacked a wash plate plus a bright 2 px ring on top of the hover-grow spring.
  The wash follows the box's `corner_radius`, and an icon button's handler box
  carries none — so a hovered *circle* wore a white *square*. Motion is now the
  whole affordance: the control springs to 1.06 and nothing else paints. Window
  chrome keeps its title-bar wash (its own path, and those buttons don't grow).
- **Rounded glass had blurred backdrop standing outside its corners.** The
  frosted-glass backdrop pass blurred the layer's bounding RECTANGLE while the
  fill on top was rounded, so every pill, avatar and circular power button sat
  in a visible blurred square. The final blur pass now carries the layer's
  corner radius and writes the rounded-rect SDF coverage as its alpha
  (`FS_BLUR_ROUND`, alpha-over blend) — the same analytic curve the content pass
  uses, so blur edge and fill edge are one line. Square layers keep the old
  shader and blend byte-identically.
- **The wallpaper was stretched, not covered.** The bake mapped the full source
  onto the full panel: a 3:2 image squashed ~7% into an 8:5 screen. It now takes
  the largest centred window of the source with the target's aspect — one axis
  whole, the other cropped evenly — matching the design contract's
  `object-fit: cover`. Never a letterbox: the window is inscribed in the source,
  so every destination pixel has source behind it. windowd's runtime cover LUT
  already did this for non-native display modes; the bake was the odd one out.

Proven on a visible boot (greeter + desktop, 1280×800 virgl): button bbox
corners now read plain wallpaper where they read blurred backdrop before, the
hover ring is gone from both the greeter submit button and the dock, and the
wallpaper is aspect-correct with no bars. `gpud: rt backdrop dst ok` still
fires. Blur shader source moved to `gpud/src/virgl_blur_shaders.rs`
(structure-gate ratchet on `backend/virgl3d.rs`).

### Changed - 2026-07-27 (settings distribution v2 — one authority, snapshots, reemit; RFC-0083 / TASK-0307)

One accent tap used to tear down and remount every DSL app (initial effects
re-run, services re-fetched), back up input by 36 events and time out the
persist. The concept was wrong, not a bug: split authority (theme keys wrote
to windowd, everything else to settingsd, GET always read settingsd), 2 s
yield-spins in the app-host per settings call, replies gated behind statefsd
round-trips, fire-and-forget pushes with no loss detection, and a full app
remount as the reaction to a COLOR change.

Now: **settingsd is the one authority** (ADR-0053) — every settings key
routes there; it validates, applies live, replies in microseconds
(persistence is async, coalesced, retried with backoff, and honestly
reported), and notifies watchers with self-healing delivery (registration
burst = boot restore; a dropped event is re-sent as full state on the next
change). windowd folds theme+accent+profile+region into ONE versioned
presentation snapshot (`OP_SURFACE_SETTINGS`, retained latest-wins,
per-frame retry, never blocking the compositor) and applies its own chrome
from the same events — its 3-GET boot probe (24×250 ms) is deleted. Apps
apply the snapshot as a **reemit, never a remount**: store state survives,
`@effect on Load` does not re-run, `if device.profile` arms re-select live
(profile switches included; `ProfileEvent::Changed` is the data-reload
hook). Legacy wire ops 16/17/23 and the CONTROL theme/accent/profile writes
are deleted, their numbers reserved. Every yield-spin on these paths became
a kernel-parked deadline wait.

Security: theme/shell writes now require a granted settingsd route (pack
ceiling `settings|shell`) instead of the windowd request slot every windowed
app holds. Recorded follow-ups: statefsd hardening, per-key write ACLs, and
a measured kernel finding — a foreign-held send cap on a service-pair
endpoint starves the system when used (watch-as-wake-source deferred; side
channel + bounded 500 ms idle tick shipped instead).

### Fixed - 2026-07-26 (the UI "barely reacted" — a dropped ack, not a dropped click; TASK-0306)

Diagnosed from a real interactive boot, and the first hypothesis was wrong in
a useful way: **no tap was ever lost.** `FAIL desktop input send` = 0 across
the whole session. The input path already retries a tap 400 times.

What was lost were **ACKs**. All three ack senders did this:

```rust
Err(_) => { debug_println("WINDOWD: FAIL desktop event send"); true }
```

One attempt, no retry, and `true` returned on failure. The asymmetry was the
bug: the frame a client merely *reads* got 400 retries, the frame a client is
**blocked waiting on** got one shot and a lie. A dropped present-ack does not
lose a pixel — it leaves the client waiting for a reply that never comes.
The 504 ms `STALL present stuck` in the log lands **1.2 ms** after exactly
that send failing, and the compositor loop falls from 56 Hz to 17 Hz.

- **One delivery contract** (`client_surface::Delivery`) replaces three copies
  of the same guesswork: `Blocking` for anything a client waits on (present /
  create / destroy acks, taps), `Coalescing` for anything the next frame
  supersedes (hover motion, region pushes). `send_client_frame` returns
  whether it actually delivered.
- The recovery was **already in the tree** — `compositor/mod.rs` falls back to
  `reply_and_close_wait` / `server.send`, both `Wait::Blocking` and neither
  losable. The `true`-on-failure return is what kept it from ever running.
- Measured over 8 boots: `FAIL desktop event send` 2 → **0**,
  `surface present stale` 4 → **0**.

### Changed - 2026-07-26 (SMP bring-up: stop starving the hart we wait for)

Every boot came up with **3 of 4 harts**, and a *different* hart went missing
each time (`got=0x7`, then `got=0xb`, then `got=0xd`) — a race, not dead
silicon. The boot hart waited like this:

```rust
loop { if online() { return true } if timeout() { return false } spin_loop(); }
```

A **hot spin, up to 500 ms per hart**. Under TCG that burns the boot hart's
whole scheduler quantum and starves the very vCPU it is waiting for. The
kernel already knew this 400 lines away — `kmain_secondary` parks in WFI with
the comment *"under icount/TCG a spinning vCPU steals whole scheduler
quanta"* — the bring-up wait never got the same treatment.

- The wait now parks in WFI on a self-armed SBI timer (no scheduler timer
  exists yet at that point in kmain, so it arms its own; `sstatus.SIE` stays
  clear so the timer only *wakes* WFI and is never taken as a trap into a
  half-built scheduler). It also bumps liveness, removing the documented
  "watchdog: no progress right after cpu0 sched loop" hazard.
- All-4-hart rate: **0 / 3 boots → 7 / 8 boots.**
- **Not deterministic, and not claimed as fixed.** New instrumentation
  (`BRINGUP_ASM_SEEN`, written by the entry stub as its first instruction)
  shows the residual failure as `asm=0 stage=0`: the hart never executes even
  the first instruction of our stub while SBI reports it STARTED. That is
  below our code, and the existing retry cannot help because SBI then answers
  `ALREADY_AVAILABLE`.
- Methodology note: the first version of that instrument was a plain
  `static [u64; N]`, which can land in `.rodata` (`AM`, no `W`) — the store
  would be dropped and it would read 0 forever, "confirming" whatever you
  hoped. It is an `AtomicU64` now.

Still open (measured, not fixed): IPC syscalls hold the global kernel lock for
10–25 ms (`long ecall nr=14 25ms`, `nr=0 21ms`, `nr=26 13ms`), which is what
makes 17 service selftests start inside a 241 ms window and drain over 2.6 s.
See `tasks/TASK-0306`.

### Added - 2026-07-26 (the type ramp the design system always assumed it had — RFC-0082 / TASK-0305)

The login greeter was supposed to be a re-skin. It turned out the platform
could not render the design at all: **every text run in every DSL app was
being drawn at 13px or 16px, in Regular 400**, no matter what the page asked
for. `.textSize(display)` (36px in the type scale) painted identically to
`.textSize(md)`; `.fontWeight()` was declared, documented, type-checked — and
had no runtime arm whatsoever. The type scale existed in the tokens and died
in the painter. It read as a spacing bug for a long time because measurement
used the *token* size, so boxes were laid out at 36px while glyphs came out
at 16.

- **A (size, weight) ladder in the baked atlas.** `nexus-text-baked` now bakes
  13/16 Regular (full Latin+CJK), 13/16/21 SemiBold and 21 Regular (Latin),
  36 SemiBold (Latin) and a 120px Light face carrying **digits only**. Every
  face declares a charset budget: a full charset above 16px is forbidden — the
  hangul syllable block alone would add hundreds of MB to an atlas that is
  mapped into every app-host as one shared RO VMO (RFC-0080). Resolution is
  `FontSize::nearest(px, weight)`, and **size wins over weight**: the other
  order answers `nearest(14, Light)` with the 120px hero face.
- **Light 300 and SemiBold 600 are instanced, not vendored.** `fontdue` has no
  variation-axis API, so the weighted rungs come from `ttf-parser`'s `wght`
  axis filled by a nonzero-winding scanline rasterizer in the build script.
  Regular 400 keeps going through `fontdue`, which is why the 13/16px Latin
  coverage is **byte-identical** and nothing reflowed.
- **CJK above 16px falls back to the 16px full face** — smaller than its
  neighbours, but rendered. Bounded and documented, never blank.
- `.fontWeight` (+ `light`), `.leading` (+ a `flat` step), `.textAlign`,
  `.border`, `.borderColor` and the four per-edge paddings stop being no-ops.
- **`.textShadow(none|soft|strong)`** (modId 50): one extra glyph pass at a
  1px offset. Deliberately not a blur — the row painter has no offscreen
  buffer, so the tokens are emphasis steps and the docs say so.
- **`.bgFade(top, bottom)`** (modId 51): a vertical fade between two color
  TOKENS, so a vignette follows the theme (`.bgGradient` keeps taking raw hex
  because app-icon artwork colors are data).
- **Unknown token arguments are compile errors.** `.fg(oNSurface)` used to
  type-check and then resolve to `None` at runtime — the node painted its
  default and the author got no signal at all.

### Fixed - 2026-07-26 (glass had no edges, and five widgets each drew it differently)

- **`GlassSurface.border` was resolved from the theme and then dropped.** Every
  glass surface in the OS — dock, control center, launcher, cards — has been
  edgeless. It now paints the 1px hairline, plus the design system's
  `inset 0 1px 0` top-shine as a real pixel line
  (`VisualStyle.inset_highlight`). The same authored `edgeHighlight` alpha
  feeds both jobs at different strengths: full for the crisp line, capped at
  15% for the soft wash, because the login recipe's 0.60 bleaches a whole pane.
- **One glass recipe.** `Style::glass(level, tokens)` is now the single
  definition; `Card`, `Banner`, `Toast`, `Avatar`, the `TextField` pill and the
  DSL's `.material()` all route through it. Each had hand-rolled its own
  subset before — which is exactly how `Card` ended up without a shine while
  `Stack.material(card)` had one.
- **`.grow()`/`.shrink()` never reached kit widgets.** Every kit arm built its
  own node and dropped the flex item, so a `.grow(1)` `TextField` laid out
  0px wide inside a row.
- **Localized dates.** `probe/clock.rs` hard-coded English weekday and month
  names; de/en/ja/ko/zh now each get their own names *and* their own field
  order ("Sonntag, 26. Juli" vs "Sunday, July 26"). The CJK glyphs those can
  emit are pinned into the baked charset — they live in host Rust, so the
  i18n-catalog sweep would never have seen them.

### Fixed - 2026-07-26 (two gates that were already red on main)

Found while proving the greeter, both predating it, both reported with numbers
rather than quietly patched:

- **`contract-windowd-size` had been failing on `main`.** windowd was 537 KB
  over its 8 MB budget before this branch touched anything — it links the
  ~4.4 MB shared glyph atlas, now half its image — and the gate ran only in
  `ci-os-headless`, never in `test-all`. Replaced the windowd-only tripwire
  with `contract-image-budgets`: a declared budget per service against the
  224 MB kernel VMO arena, a catch-all bound for anything unlisted,
  non-service images excluded *with a stated reason*, a usage table printed on
  every run — and wired into `just test-all`. The real fix (windowd mapping
  the shared atlas VMO the way an app-host does, RFC-0080) is recorded in
  `tasks/TASK-0076B`, not pretended away by a bigger number.
- **`just diag` had been failing on `main`**: four dead-code denials in
  windowd (`atlas.rs`, `window_scene.rs`) whose only callers live in OS-gated
  compositor code. cfg-gated per the repo rule, not blanket-allowed.

Also worth writing down: `just check` and `just test-host` do **not** compile
the OS cfg. Two breaks in this branch (`self.locale` vs `locale_tag`, an
unqualified `BakedTextMeasure`) were invisible to both and only fell out of
`just diag`.

### Changed - 2026-07-26 (the greeter, and an OS-wide glass re-tune)

- **The greeter is rebuilt to `design_handoff_os_login`**: wallpaper tint and
  vignette, date + hero clock, the active user's glass avatar with derived
  initials, a 44px glass password pill with the submit arrow inside it, the
  switcher and session actions along the bottom. It reads through a new
  on-glass token family (`onGlass*`, `glassIcon`, `glassPlaceholder`,
  `glassFocus`, `glassFill`, `wallpaperTint`, `wallpaperVignette`,
  `textShadow*`, `transparent`) because it paints on a photograph, where
  `onSurface` — tuned for a solid panel — disappears against a bright sky.
- **`svc.session.users()` returns records `{id, label}`** instead of bare
  strings, and `svc.session.active()` names the user sessiond would log in by
  default. The greeter was rendering raw user *ids* because `login` needs the
  id and the row could only carry one string. Pre-selection asks the
  authority: the DSL cannot index a list, and guessing "element zero"
  client-side is the break `docs/dev/ui/shell/session.md` forbids.
- **`glassPanel` and `glassCard` carry the handoff's login values** in both
  themes — notably a dark *tint* (`#121214@.40`) in dark mode instead of the
  white wash, which greys a photograph out rather than sinking into it. This
  changes the dock, control center, launcher and every card OS-wide; that was
  the intent.
- **`Avatar` derives initials from a display name** and honours its modifiers
  (it ignored all of them and was hard-coded to a flat `SurfaceVariant` disc).
  `GlassTextField` gains `FieldVariant::{Boxed,Glass,Bare}` + `pill()`, and
  the DSL forwards `variant`/`error`/`helper`/`size`/`disabled`.

### Fixed - 2026-07-26 (boot stuck on the splash — an aliased gpud route, and two waits that never decided)

- **The gpud route answer is now validated, not believed.** Routing v1 carries
  no reply nonce, and ~1 in 2 interactive boots came back from
  `new_for("gpud")` with a recv slot equal to **windowd's own server inbox**.
  Every gpud round-trip then read client requests instead of gpud replies: the
  reply drain refused to run (correctly, since 2026-07-25), the cursor-upload
  ack never matched (`windowd: cursor upload failed`), and the framebuffer
  handoff never acked. `ensure_gpud_client` now rejects that provably-wrong
  answer (`windowd: FAIL gpud route answered our own inbox slot=… — using the
  wired pair`) and binds the pair init declared. The query is still issued —
  its stale-`ROUTE_RSP` drain is load-bearing; an unconditional wired bind
  regressed the headless framebuffer handoff 4/4.
- **In-flight present credits are a lease, not a promise.** With no acks
  credited, `frames_in_flight` pinned at `MAX_IN_FLIGHT` and windowd stopped
  presenting after two frames. gpud's reveal gate is bounded — but it is
  evaluated *inside the present path*, so with no presents even its 1.2 s hard
  cap never ran: the splash held forever
  (`build/logs/manual--2026-07-26T10-31-30`). After
  `PRESENT_ACK_LEASE_NS` (0.5 s) without a credited ack, presenting resumes
  anyway with `windowd: FAIL present-ack lease expired — presenting without
  credits`. A stale credit costs a redundant frame; withholding presents costs
  the session.
- **The rule behind all three instances** (this, the desktop-bind deferral, the
  320x240 content-rect fallback) is now normative in
  `docs/architecture/16-rust-concurrency-model.md` §"Anti-Patterns ❌ 5": every
  wait terminates in a decision (ready or explicitly degraded), and its
  deadline must be evaluated on a path that cannot be starved by the very thing
  it bounds.
- Proof: 8/8 interactive 4-hart virgl boots reached `gpud: desktop reveal`
  (the route rejection fired in 3 of them, the lease in 1), against **3 of 6
  stuck on the splash** on the same tree without the fix. `cursor upload
  failed` and `drain skipped` are gone from all 8.

### Changed - 2026-07-25 (soft-RT: the deadline sweep no longer holds the BKL ~2 s per boot)

- **Attribution first.** All three syscalls observed holding the BKL >10 ms in a
  4-hart interactive boot (`KINIT: long ecall nr=0|18|26|40`, i.e. `yield`,
  `ipc_recv_v1/v2`, `waitset_wait`) share exactly one body: the deadline sweep
  `syscall::api::wake_expired_blocked`, which is O(tasks) and runs at EVERY
  scheduling transition. New probe (`trap::budgets::record_sweep`, reported as
  `KINIT: sweep bring-up|steady max=… tasks=… calls=… mean=… skipped=…`)
  measured it: **241,917 and 388,736 calls per boot at 6–7 µs each ≈ 1.7–2.3
  seconds of BKL held**, with 6–13 ms outliers that were the worst holds in the
  run. On a global lock, per-transition O(tasks) work *is* the throughput
  ceiling.
- **O(1) gate in front of the scan.** `trap::budgets::NEXT_DEADLINE_NS` holds
  the earliest armed deadline: `TaskTable::block_current` lowers it whenever a
  task blocks with one, and every scan that does run republishes the exact
  minimum still pending, so skipping while `now < min` cannot lose a wakeup.
  Result on the same boots: **6,251 / 3,787 scans executed vs 819,145 / 268,006
  skipped** (97–99 % gone), sweep max 13.1 ms → 4.2 ms and 6.2 ms → 1.1 ms,
  aggregate BKL time in the sweep down ~87–94 %. In the deterministic 1-hart
  lane the sweep is now 2 µs max with 87,500 skips in the steady window.
- **Not fixed by this, and now isolated:** the worst-case ~12 ms hold persists
  in `ipc_recv_v2`/`yield` with the sweep down at 1–4 ms, so the remaining
  spike is elsewhere in those syscalls (next suspects: `Scheduler::purge` —
  O(cpus × queues) on every block — and the wake IPI path). Gates green:
  `just check`, `build-kernel`, `ci-os-smp1` (`KSELFTEST: bkl budget ok`),
  `ci-os-smp` (2-hart).

### Fixed - 2026-07-25 (`just start` dead-session race — windowd was eating client frames off an aliased endpoint)

- **The desktop-bind race is a lost-frame bug, not a reordering one.** In a
  4-hart virgl `just start` (`build/logs/manual--2026-07-25T15-57-20`) windowd
  bound its gpud route via a runtime `KernelClient::new_for("gpud")` route
  query instead of the persistent pair init wires into slots 5/6. That query's
  recv side aliased windowd's OWN server endpoint, so `drain_gpud_replies`
  consumed **29 client frames** as unknown present verdicts and logged them as
  `gpud reply foreign frame op=0x49` — `0x49` is `b'I'`, the first byte of the
  client-surface envelope `[b'I', b'N', version, op]`. The casualties explain
  every symptom of that boot: the app-host's events-attach (`len=12` =
  `SURFACE_EVENTS_FRAME_LEN`) → `WINDOWD: desktop bind deferred` with no
  attach left to complete it; its geometry intent → the 8 s content-rect
  timeout at 2.4 s + 8 s = the `desktop surface created id=1 320x240` observed
  at 11.2 s; inputd's batches (`len=32`) → the `push backpressure` flood,
  `hidrawd: chain I2 wire send FAIL` and a desktop that never logged
  `desktop input routed`. The healthy boot 70 s later had **0** such drops.
- **The drain now only reads an endpoint that is exclusively windowd's.**
  `compositor::run` publishes windowd's own server recv slot
  (`KernelServer::slots().0`) before anything can present, and
  `drain_gpud_replies` refuses to consume from a gpud route whose recv slot IS
  that inbox (one-shot `windowd: FAIL gpud reply endpoint aliases server inbox
  slot=… — drain skipped`). Skipping the drain only stops crediting present
  completions — the in-flight bound throttles presents — whereas consuming a
  client request loses it forever, because the capability an events-attach
  carries cannot be re-delivered. Route SELECTION is deliberately unchanged:
  binding the runtime to the init-wired pair instead broke the framebuffer
  handoff in the headless lane (`gpud: ERROR attach framebuffer resource
  create failed` → `chain G4 scanout FAIL`, reproduced 4/4), so the fix stays
  on the drain, which is where the defect was.
- **Belt-and-braces + host test.** `nexus-display-proto` gained
  `envelope::is_client_envelope()` (the envelope moved into its own module,
  shrinking the 978-LOC `client_surface.rs`; covered by
  `tests/client_envelope.rs` against every client op and every gpud status
  byte). The drain uses it as a second line of defence: a client frame that
  reaches it anyway stops the drain at once with `windowd: FAIL gpud reply ate
  client frame op=… len=…` instead of eating a stream.
- **No more probe-size desktop.** `request_content_rect` re-drives the intent
  once (a lost ask is indistinguishable from a slow WM until the second one
  also goes unanswered), and app-host now treats "no content rect" for a
  compositor-owned surface (desktop/overlay/fullscreen) as a hard failure —
  `APPHOST: FAIL no content rect …` + `exit(-1)` — instead of mounting a
  320x240 "desktop" that presents frames and never routes input.

### Fixed - 2026-07-25 (the "deterministic" boot gate was the MTTCG lane — flaky `runtime timer budget ok`)

- **`just ci-os-smp1` now really runs one hart with icount.** It passed `SMP=1`
  to `--profile=smp`, but that profile declares
  `env = { REQUIRE_SMP = "1", SMP = "2", QEMU_NO_ICOUNT = "1" }` and
  `pm_apply_profile_env` overrode the caller silently — so the hard `test-all`
  boot gate was the 2-hart MTTCG lane (every run dir records `"smp":"2"`,
  `"require_smp":"1"`, empty `qemu_icount_args`). That is where the flaky
  `KSELFTEST: runtime timer budget ok` came from: it is a *secondary-hart*
  proof, and MTTCG hart bring-up is nondeterministic — two runs on 2026-07-25
  lost cpu1 outright (`cpu1_online=0`, `tlb shootdown skipped (smp=1)`), one
  brought cpu1 up but never reached the tick proof. The `make test` "SMP=1
  parity" run had the same defect, making it a duplicate of the strict run.
- **The lane is now a declaration, not an env tweak.** New
  `[profile.smp1]` (extends `headless`, `env = { SMP = "1", QEMU_NO_ICOUNT =
  "0" }`, no `REQUIRE_SMP`) in the proof-manifest owns the deterministic
  topology; `ci-os-smp1`, `make test`'s parity run, and the docs select it by
  name and pass no topology env at all. Because it extends `headless`, the
  one-hart gate now verifies the **full** userspace ladder (init → services →
  `SELFTEST: *`) instead of the 10-marker SMP subset, with
  `KSELFTEST: tlb shootdown skipped (smp=1)` as the explicit "no secondary
  hart" line.
- **The contradiction can't come back.** A caller env value that contradicts a
  manifest-declared profile key is a hard error before any build or boot
  (`NEXUS_PROFILE_ENV_OVERRIDE=1` = loud local escape hatch), redundant
  topology env was removed from `ci-os-smp`, and two host tests pin the pair:
  `accept_boot_lane_topology_is_declared` plus
  `reject_require_smp_without_two_harts_on_disk` (any lane demanding
  secondary-hart proofs must declare `SMP >= 2`). Also dropped the floating
  `cargo +stable` used to build the manifest CLI inside the harness — one
  pinned toolchain, per CLAUDE.md.

### Fixed - 2026-07-24 (Settings app couldn't reach settingsd — atlas slot collision)

- **Toggling a setting in the Settings app now applies.** RFC-0080 granted the
  shared glyph-atlas VMO into app-host child slot **15**, but that is exactly the
  `nexus-sdk-routes` `child_slot` of the `settings` route — so execd's settingsd
  grant to the Settings app failed (`execd: FAIL app route grant svc=settings`)
  and every toggle died with `apphost: dsl svc settings.set FAIL (settingsd
  unreachable)`. The atlas VMO moved to slot **19** (clear of the route range
  11..=18) in execd (`CHILD_ATLAS_VMO_SLOT`) + app-host (`ATLAS_VMO_SLOT`); the
  atlas still maps (boot-proven), and the settings route now gets slot 15.

### Added - 2026-07-24 ("Forget learned words" — RFC-0075 Phase 4, TASK-0204)

- **A "Forget learned words" button** in Settings → General management clears the
  IME's learned personalization. Implemented as a one-shot value on the existing
  key: the button sets `ime.personalization = "forget"`; on the next focus imed
  clears its store, re-enables learning, truncates the on-disk blob, and writes
  the key back to `on` (so it never rests at `forget`). settingsd's validator
  accepts `on`/`off`/`forget`. Proof: imed host tests 16/16
  (`forget_clears_learned_words`); `ci-os-smp1` green.

### Added - 2026-07-24 (IME personalization is toggleable — RFC-0075 Phase 4, TASK-0204 complete)

- **"Adaptive suggestions" On/Off toggle** in Settings → General management,
  completing the personalizable IME (RFC-0075 Phase 4 / TASK-0204).
  - imed reads `ime.personalization` (settingsd `OP_GET` over its existing
    settings route, on focus-gain, 30 ms bounded so it never blocks the serve
    loop) and applies it via `store.set_enabled`. Off = no reads, no writes, no
    learning, and drops any in-memory learning immediately; a transient GET miss
    keeps the current state (never a silent flip).
  - Settings UI: the toggle dispatches `SetPersonalization` →
    `svc.settings.set("ime.personalization", …)`; i18n in all 5 catalogs.
  - Proof: imed host tests 15/15 (incl. `toggle_off_disables_learning`);
    `ci-os-smp1` green (9/9, every ime marker unchanged). imed's unit tests were
    split to `imed/src/tests.rs` (module-size ratchet). "Forget learned words"
    UI is a noted follow-up (`ImedCore::forget_learned` exists; needs a
    Settings→imed command path).

### Added - 2026-07-24 (IME candidates adapt to use — RFC-0075 Phase 4, TASK-0204 2b-live)

- **imed now learns from commits and reranks the candidate strip.** `ImedCore`
  owns a per-locale `PersistentStore`:
  - **train**: the single commit choke point (`plan()`, for engine-handled
    commits — CJK candidate selects, dead-key composes) bumps the committed
    candidate's frequency + the `(prev, cand)` bigram + recency. NEVER for
    password fields (the password bypass never reaches `plan()`).
  - **rank**: `plan()` reranks the visible candidate page via the store
    (`ime-core CandidatePage::reordered`) — the user's frequent/recent/in-context
    picks surface first; untrained candidates keep the engine's table order (so
    `SELFTEST: ime v2 candidates ok` is unchanged).
  - **persist**: `os_lite` loads the locale blob on layout switch
    (`core.load_store`) and flushes on focus loss (`core.flush_store`,
    coalesced), both over the statefsd `StatefsBlobIo`.
  - Proof: imed host tests 14/14 (`text_field_commit_trains`,
    `password_field_never_trains`, `learned_words_persist_across_reload`);
    `ci-os-smp1` green with every ime marker unchanged (no regression). The
    visible reordering is an interactive proof (`just start`).
  - Next (2b-Settings): the `ime.personalization` toggle + "forget" UI.

### Added - 2026-07-24 (IME personalization persists through statefsd — RFC-0075 Phase 4, TASK-0204 2b-substrate)

- **`SELFTEST: ime ranking persist ok`** — imed now persists its adaptive-ranking
  store through the REAL statefsd route, end to end:
  - **init route** (`provision_imed_legs`): imed gets a SEND clone of statefsd's
    request endpoint (pinned slot 0x0B) + a private CAP_MOVE reply inbox (0x0C/
    0x0D) — the same imperative recipe as its settingsd leg, no fleet collapse
    (`init: imed route->statefsd ok`).
  - **`imed/src/statefs.rs`**: a `StatefsBlobIo` (ime-ranker `BlobIo`) over the
    pinned slots — statefsd v1 GET/PUT, bounded + fail-closed (a statefs miss
    degrades to no-persist, never a boot hang).
  - **policy** (`policies/base.toml`): `imed` granted `statefs.read` +
    `statefs.write` (deny-by-default; imed persists only committed candidates
    under `state:/ime/…`, never typed field text).
  - **proof**: imed round-trips a trained fixture at boot (PUT → GET → ranking
    preserved), the marker emitted RAW so it can't be lost post-verdict-flush.
    `ci-os-smp1` green (9/9 chain markers unchanged).
  - Next (2b-live): train-on-commit + rank-the-strip + flush-on-focus-loss +
    the Settings toggle/forget UI.

### Added - 2026-07-24 (IME ranking runs in the OS — RFC-0075 Phase 4, TASK-0204 Package 2a)

- **`SELFTEST: ime ranking ok`** — selftest-client now depends on `ime-ranker`
  and drives a pure-crate probe in the bringup ladder: one commit lifts a
  table-last candidate to the front, and that learned order survives an NDJSON
  export→import (the shape the statefs load path reconstructs). Proves the
  deterministic ranker runs correctly under the REAL service allocator
  (no_std + alloc), not just on host. Green in `ci-os-smp1` (9/9 chain markers
  unchanged); marker registered in the proof-manifest + qemu-test.sh sequence.
  Chosen as the first OS slice because it needs no init wiring (the
  imed→statefsd route is delicate) and no live candidate-flow surgery — those
  land in 2b with the real statefs round-trip (`SELFTEST: ime ranking persist ok`).

### Added - 2026-07-24 (IME personalization persistence binding — RFC-0075 Phase 4, TASK-0204 Package 1)

- **`ime-ranker` persistence binding** (`persist.rs`): the host-testable half of
  wiring the ranking store to disk, storage-agnostic behind a `BlobIo` trait
  (imed will back it with statefsd `state:/ime/<lang>/…`; tests use a fake map).
  - `PersistentStore` wraps `MemStore` with a dirty flag and the
    `ime.personalization` on/off gate. One NDJSON blob per locale holds dict +
    bigrams (self-describing format → one file, not two).
  - **Coalesced write-back**: `train` only marks dirty; `flush` writes once, and
    only when dirty + enabled + under `BLOB_MAX` (64 KiB) — never per-keystroke.
  - **Fail-closed load**: a missing / oversized / non-UTF-8 / bad-header blob
    yields an EMPTY store, never a failure (reuses the TASK-0203 reject matrix on
    the read buffer).
  - **Toggle gate**: off means no reads, no writes, no learning; `set_enabled`
    off drops in-memory learning immediately; `forget_all` clears + truncates.
  - Proof: `cargo test -p ime-ranker` 23/23 (round-trip preserves ranking across
    reload, coalesced flush, toggle-off does zero IO, disabling drops learning,
    corrupt/oversize load empty, forget truncates); `just check` + `test-host`
    green. Package 2 binds it into imed + Settings + selftest.

### Added - 2026-07-24 (IME adaptive ranking + NDJSON — RFC-0075 Phase 4, TASK-0203 complete)

- **`userspace/ime-ranker`** (new, no_std-capable, zero deps): the deterministic
  personalization layer that reorders an engine's table-order candidates by what
  the user actually picks — without ever becoming nondeterministic or unbounded.
  - Fixed-point **Q8.8** scoring (`score.rs`): four saturating signals — personal
    frequency, in-context `(prev, cand)` bigram, a mild length prior, and a
    coarse recency bucket (a caller-supplied monotonic counter, never a raw
    timestamp). No floats, no RNG → bit-reproducible.
  - `train`/`rank` (`lib.rs`): `train` bumps freq + bigram + recency from a
    committed candidate (never raw field text; password fields never call it);
    `rank` returns a permutation with stable tie-breakers (score desc → engine
    table index → candidate bytes), so untrained candidates keep table order and
    a trained one overtakes it.
  - `PersonalStore` trait + `MemStore` reference impl (`store.rs`):
    storage-agnostic (TASK-0204 binds it to statefsd), deterministic sorted
    iteration, bounded per-locale quota (≤4096) with deterministic
    least-valuable-first eviction; `forget` erases a candidate and every bigram
    that referenced it.
  - NDJSON interchange (`ndjson.rs`): `export_ndjson`/`import_ndjson` written
    once over the trait — a versioned header then one JSON line per record with
    hex-encoded candidate bytes (pure ASCII → exact length bound, no CJK
    escaping). A byte-identical round trip; import is fail-closed (bound the
    line before parsing, skip malformed/oversized with a capped error count,
    reject a bad-version header outright, enforce the quota via eviction).
  - Proof: `cargo test -p ime-ranker` 16/16 (overtake, in-context bigram,
    recency, determinism, fail-closed oversized candidate, eviction bound +
    determinism, NDJSON round-trip + reject matrix + quota); `just check` +
    `test-host` green. Next: TASK-0204 binds the store to statefsd + Settings UI.

### Fixed - 2026-07-24 (Shared-AS reap ordering — TASK-0304 Part 1)

- **A reaped thread no longer logs a spurious `TASK: destroy as failed InUse`.**
  `reap_child` used to unconditionally `destroy` the reaped task's address space;
  for a thread that shares its AS with a still-living parent (and parked worker
  threads), `destroy` correctly refused with `InUse` but the refusal was logged
  as an error. It now destroys the shared AS only when the reaped task was its
  **last** owner (a returned `InUse` is accepted silently — a co-owner is still
  alive, and the AS is reclaimed when that last owner is reaped). Investigation
  confirmed this was never a leak: `detach` releases the reaped thread's
  reference, and the AS is reference-counted by its owner set. Active teardown of
  *parked daemon* worker threads when their owning service exits stays deferred
  (TASK-0304 Part 2 — needs cross-hart quiesce; no shipping service needs it).
  Proof: `ci-os-smp1` green; the `InUse` line is gone; `SELFTEST: thread spawn
  ok` / `workpool bounded ok` still green.

### Added - 2026-07-24 (Process reaper — service-driven zombie reclaim — RFC-0081)

#### Repeated app launches no longer exhaust the kernel heap

- **execd is now the reaper-of-record.** Since RFC-0079 a closed-window app-host
  self-exits, but nothing reaped it: `execd` only waited on an explicit
  `OP_WAIT_PID`, and no client waits for a fire-and-forget window. The exited
  task stayed a Zombie, so its heap-backed page-table address space was never
  destroyed — a handful of launches filled the 8 MiB kernel heap and PANICked
  (`ALLOC-FAIL`, the heap had been bumped 2→8 MiB purely as a bridge).
- **`SYSCALL_WAIT_NOHANG` (52)** — a non-blocking sibling of `wait`: reaps one
  ready zombie child (`reap_child` → detach + destroy AS) or returns `0` instead
  of blocking. `nexus-abi::wait_nohang() -> Option<(Pid, i32)>`.
- **execd sweeps its exited children each serve-loop iteration** (before handling
  the request), so a new spawn first reclaims prior closures — the arena and heap
  never accumulate across launches. Exit codes are cached on the child record so
  an `OP_WAIT_PID` client still gets its status after the sweep reaped the child;
  crash reporting is factored into an idempotent `handle_child_exit` (runs once,
  no double-report).
- Proof: `ci-os-smp1` green (9/9); the boot log shows `execd: reaped pid=44
  code=0` firing exactly at the greeter launch (reclaim-before-spawn), no
  `ALLOC-FAIL` / `VMO-POOL exhausted`. The reap→free invariant is pinned by the
  existing target `address_space::tests`. The `8 MiB` heap stays as conservative
  desktop headroom (no longer a bridge).
- Refactor (module-size ratchet): execd's pure crash-field byte serializers
  moved to `execd/src/crash_fields.rs` (`os_lite.rs` back under baseline).
- Not in scope: terminating sibling **threads** that share an AS on process exit
  (the `destroy as failed InUse` seen only for the kernel workpool selftest) —
  needs race-free cross-hart teardown, seeded as TASK-0304.

### Added - 2026-07-23 (RO-only VMO right — RFC-0080 hardening)

#### The shared atlas is now write-protected by the kernel, not by convention

- **`CapabilityKind::VmoRo`** — a READ-ONLY alias of a VMO's physical pages,
  derived from a `Vmo` via the new `vmo_share_readonly` syscall (51). It is a
  *contained* addition: existing `Vmo` caps behave exactly as before, so there
  is zero audit surface on the write path. `sys_map` runs a `VmoRo` cap's
  requested page flags through `vmo_ro::force_readonly` (WRITE|EXECUTE are
  ALWAYS cleared, VALID|READ|USER always set) before installing the mapping, and
  `vmo_write` rejects `VmoRo` — a holder can never obtain a writable or
  executable view, even if it asks.
- **execd drops the writable cap.** After filling the shared glyph atlas, execd
  derives the RO alias and closes the writable one, keeping only the alias. It
  therefore grants RO-only clones to every app-host, and not even execd can
  corrupt the atlas after fill. Closes the RFC-0080 "compromised app-host
  runtime" corruption vector.
- The RO-forcing bit function lives in the pure, host-tested `vmo_ro` module
  (like `waitset`/`fence`/`image_allocs`/`ipc_eof`), kept out of the
  target-gated `mm`/`syscall` tree so its security invariant is unit-tested.
- Refactor (module-size ratchet): the kernel-managed user VMO arena
  (`VMO_POOL`/`VmoPool`) moved from `syscall/api/vmo.rs` (987 → 648 LOC) into a
  new `syscall/api/vmo_pool.rs`; behavior unchanged (pure code move).
- Proof: `neuron` host tests (`vmo_ro` truth table, 2/2, + 29 total green);
  `ci-os-smp1` atlas-RO chain (`execd: atlas vmo ready/granted` →
  `APPHOST: atlas mapped`, Latin+CJK render, no `VMO-POOL exhausted`).

### Added - 2026-07-23 (shared glyph-atlas RO VMO — RFC-0080 Phase 1)

#### Opening N app windows adds ~0 atlas bytes

- **The atlas is ONE shared read-only copy now.** execd creates + fills a single
  glyph-atlas VMO from the embedded blob at startup (`execd: atlas vmo ready`)
  and RO-clone-grants it into every app-host's fixed slot before resume;
  app-host `vmo_map_page`s it read-only at `0x3000_0000` and installs it as the
  text atlas base (`APPHOST: atlas mapped`). The physical pages are shared, so
  N open windows share ONE copy instead of `exec` copying ~4.25 MB per launch.
- **app-host no longer embeds the blob** (its ELF shrank 5.9 → 1.66 MB, −4.25 MB):
  its `nexus-text-baked` (and dsl-runtime's — layout only measures, never draws,
  so it needs no coverage) build with `embedded-atlas` OFF. windowd keeps its
  embed (single instance). Because scripts/build.sh builds each service
  separately, the feature split holds without unification.
- **Fail-visible, never fake**: a missing/failed atlas VMO makes text render
  blank (empty coverage), never garbage or a crash; markers record it.
- Proof: visible boot renders Latin + CJK from the shared VMO; a 5-launch
  open/close storm adds ~0 atlas bytes with zero `VMO-POOL exhausted`;
  `ci-os-smp1` green with the atlas markers. `execd`/`app-host` went
  `forbid`→`deny(unsafe_code)` for the one mapped-pointer install; the atlas
  VMO logic lives in `execd/src/atlas_vmo.rs`.
- Follow-up: a RO-only VMO right (so the MAP cap can't be mapped WRITE) — DONE,
  see the RFC-0080 hardening entry above. A kernel static-VMO zero-copy variant
  (no owner copy at all) remains deferred; the 224 MB arena / 8 MiB heap bridges
  are intentionally left as-is (desktop system, conservative headroom).

### Changed - 2026-07-23 (nexus-text-baked: runtime atlas base — RFC-0080 Phase 0)

#### Foundation for sharing the glyph atlas as ONE read-only VMO

- `nexus-text-baked` now resolves each face's coverage through a process-global
  **atlas base** (`AtomicPtr` + a compile-time per-face `(offset, len)` into ONE
  concatenated `font13 ++ font16` blob) instead of two `&'static` `include_bytes`
  slices. `set_atlas_base(ptr, len)` installs a mapped read-only VMO; without it
  the `embedded-atlas` feature (default ON) lazily backs the base with the
  linked blob, so windowd/host/tests stay byte-identical with zero setup.
- This is the library half of RFC-0080: it lets a consumer (app-host) drop the
  ~4.25 MB blob from its image and back the atlas with a shared VMO, killing the
  per-instance duplication that `exec` copies on every window launch (the
  pressure behind the 160→224 MB arena + 2→8 MiB heap bridges). The VMO
  provisioning (execd owns + RO-clone-grants; app-host maps) is Phase 1.
- Proof: a host golden installs the base at a heap copy and renders
  byte-identically to the embedded path; `ci-os-smp1` green (all consumers keep
  the embedded blob). Because scripts/build.sh builds each service with its own
  `cargo build -p …`, Phase 1 can flip app-host to the VMO without feature
  unification pulling the blob back in.

### Added - 2026-07-23 (kernel, IPC last-sender EOF — RFC-0079)

#### Closing a window now terminates the app process (arena reclaim fires)

- **The other half of the leak fix**: RFC-0075 8f reclaims a process image
  when a task EXITS, but a closed app WINDOW never exited its process — the
  app-host parked forever on its event-channel `recv` (windowd closes its
  SEND cap on window-close, but the endpoint stays alive). Each launch spawned
  a fresh ~14 MB app-host; none were ever reaped.
- **IPC last-sender EOF (opt-in)**: a new `IPC_SYS_EOF` recv flag makes a
  blocking `recv` return `PeerClosed` (errno EPIPE → `IpcError::Disconnected`)
  when the endpoint HAD a sender and its last SEND cap has closed, instead of
  blocking. Fail-safe by construction — EOF requires BOTH a monotonic
  per-endpoint `had_sender` latch AND a fresh all-tables scan proving zero
  live SEND caps (`endpoint_send_cap_count`, mirroring `vmo_overlap_count`).
  A recv WITHOUT the flag is never affected, so every server that blocks
  before its first client is safe. `cap_close` of the last SEND cap wakes the
  endpoint's recv-waiters so a blocked receiver returns promptly.
- **app-host self-exits on window close**: its event loop opts into EOF and
  exits on `Disconnected` (a process may always exit ITSELF — no cross-task
  kill, no policyd gate; the reaper #29 stays a separate design). The exit
  runs the RFC-0075 8f reclaim, closing the loop.
- **Two cap-leak fixes were required** for the last-sender scan to reach zero:
  execd's `grant_clone` now closes its COPIED grant clone after transferring
  it (`cap_transfer_to_slot` copies — execd was retaining a live SEND cap to
  every child's event channel, a per-launch leak), and app-host closes its own
  leftover SEND clone after attaching it to windowd.
- **Marker atomicity (infra)**: `selftest-client`'s `emit_line` wrote markers
  byte-by-byte via `debug_putc`, leaving a per-byte window that merged
  concurrent SMP markers (e.g. a `SELFTEST:` line spliced with
  `inputd: keymap set …`) and tripped the evidence assembler. It now emits the
  whole line in one `debug_write` (serialized under the kernel UART lock).
- **Refactors** (module ratchet): `ipc/endpoint.rs` (the `Endpoint` struct out
  of `ipc/mod.rs`) and `syscall/api/ipc_recv_v2.rs` (recv-v2 out of `ipc_msg`);
  the pure decision lives in the host-tested `ipc_eof.rs`.
- Proof: `ipc_eof::should_disconnect` host reject-matrix; `ci-os-smp1` green
  (every non-opted server recv still blocks); visible boot — each window close
  yields exactly one app exit (1:1) and an open/close storm keeps the arena
  bounded (zero `VMO-POOL exhausted`).

### Fixed - 2026-07-23 (kernel, process-image arena reclaim)

#### Process images return to the VMO arena on task exit (RFC-0075 8e)

- **Root cause of the arena exhaustion**: `exec` allocated every PT_LOAD
  segment, the user stack and the bootstrap meta/info pages from `VMO_POOL`
  but NOTHING ever freed them — the arena was bump-only for process images.
  Each launch consumed ~14 MB (a service ELF's RO segment alone is ~5.5 MB
  with the CJK atlases) permanently; the earlier 160→224 MB arena bump was a
  bridge over this leak.
- **The fix**: each task now records the arena ranges backing its image
  (`ImageAllocs`, a fixed-capacity per-task record — no kernel-heap churn);
  `exec` fills it, and `exit_current` returns it. A single teardown funnel
  (`exit_current_and_release`) covers `sys_exit` AND the trap handler's four
  fault-exit paths, so no exit can forget the memory. `free()` validates
  bounds + overlap (a wrong range is logged, never blindly freed); the
  exec_v2 failure path also returns its partial allocations instead of
  leaking them.
- **Honest scope**: this reclaims memory only when a task actually EXITS.
  Closing an app WINDOW today does not terminate the app process — it parks
  (the "open reaper", follow-up #29) — so the user's open/close storm still
  needs #29 (or IPC last-sender-EOF so app-host self-exits) to trigger this
  reclaim. This change is the mechanism that half needs; the 8 MiB kernel
  heap + 224 MB arena bridges stay until #29 lands.
- Refactor (module-size ratchet): the exec stack-mapping logic (identical in
  `sys_exec`/`exec_v2`) is now one `map_process_stack` helper, and the legacy
  guarded-stack allocator moved to `task/stack_pool.rs`.
- Proof: `ImageAllocs` host unit tests (record/drain/overflow) + `ci-os-smp1`
  green (real `e2e exec-elf` child exit runs the reclaim path; zero
  `IMAGE-RECLAIM incomplete`, zero pool corruption).

### Fixed - 2026-07-22 (evening, input-UX hardening II)

#### IME v2 follow-through: crash fix, typing resilience, caret, I-beam, remount locale

- **Kernel heap 2→8 MiB (crash fix)**: page tables are heap-backed and every
  app now maps a larger image (CJK atlases) — a live session PANICked
  (`ALLOC-FAIL`, heap full at ~20 address spaces) when the OSK launched as
  the 6th app. Dead apps also keep their address space until a parent WAITs
  (zombie reap = follow-up #29; a failing `destroy as failed err=InUse` on
  the reap path is recorded) — the grown heap is the bridge, the reaper is
  the fix.
- **Fast typing no longer loses input**: inputd aborted the WHOLE keyboard
  batch (`STATUS_OVERFLOW`) on any per-event error — one Ctrl chord,
  unmapped usage, or non-monotonic hidraw timestamp silently ate every
  other key of the batch (fast typing packs several keys per chunked
  drain). Per-event resilience now: an unproducible key skips ITS event,
  repeat arming is best-effort. New host test `keyboard_batch.rs`
  (rollover, chord-skip, timestamp-regress survival); 5/5-key burst proven
  live in greeter and chat composer.
- **Text caret (v1)**: the focused TextField paints a 2-px bar after its
  content on both render paths (plain + banded) — append-only caret model;
  blink needs a frame pulse and stays a recorded follow-up. Empty fields
  keep an anchor run so the caret shows before the first character.
- **I-beam cursor over editable fields**: new `OP_SURFACE_CURSOR_HINT=25`
  (app → windowd, semantic shape ids, fail-closed decode + tests) — the
  app owns hover semantics inside its surface and sends hints only on
  enter/leave; windowd arms the vendored theme's `text.svg` I-beam (new
  shape-cache slot 5, ring slots moved to 6+) for app windows AND the
  desktop surface (greeter/shell).
- **Mode/theme switches keep the language (the "tablet switch broke my
  language" report)**: profile/theme remounts rebuild the DslApp from the
  payload at the BAKED catalog — the last-applied region (locale/tz/keymap)
  is now remembered and re-applied after every remount. This also restores
  the OSK rows after a mode switch (the keymap axis survives, the
  `KeymapEvent` reload fires again); live-proven: control center stays
  German after desktop→tablet, OSK keeps full rows at the session composer.
- Composer latency note: each commit still re-emits the whole banded scene
  (~texts=125) — keys are never lost now, but fast bursts render with lag;
  the per-commit re-emit ceiling is the known WebRender-stage follow-up.

### Added - 2026-07-22 (morning, 8c/8d)

#### IME v2 Phases 8c+8d (RFC-0075): input UX hardening + CJK font foundation

- **TextFields finally PAINT their text** (root cause, not a patch): the
  store/insert side was always wired (compiler-synthesized `Change → Bind`,
  `write_binding`, re-emit) — but `collect_texts` only harvested
  `LayoutNode::Text`, so no TextField ever painted content OR placeholder.
  New `TextInput` arm paints the content (bullet-masked when `secure`) or
  the dimmed placeholder (~55 %); caret painting = recorded follow-up.
  The greeter password field is now `secure: true` (was about to echo
  plaintext the moment painting worked).
- **OSK dismiss + tablet-only (user decisions)**: an `X` key in the action
  row dismisses the OSK via the existing `window.control` path — windowd
  treats minimize on `WIN_LEVEL_OVERLAY` as a dismiss latch
  (`osk_dismissed`, cleared on the next focused=1 announce), and app-host
  re-announces focus when an already-focused field is tapped again so the
  next tap reopens it. `want_osk` is gated to
  `shell_profile_wire() != PROFILE_DESKTOP` (revert of the 8-phase
  every-profile choice — profile = OSK policy).
- **CJK glyphs are REAL now (8d)**: `nexus-text-baked` bakes multi-face
  A8 atlases — Inter for Latin + the PINNED Noto Sans CJK faces per script
  (JP kana/punctuation, KR compat jamo + the FULL hangul syllable block —
  typing composes arbitrary syllables, SC han). The WIDE tail is bounded
  by construction: fixed ranges + the han actually used (extracted from
  every app i18n catalog + the IME engines' output tables + OSK labels)
  + the secure-field bullet `•`; misses still render an honest `?`.
  Fonts arrive via `scripts/fetch-fonts.sh` (pinned commit + SHA256 —
  the noto-cjk repo is too large for a submodule); OTFs are build inputs
  only, never image payload.
- **Kernel memory layout followed the image** (~+8 MB atlases): init-lite
  RAM window 8M→24M, kernel page pool 0x8200_0000/24M, user VMO arena
  0x8380_0000 and grown 160→224 MB — the atlases ride in EVERY app-host
  instance and a logged-in session exhausted 160 MB (silent app-death
  failure mode). Follow-up recorded (now load-bearing): share ONE atlas
  via RO VMO instead of per-instance duplicates.
- **Settings offers 日本語/한국어/中文** language chips (natively labeled,
  readable now that the glyphs exist) → `ui.locale` ja-JP/ko-KR/zh-CN.
- **Boot-race mitigation (pulled forward)**: windowd parks composed
  intent replies until the app's event channel attaches
  (`pending_intent_replies`, flushed on attach) and the app-side
  content-rect + payload budgets are 8 s (the grown image lengthened
  early-boot drains); the deep cause (windowd lagging seconds at early
  boot) stays a recorded follow-up.
- **Launched-window locale gap CLOSED (found on the way)**: the attach
  burst is theme+profile+REGION, but `wait_for_boot_pushes` returns on
  profile — a NORMAL window left the region frame queued and the
  create/present ack-wait stashes were never applied, so launched apps
  (chat) painted their baked-default English. app-host now drains the
  queued region non-blocking after mount, before the first render.

### Added - 2026-07-22 (night, 8b)

#### IME v2 Phase 8b (RFC-0075): data-driven OSK layouts + region env axes

- **180 languages ≠ 180 `if` trees**: the OSK rows are DATA —
  `keymaps::osk_rows(LayoutId, row) → &[OskKey{label,key,action}]` is the
  layout SSOT (KR shows jamo labels over the 2-set Latin keys it
  dispatches; jp/zh share the us rows), served to the app as
  `svc.ime.rows(layout, row) → List<OskKey>` (app-host answers natively).
  The ime-ui renders four `List(...)` templates (the launcher-grid
  mechanism — a new KeyRow primitive was evaluated and REJECTED as
  accidental complexity); its per-layout view branches are gone.
- **`device.locale` / `device.keymap` env axes** (DEVICE_FIELDS rows 7/8,
  runtime-varying `FixtureEnv` String fields): string-equality arms for the
  RARE structural cases, re-selected on reemit like a size-class change.
  `OP_SURFACE_REGION` carries the keymap tag as an OPTIONAL trailing field
  (old frames decode with an empty tag); windowd holds a third watch
  subscription (`input.keymap`, second cloned push cap).
- **Globe = system-wide layout switch (user decision)**:
  `svc.ime.cycle(current)` (cycle order = platform data) → imed sets the
  engine AND persists `input.keymap` via a new settingsd route (init-wired
  slots 8/9/10, private reply inbox, mint→grant per request; cycle guard —
  the inputd relay of the same tag never re-writes). settingsd stays the
  SSOT: Settings picker, hardware keymap, engine and every OSK follow.
  `KeymapEvent::Changed(tag)` (region-push driven) reloads the rows.
- **Fix found on the way**: the create/present ack waits (`recv_ack`)
  DROPPED the attach-time region push for LAUNCHED window apps — the
  chat-app "English despite de-DE" gap. All three pre-loop drains now
  stash region pushes; the recreate path applies them.
- Splits: dsl-runtime `fixture_env.rs`.

### Added - 2026-07-22 (night)

#### IME v2 Phase 3 (RFC-0075, TASK-0150): candidate strip + CJK OSK, OS

- **imed hosts the engines**: `ImedCore` now drives `ime_core::Engine`
  (enum dispatch) — new semantics: COMPOSITION is focus-independent (the
  deterministic probes exercise the real engine without a field), DELIVERY
  stays focus-gated, and PASSWORD fields bypass the engine entirely (raw
  commits, no preedit/candidates/learning — fail-closed in the core).
  `CommitText` widened to the 64-B `TextRun` bound (CJK candidate commits).
- **Wire (additive)**: `OP_SET_LAYOUT=8` (inputd relays applied
  `input.keymap` changes on the main endpoint; the OSK globe sends it on
  the capability-gated osk endpoint) + the osk-endpoint reply now echoes
  the step's COMMIT to the injecting sender only (probe observability;
  ime-ui sends fire-and-forget and never gets an echo).
- **Strip data path**: imed pushes `OP_PREEDIT`/`OP_CANDIDATES` (bounded
  ≤ 8 × 32 B) → windowd relays to the ime-ui overlay as the new
  `OP_SURFACE_IME_STATE=24` (preedit / candidate-page kinds) — never to
  the focused app; app-host dispatches `ImeStripEvent::Preedit/Cands`.
- **ime-ui**: composition strip row (preedit + up to 8 tappable candidate
  chips → `svc.ime.select`), layout cycle de → us → jp → kr → zh on the
  globe key (`svc.ime.layout` retargets the engine; SetLayout clears the
  strip), KR rows show 2-set jamo labels (dispatching the Latin keys the
  engine maps), jp/zh ride the us rows (romaji/pinyin).
- **Proofs**: `SELFTEST: ime v2 cjk jp ok` (layout jp + `nn`+Enter echoes
  ん through the REAL service path) and `SELFTEST: ime v2 candidates ok`
  (pinyin `nihao` + space + select(0) commits 你好) green in `ci-os-smp1`;
  interactive: chat-composer focus → OSK → globe to jp → romaji preedit in
  the strip → candidate tap → `apphost: text commit applied` + strip clear.
- **ja/ko/zh catalogs for every `@t()` app** (user request): chat,
  desktop-shell, greeter, ime-ui, settings, stash — full key parity with
  de/en; `ui.locale` `ja-JP`/`ko-KR`/`zh-CN` selects them by primary subtag.
- **Known gap (recorded)**: the UI font has NO CJK glyph coverage — kana/
  hangul/han render as `?`. The byte path is proven (probes + markers);
  glyph coverage is a font task (same class as the `⌫`/`🌐` gaps).
- OSK shows in EVERY shell profile now (profile = layout, not keyboard
  presence; HID-presence-based hiding is a recorded follow-up); band 312 px.

### Added - 2026-07-22 (later)

#### Full de/en catalogs for every DSL app (RFC-0077 follow-through)

- Every `@t()`-using app now ships BOTH `i18n/en.json` (the baked default)
  and `i18n/de.json` with full key parity: **stash**'s baked default was
  authored in German — it moved verbatim to `de.json` and `en.json` is now
  real English (sidebar labels are catalog keys; the on-disk folder names
  `/Bilder` … are paths, not labels, and stay); **chat** got its German
  catalog and consistent English fixtures. greeter, desktop-shell,
  settings, ime-ui were already complete. `calculator` uses no `@t()`
  (digits/operators) and `window-kit` is a library whose `win.*` keys
  resolve in the consuming app's catalogs — neither needs own files.


#### IME v2 CJK engines, host (TASK-0149 Done)

- **`ImeEngine` trait + `Engine` enum-dispatch** in `userspace/ime-core`
  (no_std, alloc-free): one deterministic composition contract for Latin
  (the Phase-0 composer, adapted), **JP** (romaji→kana longest-match with
  っ sokuon + ん rules and a const kana→kanji lexicon; trailing lone `n`
  resolves to ん on the final commit; the kana reading is always the last
  candidate), **KR** (2-set dubeolsik: Latin→jamo, Unicode syllable
  algebra, compound medials/finals, jong-steal, jamo-splitting backspace)
  and **ZH** (pinyin exact-buffer lookup with paging). All outputs bounded
  (preedit ≤ 64 B, candidates ≤ 8 × 32 B/page); `EngineId::for_layout`
  follows `input.keymap` (unknown → Latin, fail-open).
- **Bounded user-dict API** (`UserDict<N>`, default 1024/lang):
  `train`/`lookup`/`forget` with frequency ranking, insertion-order
  tie-breaks and lowest-freq-oldest-first eviction — deterministic;
  storage + adaptive ranking land with TASK-0203/0204.
- **Proofs**: 12 host goldens (`tests/cjk_contract.rs`) — にほんご→日本語,
  きって/かんじ/ん edges, 한 + backspace split + jong steal + 닭/와
  compounds, 你好 + 10-candidate paging, user-dict determinism, one-session
  engine swap behind the trait, 10k-key fixed-seed no-panic soak per engine.

### Added - 2026-07-22

#### IME v2 Phase 2 (RFC-0075, TASK-0147 Done): on-screen keyboard

- **Capability-gated OSK injection**: imed serves a second, DEDICATED
  `imed-osk` endpoint via a kernel waitset (main + osk multiplexed) —
  possession of the route cap IS the injection authorization (app processes
  carry no sender identity on server endpoints). init mints the endpoint
  (RECV pinned to imed slot 5), execd provisions the SEND only to bundles
  holding the new `nexus.permission.IME`, and the new `ime` bundle TYPE is
  the pack-time privilege ceiling (nxb-pack) — deny-by-default with zero
  runtime identity checks. `source=osk` on the main endpoint stays DENIED;
  mis-tagged `source=hw` frames on the osk endpoint are DENIED.
- **ime-ui overlay app** (`userspace/apps/ime-ui`): the OSK as a DSL app —
  `Window { style: plain, level: overlay }`, de/us layouts (on-keyboard
  globe toggle; keymap-driven layout + shift = recorded follow-ups), taps
  dispatch through the new `svc.ime.key/action` DSL surface (route slot 18,
  fire-and-forget). Not user-launchable (launcher type allowlist).
- **Overlay window band**: new `WindowRole::Overlay` z-band (above all
  floating windows), chromeless, docked to the bottom display edge
  (`OSK_BAND_H` = 264, WM-owned geometry), shown WITHOUT stealing window
  focus (`WindowStack::show_unfocused`). windowd shows/hides the band on
  text focus in touch profiles and lazily launches `ime-ui` on first use —
  pure compositing + lifecycle request, no OSK drawing in windowd.
- **Kernel**: `DEFAULT_CAP_SLOTS` 128 → 256 (recorded urgent follow-up —
  init's table ran AT the ceiling; late clones NoSpace-failed).
- **Proofs**: `init: imed osk recv ok` + `execd/selftest route->imed-osk
  ok`, `SELFTEST: ime v2 osk ok` (positive accept + mis-tag deny) in
  `ci-os-smp1`; interactive OSK typing in a visible boot.
- Structure-gate splits: windowd `runtime/intent.rs` + `window_state.rs`;
  app-host `effect_ime.rs`; init `route_provision` osk legs +
  `endpoints::clone_osk_pair/close_wired_eps`.

### Fixed - 2026-07-22

#### i18n v2 follow-up: two container regressions (RFC-0077)

- **Greeter/shell opened as a floating window** (session start broken): the
  pre-mount window-intent reader parsed the raw payload — for apps with
  locale packs that is now an NXLC container, not a program, so the intent
  tags silently fell back to defaults. All pre-mount `ProgramReader` uses now
  go through `probe/locale.rs::payload_nxir` (container-aware).
- **Fresh mounts ignored the configured locale/tz** (Settings opened English
  despite `ui.locale=de-DE`): the attach-time `OP_SURFACE_REGION` push was
  drained and DROPPED by the pre-mount waits (`wait_for_boot_pushes`,
  `request_content_rect`) — and windowd re-pushes only on change. The drains
  now STASH the region push (`boot::RegionPush`) and app-host applies it
  right after mount, so the FIRST frame renders in the configured language.

### Added - 2026-07-21 (late night)

#### i18n v2 (RFC-0077, TASK-0240/0241 Done): runtime language switch

- **Locale packs**: bundle build compiles every `i18n/<tag>.json` into an
  index-aligned `NXL1` pack (key order = NXIR `i18nKeys`; absent keys fall
  back to the baked default text) and ships apps as an `NXLC` payload
  container (`nexus_dsl_core::compile_project_bundle`, new `locale_pack`
  module; pack-less apps keep the raw `.nxir` payload). Deterministic bytes;
  fail-closed bounded parsing on both sides (`test_reject_*` truncation +
  mutation matrices in `tests/dsl_goldens/tests/i18n_packs.rs`).
- **Runtime swap**: app-host splits the container at mount (`probe/locale.rs`),
  resolves `@t()` through `CatalogOverBaked` (active pack catalog → baked
  default) at every dispatch site, and applies the `OP_SURFACE_REGION`
  locale tag (exact tag, then primary subtag `de-DE`→`de`): swap +
  `view.reemit()` + relayout + bounded `apphost: locale <tag> applied`.
- **windowd** subscribes `ui.locale` as a second watch on its one push
  channel (`cap_clone` the SEND half before the first `OP_WATCH` cap-move —
  each moved cap = one subscriber slot; no wire/table change).
- **Settings → Allgemeine Verwaltung**: language picker (Deutsch/English →
  `ui.locale`); full German catalogs for settings, greeter and desktop-shell
  (gaps in the existing `de.json` files filled).
- **Proofs**: `SELFTEST: i18n switch ok` (ui.locale flip round-trip through
  the watch spine, end state = shipped default `de-DE`) in the boot gate;
  live re-render via `apphost: locale <tag> applied` in a visible boot.
- Structure-gate splits: app-host `probe/env.rs` (theme tokens + device env)
  + `probe/locale.rs`; dsl-core `locale_pack.rs`.

### Added - 2026-07-21 (night)

#### Wall-clock v1 (RFC-0076, TASK-0297 Done): live clock end-to-end

- **timed reads the goldfish RTC itself** (documented deviation: no rtcd
  service — a 2-register read-only device vs scarce init cap-table headroom);
  `rtc-goldfish` driver lib (`source/drivers/rtc/goldfish-rtc`, dtb-verified
  window 0x101000), policy-gated `device.mmio.rtc` grant, anchor =
  RTC epoch + monotonic delta; `OP_GET_WALLTIME=4` serves UTC,
  `STATUS_UNAVAILABLE` while unanchored — never fake time.
- **tz-lite** (`userspace/tz-lite`): 9-zone curated table (= `time.zone`
  validator SSOT, settingsd pin test), EU/US/AU DST rules, Hinnant civil
  conversion, 12/24h formatting — 5 host goldens incl. DST boundaries.
- **Region fan-out pulled forward from RFC-0077**: `OP_SURFACE_REGION=23`;
  windowd watches settingsd (`time.`) on its own init-provisioned channel
  and pushes tz/hour-format at attach + on change.
- **Live clock**: app-host minute tick (`svc.time` SDK route slot 17,
  `nexus.permission.TIME`) dispatches `ClockEvent::Tick(time, date)`;
  greeter + shell bind `$state.clock/date` (static demo strings removed);
  Settings General: timezone + 24h/12h chip pickers.
- **Proofs**: `timed: walltime anchored`, `SELFTEST: walltime rtc ok`,
  `SELFTEST: clock tz ok` deterministic in `ci-os-smp1`; **live**
  `apphost: clock tick applied` (visible boot — greeter clock state changed).
- **Cap-table cascade fixed**: windowd→abilitymgr and abilitymgr→execd
  routes were still `cap_clone`-based and NoSpace-failed after the new
  pre-mints (silently killing the greeter launch); both converted to direct
  transfers. Kernel `DEFAULT_CAP_SLOTS` raise = urgent recorded follow-up.

### Added - 2026-07-21 (evening)

#### Settings spine (RFC-0078, TASK-0298): General-management keys + OP_WATCH push propagation

- **5 new registry keys** (validated, persisted, non-secret charter pinned):
  `region.country` (DE), `input.keymap` (de — DE QWERTZ ships), `time.zone`
  (Europe/Berlin, curated zone list = future tz-lite SSOT), `time.format`
  (24h), `ime.personalization` (on).
- **`OP_WATCH`/`OP_EVENT`**: bounded change propagation — the watch request
  cap-moves the subscriber's push channel; ≤8 subscribers, drop-oldest +
  resync flag, dead-subscriber reclaim (host-tested `WatchTable`).
- **inputd consumer**: watches `input.` on an init-provisioned fixed-slot
  channel (pre-minted — cap-table ceiling — and closed after wiring) and
  swaps the live keymap on push (`inputd: keymap set <layout>`).
- **Settings app**: General management is real — Country/Region and
  Keyboard-layout chip pickers write through `svc.settings`.
- **Proofs**: `SELFTEST: settings watch ok` (subscribe → flip `input.keymap`
  us→de → both pushes observed; end state = shipped default) deterministic in
  `ci-os-smp1`. `@mint-pair` allowlist extended to the selftest harness.
  Trap fixed en route: a yield-spin settle suppressed the kernel
  `KSELFTEST: runtime timer budget ok` proof — waits are deadline-blocked
  recvs now.

### Added - 2026-07-21 (later)

#### IME v2 Phase 1 (RFC-0075, TASK-0147 Part 1): imed service real — typing lands in apps

- **imed is a real bootstrapped service**: `ImedCore` (focus gate + ime-core
  composition + push planning, host-tested) + os-lite serve loop with kernel
  `sender_service_id` identity gates (OP_KEY only from inputd, OP_SET_FOCUS
  only from windowd; rejects answer non-blocking, OK pushes stay silent).
  Boot: `init: start/up imed`, `imed: ready` in the deterministic ladder.
- **Init topology**: `ServiceId::Imed = 26`, pre-minted server pair, cpu0
  affinity (interactive chain), routes inputd→imed / windowd→imed /
  imed→windowd / selftest→imed. **Cap-table lesson:** init's 128-slot cap
  table is at its ceiling by late wiring — the routes use direct
  `cap_transfer` (target-side allocation); late `cap_clone`s NoSpace-fail
  (recorded follow-up in TASK-0147).
- **inputd** forwards every resolved key (Text/Dead/Action) to imed
  fire-and-forget per batch (`forward_keys_to_imed`, fixed frames, hot
  pointer path untouched); imed is the focus gate.
- **windowd** routes text as pure compositor plumbing
  (`compositor/runtime/text_input.rs`): `OP_SURFACE_TEXT_FOCUS` from apps is
  identity-resolved (owner sid; desktop surface included) and relayed to
  imed; imed's `'I','E'` pushes (magic-discriminated on the server endpoint)
  are translated to `OP_SURFACE_TEXT` on the focused surface's event channel.
- **app-host**: tap-to-focus announces widget focus transitions upward
  (`apphost: text focus set/cleared`); `OP_SURFACE_TEXT` commits/actions
  insert into the focused DSL field (imed wire `OP_ACTION=7` added for
  Enter/Backspace pass-through).
- **Legacy `source/services/ime` deleted** (TRACK-AUTHORITY-NAMING closure);
  dead selftest dep removed.
- **Proofs**: `SELFTEST: imed reject foreign ok` (foreign-identity OP_KEY
  DENIED — deterministic every boot); `just ci-os-smp1` green end-to-end;
  positive chain PROVEN LIVE (QMP tap + key → `apphost: text focus set` →
  `apphost: text commit applied`, one-shot count-only marker).
- **`OP_SURFACE_TEXT_FOCUS` carries the app's own `surface_id`**: windowd's
  server endpoint has no per-sender identity for app processes
  (`sender_sid == 0`) — identity-derived sender resolution was replaced by
  surface claims (focus-misdirection-only blast radius; recorded follow-up
  shared with `OP_SURFACE_CONTROL`).
- **Regression fixed (was: "320x240 desktop / splash hang")**: the two imed
  endpoint mints pushed init's 128-slot cap table to its ceiling, breaking
  runtime `@mint-pair` for app event channels — init now closes its imed
  pair caps after wiring (mint→grant→close). Plus `inputd = ["ipc.core"]`
  (`!route-deny: inputd → imed`) and key-forward failure instrumentation.
- Side fix: hidrawd dead `WireMeta` count fields (warning-gate break from the
  2026-07-20 input-storm commit) removed.

### Added - 2026-07-21

#### IME v2 Phase 0 (RFC-0075, TASK-0146): host composition core + focused-field model + wire codecs

- **RFC-0075 seeded** — the IME v2 contract: two-level text-focus model,
  imed wire protocol, composed-text delivery, typed-text security invariants
  (supersedes the RFC-0058 stub contract for everything beyond TASK-0059).
- **`userspace/ime-core` (new):** no_std/alloc-free dead-key/compose state
  machine (DE `´` `` ` `` `^`, const compose tables, bounded preedit,
  deterministic `ImeOutcome`); 12 contract tests incl. fallback (`´`+`x` →
  `´x`), cancel (Escape/Backspace), flush-and-pass (Enter).
- **`userspace/keymaps`:** DE dead keys are now marked `KeyOutput::Dead(char)`
  (EQUAL `´`/`` ` ``, GRAVE `^`); only the composer interprets them — US and
  the merged jp/kr/zh tables are unchanged.
- **Wire codecs (golden bytes + reject matrices):**
  `nexus-wire/src/imed.rs` (MAGIC `'I','E'`: SET_FOCUS/KEY/COMMIT/PREEDIT/
  CANDIDATES/CANDIDATE_SELECT, bounded candidate-list packing) and
  `nexus-display-proto/src/surface_text.rs` (new module: `OP_SURFACE_TEXT=21`,
  `OP_SURFACE_TEXT_FOCUS=22` with caret-anchor rect; op 23 reserved for
  RFC-0077 region push).
- **DSL focused-field model (`nexus-dsl-runtime/src/focus.rs`):**
  tap-to-focus on Change-bound fields, focused `insert_text`/`backspace_text`
  (bounded 256 chars), focus survives re-emits by binding identity;
  `TextField { secure: true }` renders bullets (the real value never enters
  the scene), reports password in the focus snapshot.
- Fixed a latent test-compile break in `nexus-gfx`
  (`command/buffer_wire_tests.rs` used `super::buffer` from a nested module —
  landed broken in the 2026-07-20 content-epoch commit).
- No OS/QEMU behavior change yet — typing lands in apps with TASK-0147
  (`imed` service wiring); no markers added in this slice.

### Changed - 2026-07-20

#### Kernel: earliest-deadline timer arming + affinity-respecting steal park (ADR-0052)

- **`arm_wakeup` (EDT coalescing):** every timer-arming path (timer caps, timed
  IPC recv/send, waitset, fence) now keeps the EARLIEST pending deadline armed
  per hart instead of last-writer-wins on the single mtimecmp register; the
  timer-IRQ re-arm folds in blocked-task IPC/waitset/fence deadlines instead
  of clobbering them to the 10 ms fallback tick. Fixes windowd's 120 Hz pacer
  slipping to the 100 Hz tick under SMP=4 (measured: drag-time `slip=` <1 ms
  bucket 0-3 → 8-14 ticks/s). Self-heal: an elapsed shadow deadline never
  suppresses a new arm (S-mode timer traps don't clear the shadow).
- **Steal park respects affinity homes:** an affinity-rejected stolen task is
  parked on its HOME CPU's queue, not cpu0's — `schedule_next` is
  affinity-blind, so the old cpu0 park ran background work on the pinned
  display hart.
- New deterministic KSELFTESTs: `edt arm ok`, `steal park ok`
  (`selftest/smp_sched.rs`, split out of `selftest/mod.rs` with the existing
  steal probes).

#### gpud: double-buffered GL scanout (tear-free SMP=4 presents)

- Every virgl buildup present renders into a BACK render target
  (`GL_SCANOUT_RES_B`) and flips via `SET_SCANOUT` + `RESOURCE_FLUSH` as the
  batch tail — the host GTK draw (async under MTTCG) can only ever sample
  complete frames. This removes the mouse/drag flicker that appeared with
  SMP=4: previously each present cleared + rebuilt the LIVE scanout texture
  over a ~21 ms window the host could sample mid-composite. Copy-fallback via
  `SCANOUT_FLIP = false` (atomic fullscreen `RESOURCE_COPY_REGION`). One-shot
  honest marker `gpud: gl flip on`; `scanout_sample` reads the front RT.

#### windowd/gpud: per-layer content epoch — window drags stop re-uploading the atlas

- `Layer`/`Command::CompositeLayer` gain `content_epoch` (wire: 18→19 words;
  the serializer's bounds check also fixed — it reserved 17 words for an
  18-word payload). windowd bumps ONE global atlas epoch at every atlas write
  choke point and stamps it into each emitted layer; gpud re-uploads the GL
  atlas texture only when the layer set's epoch changed (invalidated on an
  abandoned present batch). Drags/transforms re-emit the scene every frame but
  never write content — their per-layer `TRANSFER_TO_HOST` train collapses,
  and with the SUBMIT_3D coalescer the present drops from ~15 to ~4 ring
  entries (measured: drag enq 21ms → 11-13ms, entries/present 15 → 4).

#### windowd/gpud: SMP-flicker triage diagnostics

- `windowd: loop hz=` gains `nack=`/`fullrq=` counters and a pacer-slip
  histogram (`slip=a/b/c/d`: <1/1-3/3-8/≥8 ms, decoded from `OP_TIMER_FIRED`
  deadline+now); `gpud: present us` gains `win_ms=` (window wall-clock → real
  present rate).

#### nexus-wire: declarative service wire codec + nexus-abi identity split (ADR-0051, TASK-0296)

- **New crate `source/libs/nexus-wire`** (no_std, `forbid(unsafe_code)`, zero
  deps): SSOT for the nine service↔service wire protocols (execd, updated,
  routing, bundlemgrd, sessiond, settingsd, bundleimg, policy, policyd).
  Frames are **declared** via the `frames!` DSL over a small codec core
  (`Writer`/`Reader`, the magic/version/op guard, `op|0x80` reply convention
  and length-prefix bounds written once) instead of 66 hand-coded
  encode/decode functions. Wire bytes unchanged — all golden-byte tests moved
  verbatim and pass unmodified; every protocol gained a deterministic
  truncation/mutation reject matrix (`codec::testing::assert_reject_matrix`).
- **nexus-abi shrinks to its charter** (kernel↔userspace ABI): the wire half
  moved out; `nexus_abi::<svc>` paths keep resolving via transitional
  re-exports (zero churn across the ~51 dependent crates). The 4103-LOC
  `lib.rs` monolith is dissolved: syscall wrappers split into
  `src/syscall/{mod,ipc,types,task,time,caps,memory,debug}.rs` (root paths
  preserved via re-exports), root `lib.rs` is now 183 lines, and the
  grandfathered structure-gate entry is deleted from `config/loc-baseline.txt`.
- `abi_filter::MAX_PROFILE_BYTES` is now defined at its wire bound
  (`nexus_wire::policyd`) and re-sourced by `abi_filter` (single definition,
  clean dependency direction).

### Changed - 2026-07-17

#### Repository hygiene track — structure, docs, gates, zero warnings

- **Agent config SSOT**: `CLAUDE.md` (new) + slim `AGENTS.md` pointer replace the
  six drifted rule sets (`.cursorrules`, `.clinerules`, `.cursor/`, `cline/`,
  `.deepseek/`, `agents.md` — all deleted); `.claude/skills/` gains
  `boot-proof` and `verify` workflows.
- **Docs restructure**: single architecture index (`docs/architecture/README.md`,
  old `docs/ARCHITECTURE.md` merged+deleted); `graphics/` + `inference/`
  subdirs; new `docs/README.md` master index; ADR index + template
  (ADR-0019 documented as retired); RFC-0033 number collision resolved —
  DSoftBus mux RFC renumbered to **RFC-0060**; 871-line `testing/index.md`
  split into seven focused docs; run-log/hypothesis-grid reference now at
  `docs/testing/run-logs.md`; UI doc duplicates merged; `resources/README.md`.
- **Build tooling**: `scripts/fmt-clippy-deny.sh` delegates to just recipes
  (no more divergent flags); `config/os-services.txt` = SSOT of the 17-crate
  OS slice (dep-gate/diag-os/make); new `just check`, `lint-kernel`,
  `deadcode`, `logs-gc`, `check-markers`, `test-os2vm`; `test-all` redesigned;
  `visible-bootstrap` profile removed (headless GPU coverage stays in
  `ci-os-display-gpu-pci`).
- **Test infra**: chain-marker contract SSOT `tools/nx/chains/markers.txt`
  (sim tests + real uart reconciliation via `scripts/check-chain-markers.sh`,
  wired into proof profiles); stale `ui_v3a_host`/`ui_v3b_host` and
  `chain_dsl_mount` removed (tested deleted legacy APIs); os2vm runs land in
  `build/logs/os2vm--<ts>/`; selftest arch-gate green again (dispatch split,
  51 markers back-filled into the proof manifest).
- **Zero warnings**: `just diag-host` / `diag-os` / `diag-kernel` all clean
  (legacy dead code deleted, contract surfaces kept with reasoned allows);
  workspace-wide rustfmt applied (245 files).
- **CI + community**: `ci.yml` rewritten as thin just-recipe wrappers
  (`build.yml`/`ci-kernel.yml` deleted); new `CONTRIBUTING.md`,
  `CODE_OF_CONDUCT.md`, `SECURITY.md`, `docs/dev/git-workflow.md`;
  CODEOWNERS fallback owner; README current-state refresh.
- **Repo state**: nested `.claude/worktrees/` removed (drafts archived on
  branch `worktree-dsl-0075-frontend-ir-cli`); `neuron-boot.map` untracked;
  committed scratch/junk deleted; `build/logs/` retention via `just logs-gc`.
- Follow-ups tracked in `tasks/TRACK-REPO-HYGIENE-FOLLOWUPS.md`.

### Changed - 2026-06-12

#### TASK-0064 (UI v6a): Rescoped — Window Management v1 (Chat-Window + Drag)

- **Scope change**: TASK-0064 von abstraktem WM-Layer (z-order/focus/states + transitions)
  auf konkretes Chat-Window rescoped. Der Chat wird das erste echte Window.
- **RFC-0064**: Design seed contract created. Chat-Window mit Title-Bar, Drag, X-Close,
  Z-Order. Chat-Button links neben Hamburger-Menu.
- **Deferred**: Scene Transitions (Crossfade/Slide) → TASK-0064B.
  Multi-Window, Resize, IPC → zukünftige Tasks.
- **Touched**: TASK-0064 updated, RFC-0064 created, .cursor/ state files updated.

#### TASK-0063 (UI v5b): Scene graph GPU pipeline + virtual list + theme tokens + virgl — Done

- **Scene graph as rendering authority**: `generate_commands_into()` translates
  all `RenderPrimitive` variants into GPU CommandBuffer commands. `flush_pending_damage`
  now drives rendering exclusively from the scene graph dirty set — no CPU compositing.
- **CPU compositor removed**: Deleted `backdrop.rs`, `scene.rs`, `shadow.rs`,
  `surface.rs`, `source.rs` from `windowd/src/compositor/`. All rendering is GPU-only.
- **Scene graph extensions**: `MAX_NODES` raised 256→2048. Added `batch_insert`,
  `recycle_node`, `set_text_content`, `set_rect`, `free_slots` for virtual list support.
  `Group` nodes with `BoxShadow` now emit blur+fill shadow commands.
- **Virgl feature gate** (`gpud`): New `virgl` Cargo feature with runtime capability
  detection via `VIRTIO_GPU_F_VIRGL` feature bit. Emits `gpud: virgl ready` or
  `gpud: cpu fallback`. Separable gaussian blur (`blur_backdrop_separable_vmo`)
  serves as reference for future GPU shader dispatch.
- **Virtual list widget** (`nexus-virtual-list`): `VirtualList<P: ItemProvider>` with
  overscan, recycling pool, scroll anchor, mixed-height measurement cache.
  `ItemProvider` trait for lazy-loading page providers.
- **Theme tokens**: `ThemeRegistry` with dependent notification and 2PC-ready
  switching (`prepare_switch`/`commit_switch`/`abort_switch`).
- **Dual-panel blur**: Chat panel with `BackdropFilter` + `Group` shadow mounted
  in `SystemUiShell` alongside the proof panel. Shared backdrop cache in one CB.
- **Host tests** (`tests/ui_v5b_host/`): 19 tests covering scene graph wiring,
  virtual list (1000 items, mixed heights, scrolling, anchor stability),
  lazy-loading provider, chat mockup, and theme token resolution.

### Changed - 2026-06-02

#### TASK-0062 Phase 6: GPU-only display architecture — windowd sole owner

- **Architecture**: Removed fbdevd/ramfb from OS graph. windowd is sole display owner,
  gpud is pure GPU driver. Follows OHOS/Fuchsia/Android pattern: one compositor,
  one GPU driver, zero-copy VMO handoff via `OP_SET_FRAMEBUFFER_VMO`. No fbdevd,
  no ramfb, no handoff from another service.

- **gpud**: `service_main_loop` now only probes device and becomes IPC-ready.
  No startup `create_resource`/`set_scanout`/splash. `OP_SET_FRAMEBUFFER_VMO`
  now emits `gpud: scanout ok` / `gpud: cursor on` / `gpud: display ready` on
  successful scanout. Splash module (`splash.rs`) deleted.

- **windowd**: Always creates own framebuffer VMO (`vmo_create`). Removed
  `OP_SEND_COMPOSED_FRAME_VMO` handler (fbdevd VMO handoff path). Removed
  `KernelClient` import from compositor main loop.

- **init-lite**: `fbdevd` removed from `build.rs` default_candidates.

- **selftest observer**: Routes to `windowd` instead of `fbdevd` for display
  evidence (`route_with_retry("windowd")`).

- **Markers**: `qemu-test.sh` expected sequence updated for GPU-only path.
  `bringup.toml` fbdevd entries removed. `ui.toml` architecture comment
  and marker names updated.

- **Tests**: 16 new spec-validation tests in `gpud/tests/protocol_tests.rs`
  covering format constants, command types, response types, MMIO offsets,
  and wire-format struct sizes.

- **Cleanup**: Unused imports removed from gpud and windowd. Deleted
  `source/drivers/gpud/src/splash.rs`.

### Fixed - 2026-06-01

#### nexus-init OS build regression (RFC-0061 incomplete refactoring)
- **`source/init/nexus-init/Cargo.toml`**: Added `[[bin]] required-features = ["std-server"]` to prevent RISC-V compilation of host-only binary.
- **`source/init/nexus-init/src/lib.rs`**: Added missing `extern crate alloc;` for `no_std` OS builds.
- **`source/init/nexus-init/src/os_payload.rs`**: Added `pub(crate) use` re-exports (`debug_write_*`, `fatal_err`, `ServiceNameGuard`, `RouteTable`, etc.) for items moved to `bootstrap/` during RFC-0061 refactoring. Made private constants and type aliases `pub(crate)`.
- **`source/init/nexus-init/src/bootstrap/helpers.rs`**: Added `pub(crate)` visibility to functions used by sibling modules. Added missing imports (`LineBuilder`, `log_topics`, `extern` symbols). Made `ServiceNameGuard` struct and fields `pub(crate)`.

#### Compiler warnings
- **gpud/backend.rs**: Prefixed unused closure params with `_`, added `#[allow(dead_code)]` on `ResourceRecord` and `CURSOR_QUEUE_INDEX`.
- **windowd/compositor/backdrop.rs**: Removed unused imports.

#### Proof-manifest
- **markers/ui.toml**: Changed `fbdevd: ready` from `phase=end emit_when={profile=visible-bootstrap}` to `phase=bringup` (fbdevd now starts early per RFC-0059 Phase B).

### Changed - 2026-05-31

#### RFC-0059 Phase 3–6: Production-Grade Display Pipeline

- **gpud**: Display resource upgraded from 64×64 proof to 1280×800 (`DISPLAY_WIDTH`/`DISPLAY_HEIGHT`).
  `virtio-gpu-device` promoted to primary QEMU display (before `ramfb`). New markers:
  `gpud: scanout 1280x800 bgra8888`, `gpud: display ready (w=1280, h=800)`.
- **gpud**: New IPC op `OP_SET_FRAMEBUFFER_VMO` (3) — windowd sends framebuffer VMO for
  zero-copy GPU scanout. `VirtioGpuBackend::attach_external_framebuffer()` attaches external
  VMO as virtio-gpu resource backing, sets as primary scanout.
- **fbdevd**: Boot splash optimized from 800 per-row `vmo_write` calls to single bulk write.
  fbdevd promoted to Priority-0 (3rd service spawned) for <200ms splash visibility.
- **windowd**: Defensive init with wallpaper fallback (solid dark-blue 160×100 when JPEG
  unavailable). New diagnostic markers: `windowd: runtime init start/ok`,
  `windowd: wallpaper loaded (jpeg)`, `windowd: wallpaper fallback solid`.
- **windowd**: `try_handoff_framebuffer_to_gpud()` sends framebuffer VMO to gpud on registration.
  Falls back silently to CPU ramfb path when gpud unreachable.
- **fbdevd**: `register_framebuffer_with_windowd()` exponential backoff (10ms→500ms) with
  diagnostic marker on 3rd retry.

### Changed - 2026-05-22

#### TASK-0059: Compositor module refactoring

- **Refactored**: `source/services/windowd/src/os_lite.rs` (4860 line monolith) split into
  `source/services/windowd/src/compositor/` — 18 focused files with clear ownership boundaries.
  No functional change. All 9 host tests pass. `lib.rs` public API unchanged.
- **Module structure**: `runtime.rs`, `surface.rs`, `backdrop.rs`, `filter.rs`, `shadow.rs`,
  `scene.rs`, `types.rs`, `cache.rs`, `primitives.rs`, `sdf.rs`, `tile_map.rs`, `damage.rs`,
  `blur.rs`, `source.rs`, `path_cache.rs`, `cursor.rs`, `font.rs`, `tests.rs`.
- **Renamed**: `os_lite` → `compositor` throughout (`lib.rs`, module declarations, imports).

### Fixed - 2026-05-21

#### TASK-0059: ShadowCache heap exhaustion on bump allocator

- **Crash fix**: Removed `to_vec()` heap allocation from `compute_shadow_row` hot path.
  Per-row shadow caching with `Vec<u8>` exhausted the 512KB bump allocator (~3500 bytes/row
  × 316 shadow rows = ~1.1MB). Only visible with real display (QEMU `DISPLAY_BACKEND=none`
  skipped the rendering path entirely).
- **Removed**: `ShadowCache` field, import, and all cache get/insert logic from
  `windowd/src/os_lite.rs`. Shadow compositing now executes inline with zero heap allocations
  using pre-allocated `shadow_scratch` + `blur_row_buf`.
- **Added**: `ShadowArena` (64KB pre-allocated buffer pool) in `nexus-effects` for
  production-grade per-box shadow caching (follow-up optimization).
- **Tests**: 8 new tests — `ShadowArena` alloc/reset/overflow/get, alloc-fail prevention
  budget checks, deterministic reset behavior.

### Added - 2026-05-18

#### TASK-0059: UI v3b clip/scroll/effects + IME stub + filter-box proof element

- **Layout engine clip+scroll**: `clip_rect` and `scroll_offset` fields on `LayoutBox`; `Overflow::Hidden` containers propagate scissor rects to children; `compute_scroll_damage()` (bounded, allocation-free) and `LayoutResult::reposition_scroll()` (place-only, no remeasure)
- **TextInputNode**: new `LayoutNode::TextInput` variant with content, cursor_pos, placeholder, and max_length; measures like TextNode
- **Filter-box proof element**: `filter_words()` pure function on 15-word static list; filter-box layout tree (TextInput + `Overflow::Hidden` scrollable word list) integrated into windowd proof panel; 3 cards (hover/click/key) in vertical column, scroll card removed
- **Effects crate (`nexus-effects`)**: box blur (3×3 and 1×3), drop shadow compositing, `EffectBudget` with deterministic degrade, LRU `EffectCache`, `CursorBlink` timer
- **IME stub (`imed`)**: focus routing, `CaretSelection` helpers, caret movement, selection range, 6 unit tests
- **Host tests (`tests/ui_v3b_host/`)**: 23 tests covering scroll damage (4), clip boundaries (2), `filter_words` (6), filter-box layout (3), scroll reposition, effect budget (3), blur (2), cursor blink (2), proof panel filter integration
- **12 OS markers defined** in `windowd/markers.rs` (clipping, scroll, text input, filter, effects, selftest summary)

### Added - 2026-05-19

#### TASK-0059 Phase 6a: Separable blur + shadow properties + two-pass renderer

- **Separable blur (`nexus-effects`)**: `blur_1d()` sliding-window box blur (O(w·h) per pass), `blur_separable()` 2D box blur via horizontal+vertical passes, zero-copy with reused row/transpose buffers
- **Shadow types (`nexus-layout-types`)**: `BoxShadow` (offset, blur_radius, spread, color), `TextShadow` (offset, blur_radius, color), `ShadowLevel` enum (Sm/Md/Lg/Xl/Xxl2) with `to_box_shadow()` Tailwind presets
- **VisualStyle extensions**: `shadow: Option<BoxShadow>`, `text_shadow: Option<TextShadow>`, `opacity` changed to `Option<Fraction>` with `blend_factor()` for alpha compositing
- **Fraction helpers**: `OPAQUE`/`TRANSPARENT` constants, `as_u8()`, `blend_factor()` returning (numerator, 256) for `over` operator
- **Two-pass renderer (`windowd/os_lite.rs`)**: zero-copy `compute_shadow_row()` per-row shadow compositing (alpha mask → horizontal blur → tint → over-composite); `shadow_scratch` + `blur_row_buf` pre-allocated at startup; `blur_row_horizontal()` inline zero-allocation single-row blur
- **Host tests (`tests/ui_v4_host/`)**: 21 tests covering `blur_separable` (2), `blur_1d` (2), `BoxShadow`/`TextShadow` defaults (2), `ShadowLevel` presets (6), `VisualStyle` extensions (5), `Fraction` (4)
- **103 total host tests passing** across layout (9), windowd (31), ui_v3a (13), ui_v3b (20), ui_v4 (21), headless (9)

#### TASK-0059 Phase 6b: MSDF atlas for text and icon rendering

- **MSDF crate (`nexus-msdf`)**: build-time atlas generator rendering 95 printable ASCII glyphs (32-126) as 32×32 signed distance fields via `fontdue` + Inter font; packs into 1024×96 BGRA atlas embedded via `include_bytes!(env!())` for `no_std` compatibility
- **SDF computation**: two-pass 8SSEDT distance transform producing approximated Euclidean signed distance fields (0 = outside, 128 = edge, 255 = inside)
- **Runtime sampler**: `sample_atlas(ch, u, v) -> u8` bilinear-interpolated SDF lookup; `sdf_to_alpha(sd, aa_width) -> u8` smoothstep anti-aliasing; `glyph_metrics(ch) -> Option<&GlyphMetrics>` for advance/bearing/atlas position
- **Zero runtime allocations**: all data in static embedded arrays; `fontdue` only at build time; `no_std` + `alloc` compatible
- **22 host tests**: atlas dimensions/constants (6), glyph metrics lookup (5), SDF sampling correctness (7), sdf_to_alpha math (4)
- **43 total ui_v4_host tests** (21 phase6a + 22 phase6b), dep-gate PASS

#### TASK-0059 Phase 6c: Analytical SDF shapes for anti-aliased rendering

- **SDF crate (`nexus-sdf`)**: `sd_circle`, `sd_rect`, `sd_rounded_rect`, `sd_triangle` analytical signed distance primitives; `smoothstep` cubic Hermite interpolation; `fill_alpha`/`border_alpha` rendering combinators; `rounded_rect_fill_alpha`/`rounded_rect_border_alpha` convenience functions; `no_std` + `libm`, zero allocations, deterministic
- **Renderer integration (`windowd/os_lite.rs`)**: `fill_sdf_circle_row`/`stroke_sdf_circle_row` replace hard-edged `fill_circle_row`/`stroke_circle_row` for anti-aliased circles; `fill_sdf_rounded_rect_row`/`stroke_sdf_rounded_rect_row` used for `ShapeKind::Rect` with `corner_radius > 0`; hard-edged rects keep fast `fill_row_rect` span-fill path
- **23 SDF host tests**: circle (4), rect (3), rounded rect (4), triangle (3), smoothstep (3), fill/border alpha (4), rounded rect convenience (2)
- **66 total ui_v4_host tests** (21 phase6a + 22 phase6b + 23 phase6c), 148 total host tests, dep-gate PASS

#### TASK-0059 Phase 6d: 9-slice shadow compositing

- **9-slice shadow (`nexus-effects`)**: `NineSliceShadow` decomposition (corner_size, blur_radius, spread, color); `composite_nine_slice_shadow()` renders 4 corners with 2D separable blur, 4 edges by stretching blurred corner columns/rows, center fill with solid shadow alpha — ~90% fewer blur ops than full-surface; `EffectCache` integration with compound key `(elem_w, elem_h, params)`
- **Bug fix**: `blur_1d` vertical pass used wrong stride (`w*4` instead of `h*4`) for transposed buffer; fixed
- **8 host tests**: basic output, zero-size noop, budget exhaustion, corner blur verification, center fill solidity, cache hit/miss, different params → different cache keys, area ratio vs full-surface blur
- **74 total ui_v4_host tests** (21+22+23+8), 156 total host tests, dep-gate PASS

#### TASK-0059 Phase 6e: Dual-kawase blur

- **Dual-kawase blur (`nexus-effects`)**: `dual_kawase_blur()` — downscale pyramid (2× box-filter per level), iterative `stride_blur_3x3` with configurable sample step (1, 2, 4, …), bilinear upscale reconstruction; O(log(radius)) samples/pixel vs O(radius²) for box blur; `stride_blur_3x3` underflow fix for `isize` offset arithmetic
- **7 host tests**: identity (r=0, iter=0), solid color preservation, edge blur spread, small image noop, iteration comparison, large radius 48×48
- **81 total ui_v4_host tests** (21+22+23+8+7), 163 total host tests, dep-gate PASS

#### TASK-0059 Phase 6f: Render cache + damage integration

- **Specialized caches (`nexus-effects`)**: `ShadowCache` (256-entry LRU, keyed by node_id_hash + params, per-node invalidation), `TextCache` (512-entry LRU, keyed by glyph_id + scale_bucket, per-scale invalidation); existing `EffectCache` retained for 9-slice backward compat; `RenderCache` aggregator with `begin_frame()`, `invalidate_dirty()` (shadows cleared on dirty, text survives), `note_scroll()` (no invalidation), `clear()` (full clear on theme change)
- **15 host tests**: ShadowCache (insert/get, miss, update, LRU eviction, node invalidation, clear), TextCache (insert/get, miss, LRU eviction, scale invalidation), RenderCache (clear, dirty invalidate, scroll preserve, no-dirty no-op, begin_frame)
- **96 total ui_v4_host tests** (21+22+23+8+7+15), 170+ total host tests, dep-gate PASS
- **RFC-0058 Phase 6 complete** — NeX UI Rendering Pipeline fully implemented

### Fixed - 2026-05-20

- **Budgeted first-frame glass quality**: `write_current_frame` now calls `select_glass_quality(self.mode.height)` instead of forced `GlassQuality::High`. On 800-row screens this degrades to `Opaque` (no blur), preventing the high-quality backdrop blur from blocking boot scanout. Previously caused black-screen QEMU boot.
- **Test string contract fix**: `windowd_first_frame_uses_budgeted_glass_quality` assertion updated from 3-arg to 4-arg `write_rows` call to include the `paint_only: false` parameter.

### Added - 2026-05-15

#### RFC-0057: UI v3a layout engine contract seed (pretext philosophy)

- Created design seed for the deterministic layout engine (`docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md`):
  - Rust type system: `Stack` (flex row/column), `Grid` (fraction columns), `Spacer`, `FlexItem`, `EdgeInsets`
  - `MeasureText` callback trait decoupling layout from `nexus-shape` (pure Rust: rustybuzz + fontdue, no C libs)
  - Naming aligned with DSL v0.1a (`Stack` not VStack/HStack; `padding`/`margin`/`gap` mirror modifiers)
  - Paragraph/run cache + line-layout cache split following chenglou/pretext prepare/layout philosophy
  - Fixed-point arithmetic (no `f32`/`f64` in layout math)
  - windowd proof panel replacement contract (hardcoded positions → layout-tree-driven)
  - Invalidation matrix for TASK-0059 scroll-as-place-only handoff
- TASK-0058 updated: concrete types, pretext reference, shape cache integration, windowd integration plan
- TASK-0059 updated: `depends-on: [TASK-0058]`, pretext reuse for scroll damage math, place-only contract
- RFC-0057 v2: Visual primitives (`Rgba8`, `Border`, `EdgeBorder`, `CornerRadius`, `VisualStyle`), Text styling (`TextAlign`, `LineHeight`, `FontWeight`, `WhiteSpace`, `TextStyle`), Container features (`Overflow`, `Position`, `ZIndex`, `flex_wrap`, `row_gap`), Theme token integration contract
- Phases restructured: 0=Container layout, 1=Visual+Text primitives, 2=Text wrapping+caches, 3=Host tests, 4=windowd
- TASK-0058: `flex_wrap`, `Position`, `ZIndex`, `row_gap`, `WhiteSpace` added to type system
- RFC-0057 status: Draft → In Progress; TASK-0058: In Progress (implementation starting)
- .cursor files synced: current_state, next_task_prep, context_bundles, pre_flight, stop_conditions

### Added - 2026-05-17

#### TASK-0058 **DONE** — production-grade layout engine
- 31 host tests, windowd integrated, no duplicate structure
- ProofPaintRole system + proof_box_rect guard clause for allocation-free rendering
- RFC-0057: Done

### Added - 2026-05-16

#### TASK-0058 impl done (31 tests)
- nexus-layout-types + nexus-layout (Flex+Grid engine)
- nexus-shape wrap.rs (UAX#14) + cache.rs
- tests/ui_v3a_host JSON goldens (4 tests)
- windowd: layout_panel.rs integrated into os_lite.rs (single source of truth, no duplicate structure)

### Changed - 2026-05-11

### Added - 2026-05-17

#### TASK-0058 **DONE** — production-grade layout engine
- 31 host tests, windowd integrated, no duplicate structure
- ProofPaintRole system + proof_box_rect guard clause for allocation-free rendering
- RFC-0057: Done

### Added - 2026-05-16

#### TASK-0058 impl done (31 tests)
- nexus-layout-types + nexus-layout (Flex+Grid engine)
- nexus-shape wrap.rs (UAX#14) + cache.rs
- tests/ui_v3a_host JSON goldens (4 tests)

### Changed - 2026-05-11

#### TASK-0056C / RFC-0055 present-input perf latency coalescing (`TASK-0056C`, `RFC-0055`)

- Closed the embedded reactor/runtime floor for present-input perf with deterministic latency coalescing:
  - `windowd` now implements deterministic pointer-motion burst coalescing (bounded batch + latest-wins) while preserving click, focus, wheel, and keyboard edges as individually observable events
  - `windowd` implements explicit no-damage frame skip (frame-level hash match, max 3 consecutive, forced present on 4th)
  - `windowd` implements explicit no-visible-state-change skip (semantic state, bounded counter, requires at least 1 frame shown)
  - All skip decisions check both damage and visible-state before skipping; if either is true, present proceeds
  - Added idle-cheap / wakeup-collapse telemetry and stable counter infrastructure
- Authority boundaries preserved: `inputd` normalizes input, `windowd` decides compose/skip/present, `fbdevd` handles cadence/scanout
- Proof package `tests/ui_v2c_host` with 22 host tests (coalescing, skip rules, reject-edge, boundedness assertions)
- `RFC-0055` promoted to Complete; implementation checklist fully checked
- QEMU marker ladder (56C perf markers) remains deferred to follow-up; `just diag-os` RISC-V build passed clean

### Changed - 2026-04-29

#### TASK-0055B / RFC-0048 visible QEMU scanout bootstrap (`TASK-0055B`, `RFC-0048`)

- Closed the narrow visible-bootstrap slice with a deterministic QEMU `ramfb` first-frame path:
  - `scripts/run-qemu-rv64.sh` now has an opt-in `NEXUS_DISPLAY_BOOTSTRAP=1` graphics path (`-display gtk`, `-device ramfb`) while preserving headless default runs
  - `nexus-init` grants `selftest-client` a policy-gated `device.mmio.fwcfg` capability for QEMU `fw_cfg` access
  - `selftest-client` writes the fixed `1280x800` ARGB8888 framebuffer VMO and configures `etc/ramfb` through `fw_cfg` DMA
  - `windowd` owns the fixed visible bootstrap mode, pattern, present evidence, and fail-closed marker gating
  - proof-manifest profile `visible-bootstrap` is explicitly a harness/marker profile, not a SystemUI/launcher start profile
- Added proof coverage for visible bootstrap mode/capability/pre-scanout rejects and QEMU marker validation:
  - `cargo test -p windowd -p ui_windowd_host -- --nocapture`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
- Visible SystemUI/launcher profile selection, input routing, cursor, dirty-rect display service behavior, virtio-gpu, perf budgets, and kernel/core production-grade display closure remain follow-up scope.

### Changed - 2026-04-27

#### TASK-0055 / RFC-0047 headless windowd present closure (`TASK-0055`, `RFC-0047`)

- Closed the headless `windowd` surface/layer/present slice after critical remediation (`RFC-0047` Done, `TASK-0055` Done):
  - `source/services/windowd` now owns bounded surface IDs, VMO-shaped buffer validation, layer commits, damage-aware composition, and minimal present acknowledgements
  - `source/services/windowd/src/lib.rs` is now a facade over focused modules instead of a monolith
  - `tests/ui_windowd_host` proves exact two-surface composition, no-damage present skip, deterministic layer ordering, present acknowledgements, generated Cap'n Proto roundtrips, vsync/input-stub behavior, atomic commit preservation, and expanded reject paths
  - `userspace/apps/launcher` is now the canonical `launcher` package; the old `source/apps/launcher` placeholder was removed
  - `selftest-client`, proof-manifest markers, `scripts/qemu-test.sh`, and `tools/postflight-ui.sh` now gate honest UI present markers
- Added proof coverage:
  - `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture`
  - `cargo test -p ui_windowd_host reject -- --nocapture`
  - `cargo test -p ui_windowd_host capnp -- --nocapture`
  - `cargo test -p launcher -- --nocapture`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `scripts/fmt-clippy-deny.sh`
  - `make build` → `make test`
  - `make build` → `make run`
- Visible scanout, real input routing, GPU/display-driver work, rich display presets, and kernel/MM/IPC/zero-copy production closure remain follow-up scope.
- VMO scope is explicitly limited to UI-shaped `windowd` handle/rights/byte-length validation; no new kernel VMO capability-transfer or zero-copy production claim is made.

#### TASK-0054 / RFC-0046 host renderer closure (`TASK-0054`, `RFC-0046`)

- Closed the narrow host-first UI renderer proof floor and RFC contract:
  - `userspace/ui/renderer` provides a safe Rust BGRA8888 `Frame`, checked dimensions/stride/damage newtypes,
    deterministic clear/rect/rounded-rect/blit/text primitives, and bounded full-frame damage overflow behavior
  - `userspace/ui/fonts` provides the repo-owned deterministic fixture font; no host font discovery or locale fallback
  - `tests/ui_host_snap` proves expected pixels, full rounded-rect/text masks, damage behavior, snapshot/golden
    comparison, PNG metadata independence, golden update gating, artifact path confinement, anti-fake-marker source
    scanning, and required reject classes
- Added host proof coverage:
  - `cargo test -p ui_renderer -- --nocapture`
  - `cargo test -p ui_host_snap -- --nocapture`
  - `cargo test -p ui_host_snap reject -- --nocapture`
  - `just diag-host`
  - `just test-all`
  - `just ci-network`
  - `scripts/fmt-clippy-deny.sh`
  - `make clean`, `make build`, `make test`, `make run`
- Synchronized `TASK-0054` to `Done`, `RFC-0046` to `Done`, RFC index, status board, implementation order, and UI testing docs.
- OS/QEMU present markers, compositor/windowd wiring, GPU/device paths, and Gate A kernel/core production-grade claims remain out of scope.

### Changed - 2026-04-26

#### TASK-0047 / RFC-0045 host-first closure (`TASK-0047`, `RFC-0045`)

- Closed the Policy as Code v1 host-first contract floor:
  - active policy root is now `policies/nexus.policy.toml`
  - `recipes/policy/` is legacy documentation only, not a live TOML authority
  - `userspace/policy` provides deterministic `PolicyVersion`, bounded evaluator traces, and stable reject classes
  - Config v1 carries policy candidate roots as `policy.root`
  - `policies/manifest.json` records the deterministic tree hash and validates fail-closed when missing or stale
  - `policyd` stages configd-fed `PolicyTree` candidates through `configd::ConfigConsumer` and rejects stale/unauthorized lifecycle changes
  - external `policyd` host frame operations for `Version`, `Eval`, `ModeGet`, and `ModeSet` are backed by `PolicyAuthority` and bounded audit events
  - the `policyd` service-facing check frame evaluates through the unified authority
  - `nx policy` lives under `tools/nx` with deterministic JSON/exit contracts; `nx policy mode` is explicit host preflight only
- Added host proof coverage:
  - `cargo test -p policy -- --nocapture`
  - `cargo test -p nexus-config -- --nocapture`
  - `cargo test -p configd -- --nocapture`
  - `cargo test -p policyd -- --nocapture`
  - `cargo test -p nx -- --nocapture`
- Synchronized Policy as Code architecture docs and added a local `tools/nx/README.md` entrypoint for the canonical CLI.
- OS/QEMU policy markers remain gated and intentionally unclaimed.

### Changed - 2026-04-24

#### TASK-0046 / RFC-0044 closure sync (`TASK-0046`, `RFC-0044`)

- Closed the Config v1 host-first contract floor:
  - JSON-only authoring for layered config sources under `/system/config` and `/state/config`
  - canonical Cap'n Proto effective snapshots remain the runtime/persistence authority
  - `configd` subscriber/update notification seam is covered by deterministic host tests
  - `nx config push` now writes deterministic state overlay `state/config/90-nx-config.json`
- Added closure-proof coverage:
  - lexical-order layer-directory merge proof in `nexus-config`
  - non-JSON authoring reject proof in `nexus-config`
  - `nx config reload --json` and `nx config where --json` contract tests
  - `nx config effective --json` parity proof against `configd` version + derived JSON
- Synchronized status/index/queue surfaces:
  - `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` → `In Review`
  - `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md` → `Done`
  - `docs/rfcs/README.md`, `tasks/IMPLEMENTATION-ORDER.md`, `tasks/STATUS-BOARD.md`
  - `.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/pre_flight.md`, `.cursor/stop_conditions.md`, `.cursor/context_bundles.md`
- Normalized touched Rust source headers to the documented standard (`OWNERS` / `STATUS` / `API_STABILITY` / `TEST_COVERAGE` / `ADR`) and refreshed docs to describe the current proof state.

### Changed - 2026-04-23

#### TASK-0032 / RFC-0041 status synchronization (`TASK-0032`, `RFC-0041`)

- Updated execution/contract status to the requested review state:
  - `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` → `status: In Review`
  - `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md` → `Status: Done`
- Synced RFC index wording in `docs/rfcs/README.md`:
  - `RFC-0041` now tracked as `Done`
  - execution SSOT `TASK-0032` now tracked as `In Review`
- Synced task tracking views:
  - `tasks/IMPLEMENTATION-ORDER.md` now has an `In Review` section with `TASK-0032`
  - `tasks/STATUS-BOARD.md` queue head and contract-status lines now point to `TASK-0032` / `RFC-0041`
  - `tasks/STATUS-BOARD.md` cumulative done table now includes `TASK-0029` and `TASK-0031`
- Updated packaging documentation `docs/packaging/nxb.md` with explicit `pkgimg-build` / `pkgimg-verify` usage notes for PackageFS v2 image generation and verification.

### Changed - 2026-04-23

#### TASK-0032 prep sync + queue/workfile alignment (`TASK-0029`, `TASK-0031`, `TASK-0032`, `RFC-0041`)

- Added `TASK-0029` and `TASK-0031` to the cumulative Done table in `tasks/IMPLEMENTATION-ORDER.md`.
- Created RFC seed contract for the active SSOT task:
  - `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md`
- Linked the new seed from `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` and updated `docs/rfcs/README.md` index entries.
- Synced active task prep workfiles for `TASK-0032` posture:
  - `.cursor/context_bundles.md`
  - `.cursor/pre_flight.md`
  - `.cursor/stop_conditions.md`

### Changed - 2026-04-20

#### TASK-0023B Phase 6 functional closure + RFC-0038 → Done (`TASK-0023B`, `RFC-0038`)

- `TASK-0023B` advanced from `Draft` to `In Review` after Phase 6 (replay capability) reached functional closure across all six cuts.
- `RFC-0038` advanced from `Draft` to `Done`. One environmental closure step remains and is documented inline in the RFC header: external CI-runner replay artifact for P6-05; recipe lives in `docs/testing/replay-and-bisect.md` §7-§11.
- Phase 6 deliverables (cuts P6-01 → P6-06) shipped:
  - `tools/replay-evidence.sh` — bounded `--max-seconds` replay with hard env-override gate (`PROFILE` / `SELFTEST_PROFILE` / `RUN_PHASE` / `REQUIRE_*` / `KERNEL_CMDLINE` rejected), persistent worktree (`target/replay-worktree`) + Cargo cache reuse, automatic `NEXUS_SKIP_BUILD=1` warm-replay (cold ~67s, warm ~14s on dev box), structured logs, deterministic `nexus-evidence` / `nexus-proof-manifest` binary resolution.
  - `tools/diff-traces.sh` + `docs/testing/trace-diff-format.md` + `docs/testing/trace-diff-fixtures.json` — phase-aware classifier with `exact_match` / `extra_marker` / `missing_marker` / `reorder` / `phase_mismatch` classes.
  - `tools/bisect-evidence.sh` — bounded binary-search bisect with mandatory `--max-commits` + `--max-seconds`; synthetic mode extended to `good | drift | bad` so allowlist-absorbed drift is reported separately from regressions.
  - `scripts/regression-bisect.sh` — CI-friendly wrapper.
  - `docs/testing/replay-and-bisect.md` — operator workflow, append-only allowlist policy, evidence-map (§9), synthetic bad-bundle reproducer (§10), and the explicit remaining environmental step (§11).
- Phase-6 proof floor verified locally with reproducible artifacts:
  - empty-diff replay vs good bundle on native (`.cursor/replay-dev-a.json`) and containerized CI-like host (`.cursor/replay-ci-like.json`),
  - synthetic bad-bundle (tampered + re-sealed) classified diff with non-zero exit (`.cursor/replay-synthetic-bad.{log,json}` — `status: "diff", classes: ["missing_marker"]`),
  - 3-commit good→drift→regress bisect smoke (`.cursor/bisect-good-drift-regress.json` — `first_bad_commit: c2cccccc`, `drift_commits: [c1bbbbbb]`),
  - all hard gates verified (`--max-seconds`/`--max-commits` mandatory exits; `PROFILE` env override rejected with explicit error).
- Status synchronized across:
  - `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
  - `docs/rfcs/README.md`
  - `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `docs/adr/0027-selftest-client-two-axis-architecture.md` (Current state section refreshed; ADR remains `Accepted` because Phase 4-6 work consumes the two-axis structure rather than altering it)
  - `docs/testing/index.md` (RFC-0038 added to Related RFCs; topic guides extended with §9-§11 anchors)
  - `source/apps/selftest-client/README.md` (Status section rewritten with full P1-P6 closure table + remaining environmental closure step)
  - `.cursor/handoff/current.md`, `.cursor/current_state.md`, `.cursor/next_task_prep.md`
- Sequencing: queue head moves to `TASK-0024` (DSoftBus QUIC recovery / UDP-sec) once the external CI-runner replay artifact for P6-05 is captured and the documented status flip is applied.

### Changed - 2026-04-15

#### TASK-0023 gate-prep sync (`TASK-0023`)

- Archived `.cursor/handoff/current.md` snapshot to `.cursor/handoff/archive/TASK-0022-dsoftbus-core-no-std-transport-refactor.md`.
- Synchronized `TASK-0023` to explicit blocked-state truth:
  - follow-up routing now explicit (`TASK-0024`, `TASK-0044`),
  - RED feasibility point resolved as documented gate outcome,
  - security proof test names aligned to existing host reject suites.
- Updated active workfiles and queue docs for production-grade anti-drift clarity (`.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/pre_flight.md`, `.cursor/stop_conditions.md`, `tasks/IMPLEMENTATION-ORDER.md`, `tasks/STATUS-BOARD.md`).
- Synced architecture/distributed docs that still referenced `TASK-0022` review state:
  - `docs/architecture/README.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/distributed/dsoftbus-lite.md`

#### TASK-0022 closure sync (`TASK-0022`, `RFC-0036`)

- `TASK-0022` is now `Done` after final production-quality verification and closure sync.
- `RFC-0036` is `Complete` and remains aligned as the closed contract seed for this slice.
- `TASK-0023` gated-contract closure is now done with blocked/no-go unlock outcome; sequential queue head is `TASK-0024` unless resequenced.
- `dsoftbus-core` crate boundary and review evidence synchronized into process docs:
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `tasks/STATUS-BOARD.md`
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
- Fresh quality/security/performance verification pass run:
  - `cargo +nightly-2025-01-15 check -p dsoftbus-core --target riscv64imac-unknown-none-elf`
  - `cargo test -p dsoftbus --test core_contract_rejects -- --nocapture`
  - `cargo test -p dsoftbus -- reject --nocapture`
  - `just test-dsoftbus-quic`
  - `just deny-check`
  - `just dep-gate && just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `just test-e2e && just test-os-dhcp`

### Changed - 2026-04-14

#### DSoftBus QUIC host-first closure sync (`TASK-0021`, `RFC-0035`)

- `TASK-0021` advanced from `In Review` to `Done`.
- Queue head advanced to `TASK-0022`.
- Closure state synchronized across task/board/workfiles:
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
  - `README.md`
- Cargo-deny duplicate handling is now explicit and strict:
  - `multiple-versions = "deny"` remains enforced,
  - narrow compatibility skips were added only for `getrandom` (`0.2/0.3`) and `windows-sys` (`0.52/0.61`).
- Fresh green gate evidence includes:
  - `just test-os-dhcp`
  - `just test-dsoftbus-host`
  - `just test-all`
  - `just deny-check`

### Changed - 2026-04-10

#### DSoftBus mux v2 production closure (`TASK-0020`, `RFC-0033`, `RFC-0034`)

- `TASK-0020` is closed as `Done` with host, single-VM, and 2-VM marker proofs plus deterministic perf/soak and release-evidence artifacts.
- `RFC-0033` status is now `Complete` (mux v2 contract closure).
- `RFC-0034` status is now `Complete` for legacy `TASK-0001..0020` production-closure scope.
- Sequential queue head moved to `TASK-0021` after `TASK-0020` closeout.

### Changed - 2026-03-27

#### DSoftBus mux v2 kickoff (`TASK-0020`, `RFC-0033`)

- Verified `TASK-0019` closeout remains documented as `Done` across task status, board views, and changelog evidence.
- Moved `TASK-0020` to `In Progress` as the active sequential queue head.
- Moved `RFC-0033` to `In Progress` with `TASK-0020` as execution SSOT.
- Synced working-state artifacts for active execution context:
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
  - `.cursor/pre_flight.md`
  - `.cursor/stop_conditions.md`
  - `.cursor/context_bundles.md`

### Changed - 2026-03-27

#### ABI syscall guardrails v2 closeout (`TASK-0019`, `RFC-0032`)

- `TASK-0019` status advanced from `In Review` to `Done` after closing host/OS/QEMU proof gates.
- Workspace/task status sources were synchronized for drift-free closure:
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
  - `.cursor/pre_flight.md`
  - `.cursor/stop_conditions.md`
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- Root documentation now reflects closure and queue progression:
  - `README.md` (TASK-0019 done, next queue head TASK-0020)
- Additional green gate verification for this closeout:
  - `make build MODE=host`
  - `make test MODE=host`
  - `make run MODE=host RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s`

### Changed - 2026-03-26

#### Crashdump v1 final hardening closure sync (`TASK-0018`, `RFC-0031`)

- `TASK-0018` final hardening slice is now reflected across implementation + proof docs:
  - identity/report validation is fail-closed and deterministic,
  - explicit negative E2E markers are part of the canonical QEMU ladder:
    - `SELFTEST: minidump forged metadata rejected`
    - `SELFTEST: minidump no-artifact metadata rejected`
    - `SELFTEST: minidump mismatched build_id rejected`
- `execd` crash publish path now validates reported metadata against decoded bounded minidump bytes before emitting `execd: minidump written`.
- `statefsd` crash-write subject canonicalization is documented and unit-tested as a pure helper (narrow, path-bound mapping only; no broad SID-0 bypass).
- Task planning/status artifacts were synchronized for queue visibility and anti-drift:
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `tasks/STATUS-BOARD.md`
  - `.cursor` SSOT/handoff/pre-flight/stop-conditions files
- Verification set for this sync includes:
  - `cargo test -p crash -- --nocapture`
  - `cargo test -p execd -- --nocapture`
  - `cargo test -p minidump-host -- --nocapture`
  - `cargo test -p statefsd -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

### Changed - 2026-03-24

#### Networking modularization + address governance closure sync (`TASK-0016B`, `RFC-0029`, `ADR-0026`)

- `netstackd` modular refactor closure is now synchronized in docs and task/rfc state:
  - `main.rs` is entry/wiring only, with runtime split under `source/services/netstackd/src/os/**`.
  - handler and IPC helper seams are now the canonical extension points for follow-on networking tasks.
- Networking address/profile governance is now explicit and centralized:
  - `docs/architecture/network-address-matrix.md` is the SSOT for QEMU + os2vm address profiles.
  - `docs/adr/0026-network-address-profiles-and-validation.md` records policy-level decisions.
- DNS proof validation remains deterministic but is now protocol-semantic (port/QR/TXID) rather than source-IP-pinned, avoiding backend-specific false negatives.
- Task board and implementation-order docs were refreshed to match real task/RFC status progression (`TASK-0016` Done, `TASK-0016B` Complete, `RFC-0028` Completed, `RFC-0029` Completed).
- Verification set for this sync includes:
  - `just dep-gate`
  - `just diag-os`
  - `just test-os-dhcp-strict`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci RUN_PHASE=end tools/os2vm.sh`

### Changed - 2026-02-11

#### Perf/Power v1 closure (TASK-0013; RFC-0023 implemented)

- Kernel QoS syscall decode now deterministically rejects malformed/overflowed wire args with `-EINVAL` (no silent clamp).
- QoS authority model enforced and audited: self-set allows equal/lower only, escalation requires privileged `policyd/execd` path.
- New `timed` service path operational in OS bring-up with deterministic coalescing windows and bounded registration limits.
- Proof ladder extended and validated with deterministic markers, including negative over-limit and reject-path checks.
- Address-space/page-table lifecycle hardening landed during closure debugging to remove `KPGF`/allocation leak regressions in QEMU runs.

### Changed - 2026-02-10

#### Kernel SMP v1 closure sync (TASK-0012 Done; RFC-0021 Complete)

- Hardened SMP v1 proof semantics from marker-presence to causal anti-fake evidence:
  - `request accepted -> send_ipi success -> S_SOFT trap observed -> ack`
- Added deterministic SMP counterfactual proof marker:
  - `KSELFTEST: ipi counterfactual ok`
- Added/validated required SMP negative proof markers:
  - `KSELFTEST: test_reject_invalid_ipi_target_cpu ok`
  - `KSELFTEST: test_reject_offline_cpu_resched ok`
  - `KSELFTEST: test_reject_steal_above_bound ok`
  - `KSELFTEST: test_reject_steal_higher_qos ok`
- Canonical SMP harness gate now explicitly uses `REQUIRE_SMP=1` for SMP marker ladder runs.
- Documentation synchronized across task/rfc/testing/architecture/handoff to preserve drift-free follow-up prerequisites for TASK-0013/0042/0247/0283.

#### Build/QEMU reliability sync (default marker-driven run + blk lock serialization)

- `make run` now defaults to marker-driven mode (`RUN_UNTIL_MARKER=1`) so default runs complete green when the selftest ladder reaches `SELFTEST: end`.
- Added serialized lock handling for shared QEMU block image access in `scripts/run-qemu-rv64.sh` to avoid concurrent `blk.img` write-lock failures.

### Added - 2026-01-14

#### Observability v1 (TASK-0006: Complete)

**New Services**:
- `logd`: Bounded RAM journal for structured logs
  - Wire protocol v1: APPEND/QUERY/STATS (versioned byte frames for OS, Cap'n Proto for host)
  - Ring buffer semantics: drop-oldest on overflow, deterministic counters
  - Authenticated origin: `sender_service_id` from kernel IPC metadata
  - RFC: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (Complete)

**Logging Integration**:
- `nexus-log` extended with `logd` sink (`sink-logd` feature)
- Core services integrated: `samgrd`, `bundlemgrd`, `policyd`, `dsoftbusd`
- Existing UART readiness markers preserved for deterministic testing
- Fallback: UART-only if `logd` unavailable

**Crash Reporting**:
- `execd` crash reporting for non-zero exits
  - UART marker: `execd: crash report pid=<pid> code=<code> name=<name>`
  - Structured crash event appended to `logd` (queryable for post-mortem)
  - Stable crash event keys: `event=crash.v1`, `pid`, `code`, `name`, `recent_count`
  - Reserved keys for future: `build_id`, `dump_path`

**Testing**:
- Host tests: `cargo test -p logd`, `cargo test -p nexus-log`
- QEMU markers (all green as of 2026-01-14):
  - `logd: ready`
  - `SELFTEST: log query ok`
  - `SELFTEST: core services log ok`
  - `execd: crash report pid=... code=42 name=demo.exit42`
  - `SELFTEST: crash report ok`

**Documentation**:
- New: `docs/observability/logging.md` (usage guide)
- New: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (contract seed)
- Updated: `docs/architecture/` (10+ files), `docs/testing/index.md`, ADR-0017

**Demo Payloads**:
- `demo.exit42` added to `userspace/apps/demo-exit0` for crash report testing

**Breaking Changes**: None (additive only)

**Known Limitations (v1 scope)**:
- Journal is RAM-only (no persistence)
- No streaming/subscriptions (bounded queries only)
- No remote export (deferred to TASK-0040)
- No metrics/tracing integration (deferred to TASK-0014)

### Added - 2026-01-25

#### Policy authority + audit baseline v1 (TASK-0008: Done; RFC-0015: Complete)

- `policyd` established as the **single policy authority** with deny-by-default semantics.
- Audit trail for allow/deny decisions (via `logd`), binding authorization to kernel `sender_service_id`.
- Policy-gated sensitive operations (baseline): signing/exec/install paths enforced without duplicating authority logic.
- Contract: `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`

### Added - 2026-01-27

#### Device identity keys v1 (TASK-0008B: Done; RFC-0016: Done)

- OS/QEMU device identity key generation path proved without `getrandom`:
  - virtio-rng MMIO → `rngd` (entropy authority) → `keystored` (device keygen + pubkey-only export).
- Bounded entropy requests and negative proofs (oversized/denied/private-export reject); no secrets logged.
- Contract: `docs/rfcs/RFC-0016-device-identity-keys-v1.md`

### Added - 2026-02-02

#### Device MMIO access model v1 (TASK-0010: Done; RFC-0017: Done)

- Kernel/userspace contract for capability-gated device MMIO mapping (`DeviceMmio` + mapping syscall).
- Enforced security floor: USER|RW mappings only, never executable; bounded per-device windows; init/policyd control distribution.
- Contract: `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`

### Added - 2026-02-06

#### Persistence v1 (TASK-0009: Done; RFC-0018: Complete; RFC-0019: Complete)

- StateFS journal format v1 + `/state` authority service (`statefsd`) with deterministic host + QEMU proofs.
- IPC request/reply correlation v1 (nonces + bounded reply buffering) to keep shared-inbox flows deterministic under QEMU.
- Modern virtio-mmio default for virtio-blk in the canonical QEMU harness (legacy remains opt-in).
- Contracts:
  - `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
  - `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`

### Changed - 2026-02-09

#### Kernel simplification (TASK-0011: Complete; RFC-0001: Complete)

- Kernel tree reorganized into stable responsibility-aligned directories (mechanical moves + wiring only).
- Kernel module headers normalized; invariants and test scope made explicit to lower debug/navigation cost.
- Contract: `docs/rfcs/RFC-0001-kernel-simplification.md`

---

## Previous Releases

See Git history for releases prior to 2026-01-14.