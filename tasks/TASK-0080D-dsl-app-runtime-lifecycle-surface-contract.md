---
title: "TASK-0080D DSL App Runtime: app-host process + packaging (.nxir in .nxb) + execd spawn + cross-process surface (ADR-0042)"
status: Draft
owner: "@ui @runtime"
created: 2026-06-23
updated: 2026-07-06
depends-on:
  - tasks/TASK-0076B-dsl-v0_1c-visible-os-mount-first-frame.md   # visible mount + the execd isolation probe
  - tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md          # svc adapters the effect host speaks
  - tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md  # abilitymgr/bundlemgrd spine
follow-up-tasks:
  - tasks/TASK-0080C-systemui-dsl-bootstrap-shell-os-wiring.md   # launcher e2e consumes this runtime
  - tasks/TASK-0079-dsl-v0_3a-aot-codegen-incremental-assets.md  # AOT ELFs ride the same payloadKind dispatch
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Surface transport (drafted, finalize here): docs/adr/0042-cross-process-surface-transport.md
  - Per-app surface ownership: docs/adr/0037-per-app-surface-lazy-vmo-lifecycle.md
  - Service split: docs/adr/0036; lifecycle RFC: docs/rfcs/RFC-0065
  - Display wire SSOT: source/libs/nexus-display-proto (ADR-0038)
  - Packaging (EXISTS): docs/packaging/nxb.md, tools/nexus-idl/schemas/manifest.capnp,
    tools/nxb-pack, source/services/{bundlemgrd,packagefsd}
  - Spawn path (EXISTS, probe-gated): source/services/execd + userspace/nexus-loader
  - Lifecycle contract page: docs/dev/dsl/runtime.md
---

## Context (updated 2026-07-06)

**Masterplan decision: the v1 app runtime is a real separate process** — one optimized
`app_host` runtime ELF; execd spawns it per app; it loads the app's compiled `.nxir`
(zero-parse capnp) from the installed bundle and presents through its own
cross-process windowd surface (ADR-0042). This supersedes this task's earlier
"v1 = in-process host" fallback.

**Reality check on the spawn constraint (corrected):** the 2026-06-23 note said execd
only runs hand-assembled stubs. That describes the os-lite demo image table; the real
ELF spawn path EXISTS (`exec_elf`: GET_PAYLOAD → nexus-loader `parse_elf64_riscv` →
`as_create` Sv39 → VMO staging → W^X → `spawn` + `cap_transfer` + restart reaper) but
is **not the live boot flow** and carries a contradictory shared-address-space comment.
That is exactly what the **TASK-0076B isolation probe** settles first. If the probe
fails, a dedicated execd-hardening sub-task blocks this task (and only this task —
everything else is host-side).

What already exists and is built on, not around: build-time `APP_REGISTRY` from
`bundles/<app>/manifest.toml`; abilitymgr `launch_with_caps` fail-closed
(`KNOWN_PERMISSIONS = nexus.permission.{WINDOW,NOTIFY,STATE}` + QUERY from 0078B);
`.nxb` bundles + `nxb-pack` (payload today = placeholder ELF); packagefsd;
windowd `create_surface`/`destroy_surface` + `app_surface` residency (ADR-0037).

## Goal

1. **Packaging**: `manifest.capnp` v2.1 gains `payloadKind` (append-only field;
   `elf | uiProgram`); `nxb-pack` packs `payload.nxir` for DSL apps; bundlemgrd
   install/digest/enumerate unchanged (payload-agnostic).
2. **app_host** (`source/services/app-host/`, no_std, modules pinned: `payload.rs`,
   `surface.rs`, `input.rs`, `render_loop.rs`, `effect_host/` (one module per svc),
   `persist.rs`): argv `<name@ver>` → fetch payload (v1 GET_PAYLOAD; packagefs
   VMO-map recorded as the zero-copy upgrade) → validate IR (nexus-dsl-ir, fail-closed)
   → mount (nexus-dsl-runtime) → ADR-0042 surface → render loop (nexus-layout +
   renderer into the surface VMO; arenas at mount, steady-state zero-alloc,
   debug alloc-assert) → present with damage → input events in → effects over real
   IPC adapters.
3. **execd dispatch**: `payloadKind == uiProgram` ⇒ spawn app_host with the app id +
   transfer the granted caps (windowd client cap gated by `WINDOW`, statefsd by
   `STATE`, queryd by `QUERY`); restart policy per manifest.
4. **windowd transport (ADR-0042 finalized here)**: `SURFACE_CREATE/PRESENT/DESTROY`
   in nexus-display-proto; damage-blit into the atlas region backing the app's layer;
   seq/ack flow control; input routed by surface id. New windowd code in its own
   modules (`client_surface.rs`, `surface_transport.rs`) — no godfile growth.
5. **Lifecycle**: RFC-0065 hop order observable (registry → caps → launch → mount →
   visible); suspend persists `@persist` fields via statefsd; stop destroys the
   surface (ADR-0037 lazy residency); reaper handles crashes per restart policy.
