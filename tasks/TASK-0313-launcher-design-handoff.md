---
title: TASK-0313 Launcher design handoff — Startmenü, Docks, Arbeitsfläche, Minimize 1:1
status: In Progress (2026-07-30) — Track A + Track B B0-B4 done & boot-proven (357 host tests, `just check` green); OPEN: marker-contract entries
owner: @ui
created: 2026-07-30
links:
  - Design contract: userspace/apps/desktop-shell/docs/design_handoff_launcher/LAUNCHER-SPEC.html
  - Handoff README: userspace/apps/desktop-shell/docs/design_handoff_launcher/README.md
  - Visual truth: userspace/apps/desktop-shell/docs/design_handoff_launcher/OpenNexusLauncher.html
  - Layout SSOT: docs/dev/ui/patterns/shell-launcher.md
  - Window intent model: docs/dev/ui/patterns/windowing/window-intent.md
  - windowd cleanup map: docs/dev/ui/windowd-cleanup-map.md (dock.rs = MOVE step 5)
  - Prior handoff port (pattern): tasks/TASK-0312-shell-panels-design-handoff.md
  - RFC seed (Track B): docs/rfcs/RFC-0086-shell-taskbar-window-feed.md
  - Playbook: CLAUDE.md
---

## Context

`userspace/apps/desktop-shell/docs/design_handoff_launcher/` is the complete
handoff for the **launcher shell**: app launcher (windowed 720×520 + fullscreen
paged), desktop taskbar / touch docks, and the desktop icon field. The topbar +
six status panels landed in TASK-0312. This task rebuilds the rest 1:1, with
our REAL registry apps (calculator · chat · settings · stash) instead of the
handoff's 48 dummy icons.

User requirements (2026-07-30):

1. Desktop mode: launcher has a SMALL (720×520 window) and BIG (fullscreen)
   state; tablet mode: fullscreen only.
2. The fullscreen launcher pages HORIZONTALLY (no vertical scroll): page dots
   at the bottom, mouse wheel turns pages. Real glide (not a content swap) —
   user decision.
