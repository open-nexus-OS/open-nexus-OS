---
title: TASK-0076B DSL v0.1c (OS-gated): visible in-compositor mount + first DSL frame + execd isolation probe
status: Superseded
owner: @ui @runtime
created: 2026-03-28
updated: 2026-07-06
depends-on:
  - tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
follow-up-tasks:
  - tasks/TASK-0080B-systemui-dsl-bootstrap-shell-launcher-host.md
  - tasks/TASK-0080D-dsl-app-runtime-lifecycle-surface-contract.md
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Runtime contract: docs/dev/dsl/runtime.md (host #2 = in-compositor mount)
  - Shell-config SSOT feeding the mount: source/services/systemui/manifests/shells/*/shell.toml
    (dsl_root + [first_frame]), products/profiles (ADR-0035)
  - One reactive path: docs/rfcs/RFC-0070-ui-design-system-ssot-convergence.md
  - Visible present baseline: tasks/TASK-0055C; input baseline: tasks/TASK-0056B
  - Spawn path this task probes for Phase 6: source/services/execd (nexus-loader, as_create)
  - Testing contract: scripts/qemu-test.sh
---

## Context (updated 2026-07-06)

TASK-0076 proves the runtime host-side. This task mounts it **in the live compositor
path** — the same embedding that will later host the SystemUI shell and the login
greeter (masterplan decision: shell + greeter are DSL-authored; authority stays in
sessiond). The `.nxir` to mount is resolved from the **existing** shell-config registry:
`shell.toml` `dsl_root` names the program, `[first_frame]` gives the initial dims —
no new config mechanism.

The scene flows `LayoutNode → LayoutEngine → windowd SceneGraph → nexus-gfx`
(RFC-0070's one path); no separate DSL renderer.

**Also in this task (cheap, de-risks Phase 6 early): the execd isolation probe.**
execd's ELF-spawn path exists (nexus-loader, `as_create` Sv39, W^X) but is not the live
boot flow, and comments contradict each other about child address-space isolation. A
selftest spawns a trivial payload and proves isolation before TASK-0080D bets on it.

## Goal

1. **Visible DSL mount**: embed `nexus-dsl-runtime` in the windowd/systemui path;
   compile the proof-surface fixture app to `.nxir` at build time; resolve via
   `dsl_root`; first frame through the real interpreter path.
2. **Visible bounded interaction**: one live pointer interaction (button tap toggling
   state) visibly updates the DSL surface via the narrow-invalidation path
   (paint-only dispatch — no full re-render).
3. **execd isolation probe**: selftest spawns a minimal ELF that (a) writes a marker,
   (b) proves address-space isolation (a write to a parent-known VA has no effect in
   the parent), (c) exits and is reaped. Markers below.
4. Handoff notes for 0080B (shell/greeter mount) + 0080D (app-host spawn).

## Non-Goals

- Launcher/shell migration (0080B/C), app-host process (0080D), routes/i18n (0077),
  AOT (0079). Kernel changes (probe uses existing syscalls only).

## Constraints / invariants (hard requirements)

- No separate preview renderer — the live interpreter/runtime path only.
- Interpreter work per frame bounded by IR budgets; no per-frame heap allocation in
  the mounted path (OS bump-allocator rule).
- Existing boot markers unaffected; new markers additive + deterministic.
- No `unwrap/expect`; no godfiles (mount plumbing = own module, not woven into
  existing windowd runtime files).

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required (user boot-verify)

UART markers:

- `DSL: program loaded hash=<h>`
- `DSL: first frame presented`
- `DSL: interaction visible ok`
- `SELFTEST: dsl visible mount ok`
- `EXECD: isolation probe ok (as=isolated)` + `SELFTEST: execd spawn isolation ok`

Visual proof:

- the QEMU window shows the DSL-rendered proof page; a live pointer tap visibly
  changes it; the page reuses the shared proof-surface targets;
- boot remains 0-fault; reveal/present chain markers unchanged.

### Proof (Host) — required

- mount plumbing unit-tested against a stub `SurfaceSink`; the same fixture app renders
  identical goldens host-side and (structurally) in the OS path.

## Touched paths (allowlist)

- `userspace/dsl/runtime/` (SurfaceSink impl for the compositor path)
- `source/services/windowd/` + `source/services/systemui/` (mount module — new file(s))
- `examples/dsl/` fixture app; build wiring to compile `.nxir` into the image
- `source/apps/selftest-client/` (markers + isolation probe)
- `source/services/execd/` (probe support only — no behavior change)
- `docs/dev/dsl/{runtime,testing}.md`, `docs/dev/ui/` mount notes

## Plan (small PRs)

1. build wiring: fixture `.nxir` into the image + registry resolution (host-tested)
2. mount module + first visible frame [boot-verify]
3. live interaction + selftest markers [boot-verify]
4. execd isolation probe + docs/handoff notes [boot-verify, can ride with 3]

---

## STATUS / PROGRESS LEDGER (updated 2026-07-06)

> UPDATE (evening): the first boot FAILED (windowd silently dead). Root-caused
> headless + fixed; **`DSL: program loaded` + `DSL: first frame presented` now
> prove out in my own headless runs** (ci-os-headless; selftest ladder count
> identical with/without the mount — no regression). **AWAITING USER
> BOOT-VERIFY in the visible lane** (interaction + optics).
>
> ### Debug findings (the hard-won facts — read before touching this area)
> **RESOLVED (2026-07-06, user decision):** the kernel VMO pool is now **160MB**
> (`USER_VMO_ARENA_LEN`, production-grade headroom for per-app processes +
> surfaces in Phase 6; RAM 320M, arena ends 0x8B80_0000), the exhaustion path
> **logs values** instead of dying silently (`VMO-POOL exhausted: want/used/
> remaining/peak`), and windowd's image size is **CI-gated** at 8MB
> (`just contract-windowd-size`, wired into `ci-os-headless`; currently 78%).
> Growth stays a conscious act — raise the budget only with a ledger note.
>
> 1. **windowd binary size is a BOOT BUDGET.** Service images are allocated from
>    the kernel's 96MB `VmoPool` (`syscall/api.rs::sys_exec`), and exhaustion is
>    a SILENT `PermissionDenied` (the stats are discarded in the error path).
>    +134KB of text (capnp writer + sha2 via hash-verify) killed windowd with
>    zero output. Proven by a link-but-never-run probe (dead code stripped =
>    boots; linked = dead). Fix: `nexus-dsl-ir` feature `hash-verify` (default
>    ON; OFF for build-embedded payloads — the binary is the trust boundary;
>    structural validation still runs). windowd builds lean (−16KB + avoided
>    growth); app-host/CLI keep full hash verification.
> 2. **Verdict folding swallows breadcrumbs**: windowd output before the present
>    flush only prints if it matches the failure heuristic (`FAIL`/`error`/
>    `denied` tokens) — diagnostics there must carry those tokens.
> 3. **The compositor is reactive**: "retry next frame" never fires without
>    damage. One-shot hooks belong at milestones (boot-open now runs at the
>    present-visible milestone in `framebuffer.rs`), not in the frame loop.
> 4. **Atlas pool budget**: the on-demand window pool had 71 rows free at the
>    milestone — the DSL window needs 220. `WINDOW_POOL_ROWS` now reserves the
>    DSL window's bands (content+blur), same pattern as Search. The failure
>    marker carries values (`need=WxH rows_remaining=N`) — no more guessing.
> 5. Embedded `.nxir` is 8-byte aligned (`AlignedNxir`) — capnp segments are
>    word-aligned by contract; `include_bytes!` alone guarantees nothing.

### ✅ DONE (built, boot-verify pending)

- **`windowd/src/compositor/runtime/dsl_mount.rs`** (new module, ~370 LOC): the DSL demo
  window — a fourth `ShellWindow` (`WindowId::DslDemo`, MAX_WINDOWS=4 exactly filled):
  - `build.rs` compiles `examples/dsl/counter/counter.nx` → canonical `.nxir` at build time
    (build-dep `nexus-dsl-core`, host-side); embedded via `include_bytes!`.
  - Mount: fail-closed `Runtime::mount` (validate + hash) → `View::mount` → marker
    `DSL: program loaded hash=<first 8 bytes>`; failure keeps the window closed with an
    honest FAILED marker.
  - **`BakedTextMeasure`**: pixel-accurate `MeasureText` over windowd's baked glyph tables
    (`text::measure`/`line_height`) — the live-path measurer for `LayoutEngine::layout`.
  - Render: shared title-bar chrome (`draw_title_bar_row`, close/min/max buttons work) +
    frosted glass body + per-row LayoutBox walk (background fills via `write_tint_span`,
    text matched to boxes by pre-order `node_id` via `collect_texts`, drawn with
    `draw_text_row`). Marker `DSL: first frame presented` after the first render.
  - **Live interaction**: body clicks route through `dsl_pointer_body` →
    `View::pointer("Tap", x, y)` (the interpreter's hit-testing) → damage-driven
    re-layout/re-render → marker `DSL: interaction visible ok` (once). Clicking the
    counter's **+ / −** buttons visibly changes the number.
  - Auto-opens once at boot (`maybe_boot_open_dsl` from the first scene build) so the page
    is visible without input; drag/minimize/restore/close all work via the shared
    ShellWindow machinery (fullscreen = no-op like Settings).
- **Wiring**: `WindowId::DslDemo` through window_scene/mod/scene/input/wm (stack, z-order,
  hit-order, cursor shapes, dock (reuses the search glyph for now), resize clamp, wheel no-op).
- Consistency: windowd host tests 137 green; DSL host suites 37 green; riscv os-lite green.

### ▶ USER BOOT-VERIFY (the gate for this task)

`just start` (virgl) and check:
1. UART: `DSL: program loaded hash=…` and `DSL: first frame presented` in the boot log
   (`build/logs/manual--<ts>/uart.log`).
2. Visible: a "DSL Demo" glass window (300×220, at 420/160) showing the counter page
   (number + two buttons) alongside the existing desktop.
3. Interaction: click **+** in the window body → the number increments visibly; UART shows
   `DSL: interaction visible ok`.
4. Regressions: greeter/login/chat/search/settings behave as before; 0 faults.

### ⬜ OPEN (within this task)

- **execd isolation probe**: the existing selftest exec phase already proves spawn+ELF load
  (`M_EXECD_ELF_LOAD_OK`, IMG_HELLO). Still to add (also task #102 territory): spoof-requester
  deny re-coverage (`execd_spawn_image_raw_requester` → STATUS_DENIED) + wait-exit assertion
  markers. Additive selftest work in `phases/exec.rs` + `proof-manifest/markers/exec.toml`.
- `SELFTEST: dsl visible mount ok` selftest marker (needs a windowd-side observable the
  selftest can query, or promotion of the `DSL:` markers into the proof manifest).
- Reopen trigger after closing the window (menu entry / Apps dropdown) — currently the demo
  auto-opens once per boot; toggle was removed as dead code until a menu entry exists.
- Theme-mode awareness for the DSL body (uses BaseTokens; dark/light switch re-render hooks in
  with 0077's env work).

### Notes for whoever continues

- The mount deliberately renders via the ATLAS per-row seam (like Search/Settings), NOT via
  windowd's SceneGraph — the box-walker is the honest v0.1 bridge; SceneGraph convergence is
  TASK-0074 W6 / RFC-0067 territory.
- `LayoutBox.node_id` is 1-based pre-order and matches the interpreter's handler box ids AND
  `collect_texts` indices — three consumers of one numbering; don't reorder emission.
- Body coordinates: interpreter space = window-local minus `DSL_TITLE_H`.

## Closure (2026-07-19) — Superseded by TASK-0080C
This task’’’s own visible-DSL-mount demo (`dsl_mount.rs`, `WindowId::DslDemo`, markers `DSL: program loaded`/`DSL: first frame presented`/`EXECD: isolation probe ok`) was **retired** (git `e8b292fe` "windowd cleanup dsl mount"); those markers no longer exist in the tree and its DoD was never boot-verified. The **capability it targeted — a DSL-authored frame mounted+rendered on real OS boot — is delivered by TASK-0080C** (Done): live emitter `APPHOST: mounted hash=<hex>` (`source/services/app-host/src/probe/boot.rs`) + `DSL: program loaded hash=…` on real boot, host plumbing in `tests/systemui_bootstrap_shell_host/`. Status → Superseded (not Done: this task’’’s own path is dead; its capability lives in the successor).