6. **Probe-first sequencing**: R1 is a solid-color app_host (no DSL) proving
   spawn + VMO + present before anything else.

## Non-Goals

- Launcher/shell wiring (0080C). AOT codegen (0079 — but its ELFs launch through the
  same payloadKind dispatch, forward-compatible here). Notifications/state permission
  service-side enforcement beyond declaration (RFC-0065 follow-up). Migrating
  chat/search content apps (follow-up wave). Kernel changes (probe-driven execd fixes
  excepted, staged separately if needed).

## Constraints / invariants (hard requirements)

- **Manifest = the app's only identity/caps source**; registry + launch authority
  derive from it; a DSL app without a manifest does not exist.
- **Fail-closed everywhere**: unknown permission ⇒ denied (existing); invalid/tampered
  IR (hash/type/budget) ⇒ deterministic launch error, never a partial mount.
- **One launch path** through abilitymgr — no `mount()` shortcut in live paths (host
  fixtures may mount directly for snapshots only).
- **Apps get pixels + events, nothing else**: no scene-graph access, no shell atlas
  writes (information hiding at the process boundary; ADR-0037/0042).
- Bounded: per-frame work capped by IR budgets; one in-flight present; damage list
  capped; `MAX_APP_SURFACES` respected.
- Steady-state zero heap allocation in app_host (bump-allocator rule; debug assert).
- No `unwrap/expect`; no godfiles; no company/product names.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- app_host logic host-tested: payload validate/mount against fixtures; effect host
  against transcripts; surface module against a stub sink (frame bytes golden);
  manifest payloadKind round-trip (nxb-pack → parse); registry/manifest cross-check
  (extends the existing `repo_bundles` test).

### Proof (OS/QEMU) — required (user boot-verify, staged)

Marker chain in hop order:

- R1: `APPHOST: probe surface presented` + visible solid-color window (no DSL)
- `windowd: apps ok (n=…)` → `abilitymgr: caps ok app=<id> (n=…)` →
  `abilitymgr: launch (app=<id>, inst=…)` → `APPHOST: mounted <name>@<ver> hash=<h>` →
  `WINDOWD: surface presented id=<n> seq=<k>` → `dsl: app surface visible`
- input: click inside the app window dispatches an event visibly (state change on
  screen); no background leak to the shell
- suspend/stop: `APPHOST: persisted (fields=<n>)` + surface destroyed
  (ADR-0037 marker); crash → reaper restart per policy
- denied fixture app (unknown permission) refused with the existing denial marker

### Docs — required

- ADR-0042 Status → Accepted (with any deviations recorded);
  `docs/dev/dsl/runtime.md` lifecycle/launch sections final;
  `docs/packaging/nxb.md` payloadKind section.

## Phasing (each boot-verified)

- **R1 — transport probe**: solid-color app_host spawned by execd, ADR-0042
  create/present, visible window. De-risks everything.
- **R2 — real payload**: manifest v2.1 + nxb-pack `.nxir` + bundlemgrd fetch +
  validate/mount + first DSL app frame (counter demo).
- **R3 — interaction + lifecycle**: input routing, effects over real IPC, `@persist`
  suspend/restore, stop/crash residency.
- **R4 — AOT forward-compat**: payloadKind dispatch verified against a generated-ELF
  bundle (lands with 0079/0080).

## Notes

The DSL gives apps a shape; RFC-0065 gives identity + lifecycle + permission;
ADR-0037/0042 give a surface. This task binds them so "a DSL app" and "a real app in
the OS" are the same thing — search first, then chat, then the hard apps.

---

## STATUS / PROGRESS LEDGER (updated 2026-07-06)

### ✅ BUILT — R1 transport chain, end to end (boot-verification pending)

- **Kernel**: new syscall `VMO_READ` (47) — exact mirror of `VMO_WRITE`
  (`Rights::MAP` derive, span checks, bounded copy OUT of the VMO). Needed
  because userspace has NO VMO mapping path (`nexus_abi::vmo_map` was dead
  code — no kernel handler); the compositor blit reads app pixels through it.
  abi wrapper `nexus_abi::vmo_read`.
- **Wire** (`nexus-display-proto::client_surface`): SURFACE_CREATE(8)/
  PRESENT(9)/DESTROY(10) + ack codecs, `[I,N,ver,op]` envelope (windowd's
  server family; input ops 1–4 collision pinned by test), damage ≤4 rects,
  BGRA8888. 4 unit tests.
- **windowd**: `client_surface.rs` bookkeeping (create validation
  format/bounds/quota, STRICT seq (+1, one in flight), damage clamping;
  6 host tests — 143 windowd tests green) + `compositor/runtime/app_window.rs`
  (5th `ShellWindow`, `WindowId::AppClient`, MAX_WINDOWS=5, pool reserve
  +2×392 rows; create retains the MOVED VMO cap (gpud-attach pattern,
  `mem::forget` on the ReplyCap wrapper), present marks dirty + acks, render
  blits rows `vmo_read`→`vmo_write` under a windowd-drawn title bar; close
  frees atlas per ADR-0037). Server-loop dispatch for the three ops; acks via
  the shared response endpoint (R1 single-app; see ADR deviations).