3. Minimize goes to the taskbar (desktop) / dock (touch) — the hardcoded
   legacy dock in windowd (`dock.rs`, banner "MOVE → DSL-Shell-App, DO NOT
   EXTEND") is deleted.
4. Desktop icons: md/56px, desktop = top-left column, touch = 6-column grid.

Both tracks run in this task (user decision): Track A (pixel rebuild) first,
then Track B (minimize → taskbar platform work, RFC-0086).

## Decisions taken with the user

| Question | Decision |
|---|---|
| Pager v1 | Real glide: ScrollMomentum on X + host-side page snap (not content swap) |
| Track B scope | Complete in this task (B0–B5 after Track A) |
| Pixel fidelity | Same contract as TASK-0312: fixed sizes/radii exact, padding/gap on the 4px grid |

## Track A — launcher / icons / docks

- **A1 platform enablers**: expose the engine's `Grid` node as a DSL widget
  (2 lock-step SSOTs); app-icon bake sizes {88, 56, 48, 40}; `.scroll(paged)`
  token + host page-snap (ScrollMomentum X, wheel accumulate ≥30 → ±1 page +
  360ms lock, axis guard in `negotiated_band()`); `svc.shell.scrollToPage(i)`
  in-process effect; enumerate page-chunking → `PagesLoaded(List ×4, Int)`.
- **A2 AppIcon family**: per-size stateless components (xs 40 taskbar · sm 48
  dock · md 56 workspace/window menu · xl 88 fullscreen), running dot 4×4 /
  taskbar underline 20×2 props.
- **A3 icon field**: desktop = top-left column (no scroll), touch =
  `Grid{columns:6}` gap 24×16 padding 28.
- **A4 launcher = overlay, not route**: PanelStore gains `"launcher"` /
  `"launcher-big"` (exclusivity for free); windowed 720×520 exact + expand
  button; fullscreen with greeting (real session user) + scrim.
- **A5 pager**: `.direction(row).scroll(paged)` track, ≤4 page cells
  (if-guarded), `Grid` per page with xl icons; dots 7×7 / active 18×7;
  `PageNext`/`PagePrev` host triggers ↔ `SetPage(i)` dot taps.
- **A6 dock/taskbar pixel pass** per spec §6/§8.

## Track B — minimize → taskbar/dock (RFC-0086)

Identity keystone: execd spawns app-hosts via `exec_v2` with service name
`"app:"+app_id` (kernel-stamped sender id; `app:` prefix prevents boot-service
impersonation). windowd stays app-agnostic; the sid↔app-id join happens in the
shell's app-host via `service_id_from_name`.

- **B0** RFC-0086 seed (ops 27/28, authority = desktop-surface owner sid,
  retained latest-wins delivery, threat model).
- **B1** identity floor: execd exec_v2 naming; windowd `desktop_owner_sid`
  capture + owner checks in `CONTROL_WIN_*` (closes the recorded follow-up);
  `test_reject_*`.
- **B2** window feed `OP_SURFACE_WINDOWS = 27` (windowd → desktop channel).
- **B3** shell consumption (`WindowsChanged` trigger, enumerate merge
  running/minimized/focused, `svc.shell.activate` with launch fallback) +
  `OP_SURFACE_TASKBAR = 28` gate (reject tests ×3).
- **B4** delete legacy dock (dock.rs + wm/input/transitions/assets remains),
  minimize animation retargets to `(w/2, h − SHELL_TASKBAR_H/2)`.
- **B5** e2e marker chain (markers.txt + qemu-test.sh together) + docs sweep.

Hard ordering: B1 before B3; B4 never before B3 (a minimized window must never
be unreachable).

## Known deltas (to be recorded in shell-launcher.md when landing)

- Pager: no 1:1 finger drag/swipe (wire has no drag kinds); glide curve is the
  ScrollMomentum approximation of `cubic-bezier(0.22,1,0.36,1)`; page cap 4.
- Grid cols/rows are per-arm constants, not measured (no container measurement
  primitive; QEMU mode is fixed 1280×800).
- Desktop icon field: single column until >~8 apps (engine column-wrap still
  pending); label px sizes ride the existing type ladder.

## Verification

Host-first per package (`just check`, dsl-runtime/layout tests, app-host unit
tests), then boot proof (`just start`, visual + markers). Track B stage B5 adds
the marker chain launch → windows push → minimize → push → activate → restore.
Commit proposal per verified package (user commits).

## Log

- 2026-07-30: Planning complete (3 exploration reports + protocol design);
  task created; Track A starting.
- 2026-07-30: **Track A complete, boot-proven both modes** (visible boot,
  VNC-driven):
  - A1 platform: `Grid` widget + `.columns(n)`/`.rowGap(n)` modifiers (the
    engine's `LayoutNode::Grid`, data-driven via `List`); `skip`/`take`/`len`
    list builtins (IR `ListOpKind` 10/11 + `len` impl) — page slices out of
    ONE store list; `.scroll(paged)` + `ScrollAxis::Paged`; host pager
    (`pager_math` pure module, 8 tests: one notch = one page, 360ms lock,
    edge clamps, target-based mid-glide turns) + ScrollMomentum-X glide;
    band axis guard (horizontal/paged never banded — banded kills wheel);
    `svc.shell.scrollToPage` in-process effect; `PageNext`/`PagePrev`
    triggers; app-icon bake sizes {88, 64, 56, 48, 40}.
  - A2–A6 shell (enterprise component structure): tiles
    (AppTile md 56 / DockTile sm 48 / TaskbarTile xs 40 + underline slot /
    LauncherTile xl 88), `workspace/` (Column desktop · Grid 6-col touch),
    `launcher/` (Host overlay via PanelStore "launcher"/"launcher-big" ·
    Window 720×520 · Fullscreen + greeting · Pager ≤4 pages · PagerDots),
    `dock/` (Taskbar · DockWide · DockRegular · DockCompact); `/launcher`
    route deleted; pages are thin compositions.
  - Boot-proof: desktop windowed launcher (720×520, search+expand, 4-col
    grid, footer) ✓ · expand→fullscreen (greeting, xl grid, active page
    pill) ✓ · compress→window ✓ · ✕/outside-tap close ✓ · glyph toggle ✓ ·
    CC exclusivity (opening CC closes launcher) ✓ · mode switch → touch
    shell: 6-column workspace grid + three floating dock elements ✓ · touch
    fullscreen launcher (✕ only, no compress) ✓ · app launch from grid ✓.
  - Observed (not a blocker): multi-second full-frame present latency on TCG
    after big overlay swaps — pre-existing compositor perf reality.
- Backlog (phase 2): launcher auto-close after successful app launch;
  loading-flash on TextField mount (`QueryChanged` fires once — cosmetic);
  page reset on search while paged.
- 2026-07-30: **Track B implemented** (B0–B4; host gates green: 340 tests,
  `just check` incl. structure ratchet, OS cross-build clean):
  - B0 RFC-0086 seed + index entry.
  - B1 identity floor: execd spawns app-hosts via `exec_v2` with
    `app:<bundle_id>` (pure `execd::identity`, 4 tests); windowd captures
    `desktop_owner_sid` at desktop SURFACE_CREATE and gates every
    `CONTROL_WIN_*` on the window's `owner_sid` — closing the recorded
    spoofable-value-byte follow-up. `control_gate` reject tables (6 tests:
    `test_reject_win_control_foreign_surface`/`_sid_zero`,
    `test_reject_taskbar_verb_from_non_desktop_owner`/`_sid_zero`).
  - B2 feed: `nexus-display-proto/surface_windows.rs` (ops 27/28, 5 tests
    incl. count-lie + oversize rejects); windowd `windows_feed` — retained
    latest-wins, deduped, NONBLOCK + owed retry on the presentation pump,
    re-push on desktop (re)bind; pure `window_feed` derivation (5 tests:
    sid-0 slots omitted, minimized never focused, bounded).
  - B3 consumption: app-host caches the set, fires `WindowsChanged`
    (trigger whitelist +1), merges running/minimized/focused into every
    enumerate row via `service_id_from_name("app:"+id)`;
    `svc.shell.activate` = restore/raise when running, else launch;
    windowd op-28 handler behind the sid gate. Shell: `Activate` +
    `WindowsMoved` events, running dot/underline on all tile families.
  - B4 legacy dock DELETED (`dock.rs`, wm/input/scene/assets/transitions
    remains, `DOCK_*` icon bakes); minimize/restore now fly to
    `taskbar_anchor` = `(w/2, h − SHELL_TASKBAR_H/2)`; cleanup-map row
    marked done. Four dock-orphaned dead fns removed (no blanket allows).
  - **Bug the tests caught**: `on WindowsChanged -> dispatch(Refresh)` made
    `Refresh` non-root, silently killing the mount-time app load (root
    effect = "dispatched by nothing"). Split into a `WindowsMoved` event
    with its own enumerate effect; `Refresh` stays the root.
- 2026-07-30: **Track B BOOT-PROVEN end to end** (visible boot, VNC-driven):
  - desktop bind → `windowd: windows push (n=0)`
  - workspace tile tap → `apphost: shell activate -> launch` (nothing
    running yet — the fallback) → window opens → `windows push (n=1)`
  - the taskbar tile renders the §7 running UNDERLINE and the workspace tile
    the running dot, both fed by the merge
  - window-chrome minimize → `windowd: minimize id=app0` → `windows push
    (n=1)` (still open, now minimized) — the window stays reachable
  - **taskbar tile click → `windowd: taskbar activate ok` → `windowd:
    restore id=app0`**, window flies back in from the taskbar anchor.
  - **Regression found and fixed on device**: giving app-hosts identity broke
    `bundlemgrd`, whose apps-listing gate accepted `sender_service_id == 0`
    ("app-hosts carry no identity yet", with the follow-up recorded in-file)
    — the shell's app list went empty. The gate now resolves the sender
    against the same registry it lists
    (`bundlemgrd::app_sender::is_registered_app_host`, 4 tests incl.
    `test_reject_unnamed_sender`) — strictly tighter than the old `== 0`.
    Recorded in RFC-0086 under "Fallout of giving app-hosts identity".
  - Also fixed: the feed's OPEN path was unwired (only close/minimize/
    restore/focus pushed), so a fresh window never entered the set —
    `show_window` now publishes.
- OPEN: `markers.txt` / `qemu-test.sh` contract entries for the new markers.
  They are boot-proven above, but the contract file additionally requires a
  simulated service contract under `tools/nx/src/chain/contract/` (the
  `contract_covers_markers` gate), which is its own mechanical piece of work.