- **app-host** (`source/services/app-host`, bin-only, NO nexus-service
  metadata so init never spawns it): solid-teal probe — vmo_create/fill,
  `cap_clone`+cap-move CREATE, seq=1 full-damage PRESENT, bounded retries on
  the fixed slots 5/6 (the cap-transfer race, #123 lesson), markers
  `apphost: start` → `APPHOST: surface created` → `APPHOST: probe surface
  presented`.
- **execd**: `IMG_APPHOST=4` ("app.probe"), payload embedded via
  `build.rs` from `EXECD_APPHOST_ELF` (empty ⇒ UNSUPPORTED, fail-closed);
  post-spawn `grant_windowd_route` clones its granted windowd caps into the
  child's slots 5/6 (`cap_transfer_to_slot`). Slot-order contract: execd
  expects its own pair at 8/9 — **verify against `init: execd windowd slots`
  in the boot log** and adjust `APP_WINDOWD_*_SLOT` if the order differs.
- **nexus-init**: execd wiring arm grants SEND on `window_req` + RECV on
  `window_rsp` (cap clones; logged).
- **Trigger**: selftest exec phase requests IMG_APPHOST after the hello
  proof (`SELFTEST: apphost spawn requested|refused`); policyd's EXEC check
  is requester-based (image_id reserved) — no policy change needed.
- **build.sh**: builds app-host FIRST, exports `EXECD_APPHOST_ELF`.
- **ADR-0042 → Accepted** with 5 recorded deviations (app-allocated VMO +
  clone-move; vmo_read syscall; shared-channel acks; full-surface v1 blit;
  envelope family).

### ✅ R1 CHAIN PROVEN END-TO-END (boot 7, 2026-07-06 21:20)

Full marker chain in hop order:
`execd: apphost windowd route granted` → `execd: apphost probe autolaunch
(R1)` → `apphost: start` → `apphost: vmo filled` → `WINDOWD: surface created
id=1 320x240` → `APPHOST: surface created` → `WINDOWD: surface presented
id=1 seq=1` → `APPHOST: probe surface presented`. Cross-process surface
transport works: spawn → own VMO → cap-move CREATE → windowd blit + window →
strictly-sequenced PRESENT → acks back to the app. **User visible-lane
verify pending** (teal 320×240 window, title "App", at 460/420).

### ✅ #102 ROOT-CAUSED AND FIXED (7 boot iterations)

**Root cause**: `spawn_inner` starts EVERY task `Suspended` (the
grants-before-resume hardening); nexus-init resumes its services explicitly
after cap wiring — **execd never resumed its children**, so they loaded but
never executed. Fix: execd calls `task_resume(pid)` after the grants (both
the OP_EXEC_IMAGE path — repairing hello/exit0/minidump children too — and
the R1 autolaunch). The retired selftest exec/crash/minidump chain (#102)
can be restored on this fix.

Iteration traps burned down on the way (recorded for the trap ledger):

- boot 5: child ack-wait budget was ITERATION-counted (4000 yields ≈ 1.17s)
  and expired 3ms before windowd's 1.50s bring-up answer → time-budgeted
  (`nsec()`, 30s).
- boot 6: the chain ran fully but the child's success markers vanished —
  `nexus-service-entry` ARMS verdict folding for every process it
  bootstraps, so non-FAIL `debug_println` lines fold into recall-only.
  Probe markers now bypass via raw `debug_write`.

### ⚠ Historical diagnosis notes (superseded by the fix above)

The R1 chain runs cleanly up to and including the spawn:
`execd: apphost windowd route granted` + `execd: apphost probe autolaunch
(R1)` at 0.33s — but the child NEVER emits `apphost: start` (its first
`debug_println`), with **no fault/KPGF in the kernel log**. This is task
#102 ("execd-spawned children LOAD but no longer execute") reproduced with a
minimal, fully-instrumented payload. Diagnosis notes for the next move:

- kernel `sys_exec` returns a pid; KSELFTEST kernel-side spawns DO execute
  (`KSELFTEST: child entry/exit` in the same boots) and init-lite's `exec_v2`
  services all run — the failure is specific to U-task-initiated `sys_exec`
  children (scheduler enqueue/QoS/resume path, or .bss/stack setup for
  Rust ELFs vs the 409-byte asm payload).
- kernel `EXEC-ELF` trace lines are `log_debug!(target: "exec")` — quiet
  default; a debug-log build shows segment mapping and entry values.
- Boot-lane findings along the way: the selftest exec-phase trigger is
  unusable in BOTH lanes (headless: pre-existing `netstackd: ready` stall
  fails the ota ladder phase; interactive: netstackd isn't in the image and
  the U-mode selftest never reaches phase 5) — hence the execd R1 autolaunch.
- Trap ledger: post-`ready` `emit_line` folds into recall-only (probe
  markers must use `debug_println`); `cap_query` answers ONLY Vmo/DeviceMmio
  kinds (endpoint-slot polling needs a `cap_clone`+`cap_close` probe).

### ✅ R1 ALSO PROVEN HEADLESS + further hardening (2026-07-06 evening)

- The full R1 marker chain reproduces in the **headless proof boot**
  (`build/logs/headless--2026-07-06T21-57-08`, lines 355–475) — "test
  headless" for the transport is done at the uart level.
- app-host idle loop → **blocking recv** (a Normal-QoS `loop{yield}` starves
  every Idle-QoS service on the strict-priority scheduler — netstackd's
  exact exposure class; R3 turns this recv into the input loop).
- netstackd now emits `ready` BEFORE its Idle self-lower (correct marker
  semantics — "process reached entry").

### ✅ R2 PACKAGING HALF DONE (host-proven)

- `manifest.capnp` **v2.1**: `payloadKind @16` (`elf` default | `uiProgram`),
  append-only.
- `nxb-pack`: `payload_kind = "ui-program"` in manifest.toml → packs
  **`payload.nxir`**; found + fixed a PRE-EXISTING gap on the way:
  `rewrite_manifest_with_digests` dropped all v2.0+ fields (bundleType too)
  — scalars now survive; the unused v2.0 LIST fields are still dropped
  (commented, copy when first used).
- Round-trip pinned: `tools/nxb-pack/tests/payload_kind.rs` (2 tests).
- `docs/packaging/nxb.md` payload-kinds section (DoD item).

### ⚠ SEPARATE PRE-EXISTING FINDING: netstackd never reaches its entry

`just test-os headless` fails in the ota phase on a missing
`netstackd: ready` — **pre-existing** (identical fail with this task's work
stashed). netstackd prints NOTHING (not even the service-entry panic
handler); stack-pages 8→32 changed nothing; the binary + entry point look
like every other service. init reports `up netstackd` (exec_v2 + resume ok).
Until this is diagnosed the headless LADDER cannot go green for anyone —
the R1/Phase-6 chain is proven from the uart log instead. Needs its own
diagnosis pass (kernel exec debug build: `log_debug target "exec"`).

### ✅ R1 USER-VERIFIED (teal window visible) + drag fix (2026-07-06 late)

User confirmed the teal window on screen — and reported neither it nor the
DSL demo window can be MOVED. Root cause: the drag-continue block in
`input.rs` only advanced chat/search/settings — **the DSL window was never
draggable** (pre-existing since 0076B: `begin_drag` armed, no `drag_to`, no
`end_drag` on release → stuck), and the app window inherited the gap. Fixed:
both windows drag like Settings (move, no edge-snap) + release termination +
`surface_dirty` re-render after moves.

### ✅ R2a: FIRST CROSS-PROCESS DSL FRAME (boot 9, 2026-07-06 23:39)

The app-host now mounts a real `.nxir` and renders it into its own surface:
`APPHOST: mounted hash=5f1a6f3ab24e3dde` → `APPHOST: dsl frame rendered` →
create/present chain. The hash is IDENTICAL to windowd's in-compositor demo
mount — the same canonical program executing in two different hosts (the
IR-determinism thesis, proven on hardware). Implementation:

- `app-host/build.rs` compiles `examples/dsl/counter/counter.nx` (windowd's
  seam; the bundle GET_PAYLOAD step swaps the BYTE SOURCE, not this code);
  payload embedded 8-byte aligned (capnp), hash-verify off (embedded = trust
  boundary), heap-2m (DSL mount + layout allocate).
- Mount recipe = windowd's demo mount (Runtime::mount for symbols, key
  table, `View::mount` with BaseTokens/FixtureEnv/IdentityLocale); fails
  closed to the visible teal probe fill + FAIL marker.
- Render v1: real layout (`nexus-layout`) at surface size with an ESTIMATE
  text measurer (8px/16px — honest placeholder until the shared text SSOT
  is promoted out of windowd, RFC-0067 P5), fills pass only (page base +
  per-box `visual.background`). Text glyphs are the known gap.

### ✅ TEXT SSOT PROMOTED + REAL GLYPHS IN THE APP FRAME (boot 10, 2026-07-07)

`nexus-text-baked` (userspace/ui/text-baked): windowd's baked-atlas text
pipeline promoted VERBATIM (RFC-0067 P5 discipline) — build-time A8 atlases
(13/16px Inter, ASCII + sparse kerning, fontdue in build.rs only), no_std
measurement + row-based glyph blending, plus the pixel-real
`BakedTextMeasure` (feature `layout`). windowd's `text.rs` is now a thin
re-export (its baking left build.rs/assets.rs; 5 text tests moved with the
code); the app-host renders the counter with REAL Inter glyphs — same
atlases, same blender, same measurement as the compositor. Full chain green
in boot 10; windowd size contract 79%.

### ✅ R3 INPUT PATH BUILT (2026-07-07; boot-regression green, click verify = user lane)

- Wire: `OP_SURFACE_INPUT` (11) — windowd → app, surface-LOCAL body
  coordinates, `INPUT_KIND_TAP` (motion/keys land with the focus model).
- windowd: a body press on the app window forwards the tap over the app's
  response channel (`send_app_input`, slot-4 send, non-blocking — input must
  never stall the compositor) + `WINDOWD: surface input routed` marker;
  windowd keeps focus/raise/drag only. Apps get pixels + events — nothing
  else.
- app-host: the blocking idle recv IS the event loop now — tap →
  `View::pointer` (interpreter hit-testing over the CURRENT LayoutBoxes) →
  visible damage ⇒ re-layout + re-render + strictly-sequenced present
  (`APPHOST: interactive frame presented`). v1 limitation recorded: taps
  arriving during an ack wait are skipped.
- Verification note: a QMP click-injection attempt hit the launcher lane
  (the visible-input autoinjector runs its own script; my grid clicks left
  no windowd echo) — the definitive check is a REAL mouse click on the "+"
  button in the visible lane: the counter value must increment on screen
  (DoD: "click inside the app window dispatches an event visibly").

### ⬜ OPEN

- USER VERIFY: click "+"/"−" in the app window → number changes (R3 DoD),
  text + drag if not yet verified.
- ✅ **LAUNCH PATH BUILT (2026-07-07)** — the real RFC-0065 chain, simpler
  than planned because BOTH sides route at runtime (`route_blocking`/
  `KernelClient::new_for`, no init wiring needed):
  Apps-dropdown click "counter" → windowd `launch_app` sends `OP_LAUNCH`
  (`[A,M,1,1,len,app,len,"main"]`) to abilitymgr → session gate + caps check
  (existing) → broker `launch_with_caps` → `abilitymgr: launch (app=counter,
  inst=…)` (existing event marker) → NEW spawn hop: `spawn_app` maps
  counter→IMG_APPHOST(4) and requests execd (`OP_EXEC_IMAGE`, requester
  "abilitymgr", policyd-verified `proc.spawn`) → `abilitymgr: spawn ok` →
  execd grant_windowd_route + resume (central path) → app window. execd's
  R1 autolaunch DELETED. Registry apps without a spawnable payload
  (chat/search placeholder ELFs) report `launch spawn skipped` by value.
  Boot 13: `registry ok (n=3)` + `caps ok app=counter` — counter is in the
  Apps menu. **USER-VERIFIED 2026-07-07**: Apps→counter opens the app window
  through the full chain. Two launch-path fixes on the way: (a) `new_for` is
  ONE ~100ms routing window — retry + cache the client (inputd lesson);
  (b) ROOT CAUSE: the runtime routing responder answers from init's STATIC
  RouteTable (nothing minted on demand) — both routes had to be provisioned:
  abilitymgr server pair PRE-MINTED (sessiond pattern, so windowd's route
  targets the pair abilitymgr actually serves) + `provision_windowd_ability_
  route` + abilitymgr→execd grant in the declarative arm. Markers:
  `init: windowd route->abilitymgr ok`, `init: abilitymgr route->execd ok`.
  KNOWN COSMETIC ISSUE (next small fix): the app frame renders on a
  near-black page base and the counter buttons (surfaceVariant bg) have too
  little contrast — user cannot SEE the buttons to verify R3 clicks. Fix =
  lighter page base / token-correct surface color in app-host `render`.
  The v1 app→image table (`image_for_app`) is the seam GET_PAYLOAD replaces.
  THEN: bundle GET_PAYLOAD (os-lite opcode; payload → VMO → cap-move to the
  spawned app-host, child slot 7) replaces the embedded `.nxir` + the image
  table; `@persist` via statefsd; stop/crash residency.
- R4 payloadKind dispatch (with 0079). Then 0080B/0080C 
  (DSL shell + greeter, launcher e2e) complete phase 6.
- R2 runtime half — RECON FINDING: os-lite bundlemgrd has NO GET_PAYLOAD
  opcode today (only the std_server speaks it); the R2 payload fetch needs
  either that opcode added os-lite-side (payload bytes → VMO + cap-move
  reply) or execd embedding via the existing image-table seam. Then:
  abilitymgr launch path replaces the R1 autolaunch, app-host validates +
  mounts the `.nxir` and renders the first DSL frame (needs a no_std
  LayoutNode renderer for the surface VMO — the blocking design question).
- R3: input routing by surface id (windowd → focused surface's connection),
  effects over real IPC, `@persist`, stop/crash residency, per-app channels.
- 0080B/0080C (shell + greeter in DSL, launcher e2e) — after R2/R3.
- Restore the retired selftest exec/exit0/minidump chain on the #102 fix.
- netstackd entry diagnosis (separate finding above).
- R2: manifest v2.1 `payloadKind`, nxb-pack `.nxir`, bundlemgrd GET_PAYLOAD
  sourcing, validate/mount, first DSL app frame (counter).
- R3: input routing by surface id, effects over real IPC, `@persist`,
  stop/crash residency, per-app ack channels.
- R4: AOT payloadKind dispatch (0079).

### 2026-07-07 (autonomous phase-6 completion batch, uncommitted)

- **Contrast fix DONE**: app-host `render` page base is now the theme's
  Surface token (`BaseTokens.color(ColorToken::Surface)` = #f8f9fa) instead
  of the hardcoded near-black — the counter's surfaceVariant buttons and
  onSurface text are specified against exactly this base. USER-VERIFY: "+"
  click increments visibly.
- **GET_PAYLOAD CHAIN BUILT** (replaces the embedded `.nxir` byte source +
  the hand-maintained image table):
  - Wire: `nexus_abi::bundlemgrd::OP_GET_PAYLOAD(6)` — request
    `[B,N,1,op,id_len,id]` with the payload VMO cap MOVED alongside
    (ADR-0042 SURFACE_CREATE pattern; the message's single cap slot carries
    the VMO, so there is NO reply frame). bundlemgrd writes payload bytes at
    offset 16, then the 16-byte header (`NXPL`, status, len) LAST —
    header-last release ordering; consumers poll the header. Codecs + tests
    in nexus-abi (`encode/decode_get_payload`, `encode/decode_payload_header`).
  - bundlemgrd: build.rs compiles every `payload_kind = "ui-program"` bundle
    (source convention `examples/dsl/<name>/<name>.nx` until the TASK-0081
    consolidation) into `APP_PAYLOADS`; os-lite serves OP_GET_PAYLOAD
    (moved-cap take via ReplyCap::slot + mem::forget — the windowd CREATE
    pattern), markers `bundlemgrd: payload served` / `FAIL get_payload`.
    execd added to the sender allowlist.
  - init wiring: execd arm gets a CLONE of bundlemgrd's request SEND at
    slot 10 (slot-order contract; marker `init: execd bundle slot send=0xa`).
  - abilitymgr: `image_for_app` is now MANIFEST-DRIVEN (`APP_UI_PROGRAMS`
    generated from `payload_kind = "ui-program"`); the exec frame carries an
    append-only app-id extension (`…requester, app_len, app`).
  - execd: parses the app id; for IMG_APPHOST creates the payload VMO
    (16 + 256KB budget), fires GET_PAYLOAD (bounded non-blocking send,
    fire-and-forget — the CHILD polls), transfers the ORIGINAL cap to child
    slot 7 (`Rights::MAP`) after the windowd grants, before resume. Markers
    `execd: app payload requested/granted` + FAIL variants; VMO closed on
    every failure path.
  - app-host: `resolve_payload()` — slot-7 presence probe (cap_clone+close),
    header poll (3s budget; the fetch starts before our ELF even loads),
    `AlignedBytes` read (NEW in nexus-dsl-ir::read — u64-backed 8-aligned
    owned bytes, the runtime counterpart of the repr(align(8)) embed),
    leaked once per process. Marker `APPHOST: payload source=bundle` (or
    `source=embedded (<reason>)` — the embed stays as the fail-closed,
    visibly-marked fallback). Determinism cross-check: the mounted hash
    must STAY `5f1a6f3ab24e3dde` (same canonical compile in bundlemgrd).
- **@persist RUNTIME CORE DONE** (host-proven): `nexus-dsl-runtime::persist`
  — NAME-keyed `NXPS` v1 snapshots of `@persist` store fields
  (`Runtime::persist_snapshot/persist_restore/has_persist_fields`);
  per-entry tolerant restore (missing field / no-longer-persist / type
  change ⇒ skip, never poisoned state); records persist field names by
  STRING (symbol ids are compile-run-specific). 3 unit + 2 conformance
  tests (round-trip across mounts incl. non-persist field isolation;
  type-change skip). **OS wiring deliberately deferred**: no shipped app
  declares `nexus.permission.STATE` or `@persist` yet — granting statefsd
  routes to app children today would be a dead cap leak (fail-closed rule).
  Lands with 0080C step 4 (queryd/statefsd wiring) or the first persisting
  app.
- **Stop/crash residency + reaper: OPEN** (documented, not faked): nobody
  waits on app-host children today (OP_WAIT_PID is a synchronous
  selftest-lane op); the async exit→abilitymgr-transition→window-close
  protocol is boot-iterated work, queued behind the 0080C mount steps.
- Boot-verified (manual--2026-07-07T10-05-27 + final re-boot): 0 FAILs,
  launch routes + registry + caps + DSL chain green. The GET_PAYLOAD
  runtime markers appear on the USER's Apps→counter click (see
  `tools/postflight-systemui-bootstrap-shell.sh`, "payload chain" stage).

### 2026-07-07 (R3 input ROOT CAUSE + fix, uncommitted)

- **User-Verify des Launch-Batches**: Fenster erscheint, Kontrast ok,
  GET_PAYLOAD-Kette komplett auf dem User-Boot bewiesen
  (`APPHOST: payload source=bundle`, hash unverändert) — aber BUTTONS TOT.
- **ROOT CAUSE (aus dem Code, kein Boot-Loop)**: OP_SURFACE_INPUT lief über
  den GETEILTEN `window_rsp`-Kanal — auf dem hält AUCH inputd ein RECV und
  drainiert ihn permanent (Ack-Lesen nach jedem Visible-State-Push): Taps
  wurden von inputd KONSUMIERT, bevor der app-host sie sah. Der Marker
  `WINDOWD: surface input routed` war zudem HOHL (druckte vor bekannter
  Zustellung — die vom User geforderte Fehlermeldungs-Klasse).
- **FIX = ADR-0042 Deviation 6, dedizierter Per-App-Event-Kanal**:
  init mintet das Pair im execd-Arm (`init: execd app-event slots send=0xb
  recv=0xc`, Slot-Order-Kontrakt); execd bewegt den SEND-Clone per
  `OP_SURFACE_EVENTS`(12)+CAP_MOVE an windowd (VOR dem resume, gleiche
  Request-Queue wie das spätere SURFACE_CREATE ⇒ Attach-vor-Create
  garantiert) und grantet RECV an Kind-Slot 8; windowd liefert Input UND
  Acks auf dem Kanal (Fallback shared nur noch markiert für alte Wiring);
  app-host recv't blocking auf Slot 8 (`APPHOST: events
  source=dedicated`).
- **Observability-Härtung** (Konsequenz aus dem User-Feedback): `routed`
  nur noch bei zugestelltem Send; `WINDOWD: FAIL surface input (no event
  channel)/send`; app-host bounded-Marker für recv-Fehler, Nicht-Input-
  Frames und Tap-Misses (Koordinaten-Bugs sähen exakt so aus). Postflight
  hat eine neue Stufe „app event channel (dedicated)".
- Proto-Op + Codec getestet (nexus-display-proto 9 Tests inkl.
  Kollisions-Pin); windowd 138+ Tests grün, Size-Contract 79%.
- USER-VERIFY: Apps→counter → „+" klicken → Zahl steigt; Postflight zeigt
  die Event-Kanal-Stufe + `APPHOST: interactive frame presented`.

### 2026-07-07 (Runde 2: recv-Wake-Befund + Demo-Retirement, uncommitted)

- User-Test nach dem Event-Kanal-Fix: Kette VOLLSTÄNDIG grün (sent →
  granted → attached → source=dedicated → routed×N, `routed` druckt nur
  noch bei zugestellter Nachricht) — und der app-host blieb TROTZDEM stumm
  (kein tap-miss, kein event-frame-skipped, kein recv-FAIL). Einziger
  offener Hop: das BLOCKING recv selbst.
- **KERNEL-BEFUND (#102-Familie): ein exec'd-Kind, das in `Wait::Blocking`
  recv parkt, wird von einem Sender NIE geweckt.** Die Acks kamen nur an,
  weil `recv_ack` NonBlocking pollt. Diagnose-Anker: Boot
  manual--2026-07-07T12-12-27 (zugestellte Frames, Empfänger für immer
  still). Kernel-Fix (Sender-Wake für U-Task-Kinder) = eigener Zug.
- **WORKAROUND im app-host**: Event-Loop auf `Wait::Timeout(30ms)` —
  der Timer-Deadline-Wake (`wake_expired_ipc_deadlines`) ist bewiesen
  generisch; timed park wacht spätestens am Deadline und pollt neu
  (bounded Latenz, kein Normal-QoS-yield-spin). Neuer Marker
  `APPHOST: event loop armed` schließt die letzte stumme Lücke
  (Loop-Eintritt ist jetzt beweisbar).
- **DSL-Demo-Fenster RETIRED (User-Entscheidung: nur EIN Counter)**:
  `maybe_boot_open_dsl` mountet weiterhin (Marker `DSL: program loaded
  hash=` bleibt der Headless-Beweis des In-Compositor-Pfads, den 0080C
  wiederverwendet; neuer Marker `DSL: demo window retired (mount-only)`),
  öffnet aber KEIN Fenster mehr; `DSL: first frame presented` entfällt —
  Postflight-Stufe umgestellt; tools/nx-Chain-Tests sind Host-Simulationen
  (unberührt). `open_dsl_demo` bleibt als 0080C-Fenster-Pfad (allow(dead_code)).

### 2026-07-07 spätabends (Closure-Plan P0.2, uncommitted): recv-wake-Gate + app-host auf Wait::Blocking

- **Regressionsgate `recv-wake-probe`** (neues no_std-Kind unter
  source/services/recv-wake-probe, EXECD_RECVWAKE_ELF-Embed wie app-host):
  execd spawnt es EINMAL nach ready — armed→BLOCKING-recv-Park→execd-Ping→
  woke-Reply (zwei init-geminte one-way Endpoints, execd-Slots 13–16,
  Kind-Slots 5/6, grants-before-resume, 30ms-Park-Fenster, jeder Hop
  fail-loud). Boot-Verdict `SELFTEST: exec child blocking recv wake ok` +
  Postflight-Stufe. BEFUND: der Sender-Wake eines geparkten exec'd-Kindes
  funktioniert mit dem aktuellen Kernel — die 12-12-27-These reproduziert
  sich nicht mehr; das Gate hält die Klasse ab jetzt jeden Boot unter Beweis.
- **app-host Event-Loop: Timeout(30ms)-ÜBERGANG ENTFERNT → Wait::Blocking**
  (reaktiv, null Polls) auf der Basis des grünen Gates. USER-VERIFY offen:
  Apps→counter, „+"-Klick → Zahl steigt (jetzt über den blocking-Pfad).
- **Kernel fail-loud**: observe_wake_outcome druckt one-shot
  `KERNEL: FAIL ipc wake (task-not-found|enqueue-rejected)` — ein gepoppter
  Waiter, dessen Wake scheitert, kann nie mehr still verloren gehen.
- **Neue Falle (#123-Klasse, dokumentiert)**: execd resumed BEVOR inits
  Wiring-Arm die Slots transferiert (Probe fand Slots 4..24 leer bei
  0.124s) — jeder Post-ready-Cap-Zugriff in execd braucht den bounded
  cap_clone-Poll (Muster im Probe-Runner).
- Beweise: Boot 20-06-16 — 0 FAIL/PANIC/KPGF, Postflight alle Stufen OK
  (inkl. recv-wake-Gate + display truth); execd/init/kernel/app-host/probe
  riscv-Checks grün; Kernel-Host-Tests 16 grün.

### 2026-07-07 nachts: +/- reagieren nur einmal — ROOT-CAUSED + GEFIXT (uncommitted)

User-Report: Counter-Buttons reagieren nur beim ERSTEN Klick. Automatisiert
reproduziert (QMP-Klicks auf „+" @ 526,520): Tap 1 → Frame, Taps 2-5 → tot.

**Fehldiagnose zuerst (verworfen):** „wake-then-lost" im Kernel. Widerlegt
durch die auf ZWEI Zyklen erweiterte recv-wake-Probe (grün) — der Kernel-
Sender-Wake für geparkte exec'd-Kinder funktioniert wiederholt einwandfrei.

**ECHTE URSACHE (bewiesen durch schrittweise Instrumentierung):** windowd
hatte den Server-Protokoll-Dispatch DOPPELT — im Drain-Batch (vollständig)
UND im Idle-Blocking-Recv (nur OP_GET/UPDATE_VISIBLE_STATE, alles andere →
UNSUPPORTED). Sobald der Desktop idle ist (keine Animation, kein pending
damage — genau nach App-Öffnen + Tap), blockiert windowd im zweiten Recv.
Das App-Present (OP_SURFACE_PRESENT) landet dort → UNSUPPORTED → VERWORFEN
(kein Ack). Das Probe-Present (seq=1) klappte nur, weil windowd beim Boot
noch beschäftigt war und den ersten (vollständigen) Recv-Pfad nutzte.
Beweis-Kette: `APPHOST: present send slot=5 seq=2 ok=true` aber windowd
empfängt op=9 nie ausserhalb des Boots.

**FIX (production-grade, keine Doppelstruktur):** `dispatch_client_frame()`
als EINE SSOT für das gesamte Server-Protokoll (compositor/mod.rs); beide
Recv-Stellen rufen sie — eine fehlende Branch ist damit strukturell
unmöglich. `nexus_ipc::ReplyCap` dafür public exportiert.

**ZWEITER FIX (app-host Event-Loop):** war `tap→render→present→recv_ack`,
wobei recv_ack denselben Event-Kanal blockierend drainierte und interleavte
Input-Frames VERWARF („keep waiting"). Jetzt EIN vereinheitlichter Blocking-
Recv: jeder Tap wird SOFORT aufs Modell angewandt (Counter zählt immer),
Present via `present_in_flight`-Flag entkoppelt (max. 1 in flight, Coalescing
per `dirty`), Present-Ack ist reine Flusskontrolle — nie wieder ein Tap
verworfen.

**Beweis:** 5×„+" → 5 interactive frames (vorher 1), Counter sichtbar
inkrementiert; danach 3×„−" → Counter zeigt **2** (visual-postflight grün,
Screenshot). Host-Suiten: windowd 138+2+9, kernel 16, nexus-ipc 31 grün;
riscv-Checks kernel/windowd/app-host 0 Fehler. Diagnostik wieder entfernt;
der ehrliche Reject-Marker `WINDOWD: FAIL surface present rejected` bleibt.
