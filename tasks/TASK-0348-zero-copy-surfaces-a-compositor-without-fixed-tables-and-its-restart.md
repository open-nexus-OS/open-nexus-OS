---
title: TASK-0348 Zero-copy surfaces, a compositor without fixed tables, and its restart
status: In Progress (2026-11-02 — P1–P5 built and QEMU-proven; P6 milestone 1 (the restart and the rejoin, without ghosts) built and proven on the many-windows lane; test-all + board cycle at this checkpoint; P6 milestones 2–3 (ghosts, the board context's rebinding) next)
owner: @ui @runtime @kernel-team
created: 2026-10-31
depends-on: [TASK-0347]
follow-up-tasks: [TASK-0349 the memory model (an owner's footprint includes its surfaces, jetsam, the large-memory grant), TASK-0350 layer 8 (direct scanout via usage=SCANOUT, apps rendering with the GPU into their slots, the passes a frame runs)]
links:
  - RFC: docs/rfcs/RFC-0114-surfaces-are-shared-buffers-the-app-draws-where-the-gpu-reads-the-compositor-composes-by-reference.md
  - RFC: docs/rfcs/RFC-0112-a-services-death-is-a-re-resolve-for-its-clients-the-compositors-restart.md (amended, Phase 6)
  - ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md
  - Evidence: board cycle 79 (build/logs/board--2026-10-30T18-10-02): `copy_rows=`/`copy_us=` up to 4008 rows / 138 ms per pass
  - Queued by: the operator's three-package plan (2026-10-30): package 1 of 3
---

# TASK-0348 — Zero-copy surfaces, a compositor without fixed tables, and its restart

## The operator's ask (2026-10-30)

"wie vorgeschlagen paket 1, 2 und schicht 8 alles production grade in endgame manier" — the
OS as smooth on the BPI-F3 as a current MacBook, layer 6's memory work actually used, no app
limited by a hard-coded slot or size, future-proof for heavy desktop apps (a browser), GPU memory
inside the one shared-memory model. Package 1: windows as shared buffers (IOSurface / sysmem),
windowd's tables dynamic, windowd's restart.

## Architecture review (three lenses, 2026-10-31)

- **Scope** — in: kernel `vmo_clean` (74) + clean zeroed non-coherent device frames + REPLACE_DEAD
  transfers; gpud surface objects (board, virgl, CPU) + allocation protocol (18/19) + adopt;
  nexus-gfx by-reference layer (`source`, slot, per-source generation, glass key without slot);
  nexus-display-proto surface v2 + CLOSED/BACK; windowd by-reference composition, release rule,
  dynamic tables + window keys; app-host mapped painting, copy-forward, clean, re-entrant bring-up;
  init/execd re-pin; abilitymgr singleton launch. Out: jetsam/limits (0349), direct scanout and
  GPU-rendered apps (0350), gpud's own restart, triple buffering for CPU apps (measured, not built).
- **Invariant** — an app reaches only its own surface (owner = kernel sender id; gpud serves
  windowd only; no device cap to apps; `vmo_clean` writable objects only); a slot is reused only
  when no in-flight frame reads it; a live capability is never overwritten (REPLACE_DEAD) —
  `test_reject_vmo_clean_read_only_alias`, `test_reject_vmo_clean_past_end`,
  `test_reject_surface_alloc_from_stranger`, `test_reject_present_slot_out_of_range`,
  `test_reject_replace_dead_over_live_cap`, `test_reject_window_key_collision`.
- **Contract** — RFC-0114 (new), RFC-0112 (amended), ADR-0092 (supersedes ADR-0042 for app
  surfaces), ADR-0079's deferred maintenance call (built here).

## Phases (stop condition · proof)

- **P0 — measured.** Board cycle 79: windowd's copy 20–138 ms per pass, GPU wait 17–31 ms; a
  fifth app refused (`WINDOWD: surface create FAIL (window slots exhausted)`), a ninth channel
  evicts a live app. · Lane log of the sixth-app refusal on visible.
- **P1 — kernel.** `vmo_clean` (phased; no-op unless the object is for a non-coherent device);
  zeroed frames of such objects cleaned at commit and create. · host tests +
  `KSELFTEST: vmo clean ok (`.
- **P2 — gpud + nexus-gfx.** Surface objects per backend (board: lazy, bound whole at
  allocation, GPU-VA allocator, dynamic BO/binding tables; virgl: eager, one resource, per-present
  transfer of damaged rects of the presented slot; CPU: eager, mapped once); `OP_SURFACE_ALLOC`/
  `FREE`, charge `Part::Surfaces` to the owner; `CompositeLayer.source`, slot in flags, per-source
  generation, glass fold without the slot, descriptor with base+stride; `retired` in the present
  ack. · nexus-gfx host tests; `gpud: surface alloc ok (`.
- **P3 — windowd by reference.** No `copy_surface_rows` for app surfaces; atlas only for
  windowd's own patches; the release rule (host state-machine test); per-surface generation
  replaces the global epoch for app layers. · `windowd: compose copies=0` on headless + visible.
- **P4 — app-host.** Surface v2 create (the object cap in the ack), `vm_map`, paint in place,
  copy-forward of the last present's rects, `vmo_clean`, present v2 with the slot; resize reuses
  a fitting object. · lanes green, typing + scroll scenarios unchanged, `apphost: surface mapped (`.
- **P5 — dynamic tables.** Window keys; every `[_; MAX_APP_WINDOWS]`/`[_; 8]` table growable;
  feed and capture frames for N windows; planner transforms by key. · twelve apps open on visible.
- **P6 — restart (RFC-0112 amended).** REPLACE_DEAD; init's windowd arm (one loop over
  `SERVICE_SPECS`); execd re-pins children and sends `OP_COMPOSITOR_BACK`; `OP_SURFACE_CLOSED`;
  app-host re-entrant bring-up; singleton launch; gpud keeps frame + objects, `OP_SURFACE_ADOPT`,
  ghosts. · `windowd: restarted (ghosts=N)` on the visible lane after a QMP kill.

## Progress (2026-11-02)

- **P1 ✅** — `vmo_clean` (74), zeroed device frames cleaned: `KSELFTEST: vmo clean ok (`.
- **P2 ✅** — gpud surface objects on 2D, virgl and the board backend (`surfaces.rs`,
  `surface_serve.rs`, `virgl_surfaces.rs`, `backend/gpu/surface_buffers.rs`); the by-reference
  layer in nexus-gfx (`source`, slot in `flags`, per-layer upload on a source/slot/generation
  change, the glass key without the slot): `gpud: surface alloc ok (`.
- **P3 ✅** — windowd composes by reference: `client_surface.rs` (the table grows; buffer + slot
  ledger; `surface_release_*`), `compositor/runtime/surface_buffers.rs` (async ALLOC through the
  one capability-aware gpud receive, the answer's capability in order through the client outbox,
  FREE, the release on every gpud ack), the copy path and the app atlas bands deleted, a
  replaced surface shown with its layers until its successor presents; the loop line names
  `work(released= held_us_max=)` instead of `copy_rows=`: `windowd: compose copies=0 (`.
- **P4 ✅** — app-host: no VMO of its own; `OP_SURFACE_BUFFERS` answer mapped
  (`nexus_abi::SharedVmo`), rows painted in place (no syscall per row), copy-forward of the last
  present's rects once per slot (`slot_math.rs`, host-tested), clean for a non-coherent GPU,
  present v2 with the slot; glass regions follow the node's animation (the greeter's entry):
  `apphost: surface mapped (`.
- Found on the way: the reveal frame was built before the present's structure took effect (an
  old ordering bug the old copy path hid); a present racing a replaced surface is stale, not a
  FAIL.
- Lanes: `just test-os` (headless 2D), `just test-os visible` (virgl, pixel proof), `just
  ci-os-usb-visible` (35/35 chain markers; the `windowd: ux input FAIL` lines of the last run
  gone). Gates: `just diag`, `just check`, host tests of display-proto, windowd, app-host, gpud,
  nexus-gfx, nexus-abi.
- **P5 ✅** — windowd's slots grow (a free one reused, a new one registered in the z-stack) and
  every window gets a fresh key (`window_keys.rs`: climbing through `1..=0xFFFE`, skipping live
  keys, bounded — `test_reject_window_key_collision`); a new window takes its slot's place in a
  cascade computed from the room its frame leaves (`cascade_origin`: the old fixed-origin cascade
  slid every large window past the second into one corner); the client outbox pool grows (a
  ninth client behind got no outbox); gpud's board buffer table grows (it stopped at 64);
  `windowd: windows drawn (n=N)` names each new high of app windows one frame drew from their
  own buffers. The new **many-windows lane** (`just ci-os-many-windows`: usb-visible's desktop
  on a 1 GiB guest) presses the launcher's settings button twelve times — every press a new
  instance — and waits for one frame with all twelve: `windowd: windows drawn (n=12)`, the
  frame's layer bound shared out fairly (`layers over the frame bound (dropped=2)`, bodies
  first).
- Found on the way (P5): **settingsd stranded the last answer of a batch** — its loop answered
  request N together with the blocking receive of N+1 (one trap), and after its 32-request batch
  it went to its waitset with the last answer parked: the caller hung until someone else called
  (the eighth settings window waited forever for its second read; the dirty settings blob was
  never persisted while the chain lasted). settingsd now answers at once and never blocks on a
  client; `serve_next` honours a non-blocking wait (answer, then receive); `reply_and_close`
  closes the reply capability even when the answer cannot be delivered (it leaked the server's
  slot). `composite_glass` reported the blur cache's rebuild, not the window drawn — the count
  needed the latter.

- **P6 milestone 1 ✅ (QEMU)** — the restart and the rejoin, without ghosts (RFC-0112
  amendment 2026-11-02 (b)). Kernel: transfers into the caller's own child only, REPLACE_DEAD,
  the in-flight capabilities of a dying endpoint judged like a close (`cap_rules`, host + two
  `KSELFTEST` rungs). init: windowd's legs pinned by one path at boot and at a respawn
  (`respawn_plan`, host-tested over `SERVICE_SPECS`), the five client legs re-pinned,
  `OP_COMPOSITOR_BACK` to execd. execd: every live app's windowd slots re-pinned and the app told
  on its own channel with a fresh SEND — a full channel makes the notice owed, armed writable.
  windowd: `OP_SURFACE_CLOSED` before a close on purpose; surface ids and present seqs seeded
  past the predecessor's; start-up waits matched against strays; present/destroy refused for a
  surface the sender does not own; the restart chord (code 16 in the shortcut registry);
  `OP_COMPOSITOR_UP` to abilitymgr at every start. app-host: the rejoin state machine
  (`probe/reconnect.rs`). abilitymgr: the frozen thawed at a compositor's start, one-instance
  roles by `bundle_type`. gpud: a reveal per instance (after its own `OP_REVEAL`), the old
  compositor's surfaces and layer set released at the new one's framebuffer request.
  settingsd/sessiond: a dead watcher goes at once. The many-windows lane restarts the compositor
  over its twelve windows: `execd: compositor back (apps=13 owed=0 failed=0)`, `abilitymgr:
  launch kept (app=desktop-shell`, thirteen `APPHOST: compositor back (reattached)`, and
  `windowd: windows drawn (n=12)` again — checked in order by `scripts/qemu-test.sh`, the pixel
  proof taken after the restart (91.6 % non-black).
- Found on the way (P6): gpud acked a restarted compositor's FIRST present as its reveal (the
  display's latch outlived the instance that set it) — the new instance's reveal never
  completed; a settings registration's burst (nine keys) overran its depth-8 push channel when
  the subscriber was not yet draining (a restarted compositor) — the channel now holds a whole
  burst (`WATCH_CHANNEL_DEPTH`, a test holds every key to it); a key sent while no compositor
  ran was a `FAIL key send` and the visible-state push a `chain I5 deliver FAIL` — both name the
  outage now; a chord that failed to push would have been replayed to the next compositor (the
  restart chord would restart it again); inputd's route ask could park on an init busy with the
  respawn — the declared slots alone now; the window-zones test did not know the restart code.
  The lane's first end condition (2 s after the twelfth window) stopped QEMU before the shell —
  the heaviest surface, often the last to present — drew its first frame: the snapshot showed the
  windows without the shell and looked like a broken restart. The end now waits for the twelve
  AND the shell's reveal (a group step in the launcher's chain, the pixel proof's too), and
  `scripts/qemu-test.sh` requires the reveal after the restart. The present-ownership check knew
  one desktop owner: the greeter's last presents into its replaced surface were refused as a
  stranger's — the replaced surface keeps its owner now, and a refused create changes no owner;
  a late present into a surface that is gone altogether (the greeter after two shell re-creates)
  is `stale (gone)`, a present into another's live surface stays `FAIL … denied`.
- **Board cycle 80 (2026-11-02, `build/logs/board--2026-11-02T14-16-01`)** — the package on the
  board's GPU path: `KSELFTEST: cap transfer rules ok (`, `in-flight caps released ok`,
  `ambient authority reject ok (`, `vmo clean ok (`; 22 × `gpud: surface alloc ok (… lazy=1)`,
  `apphost: surface mapped (`, `windowd: compose copies=0 (`, `windowd: windows drawn (n=4)`;
  the cycle-79 failures `WINDOWD: surface create FAIL`, `apphost: FAIL surface ack status` and
  the settings watch's `QueueFull` are gone. Ten operator acks (desktop, typed, pointer, modal,
  clipboard, screenshot, gpu-desktop, lifecycle via the minimize button, caret, shortcuts).
  `board-test` [FAIL]: (1) `SELFTEST: ime v2 cjk jp FAIL` and `… candidates FAIL` — the probes
  type composition text through imed's OSK endpoint, and a real pointer event that arrived after
  the greeter's surface existed made windowd tell imed the window focus without a text field
  (`FIELD_KIND_NONE`), under which imed drops composition text by design (TASK-0074 D3); this
  build brings the greeter up earlier (its surface at line 1114, cycle 79's at 1140), the
  2026-10-28 cycle passed with the first input BEFORE the greeter's surface — a latent order
  dependency of the probe on real input, not an engine defect; (2) no `windowd: wm chord (` line
  at all: Super+Ctrl+M/W (never seen on the board, cycle 79 had arrows only) and the restart chord
  did not reach windowd — `tile` and `restore` not acked. Both are the first items after this
  checkpoint.
- Next (P6 milestones 2–3): ghosts — gpud keeps the old compositor's objects and the last
  frame's layers that sample them, `OP_SURFACE_ADOPT` (20) hands them to the new windowd, which
  shows each as its owner's ghost until the app presents again (`windowd: restarted (ghosts=N)`);
  on the board the replaced GPU context re-binds the adopted objects.

## Gates

- Host: `cargo test -p neuron`, `-p nexus-gfx`, `-p nexus-display-proto`, `-p windowd`,
  `-p app-host` (host slices), `-p gpud` (host).
- QEMU: `just test-os` (CPU path), `just test-os visible` (virgl), `just ci-os-usb-visible`.
- Board: one cycle at the end (frame times, no copy line, twelve windows, the restart from the
  console); `just test-all` before it.

## Not doing here

Per-app memory limits and jetsam (TASK-0349); direct scanout and GPU-rendered apps (TASK-0350 —
the allocation's `usage` and the object's bindability leave room); gpud's restart; a generic
sysmem constraint solver. Process-level tables that bound the number of apps — execd's 32
tracked children, abilitymgr's 32 instances, memd's 32 banded processes and its 8 GPU charge
clients (a ninth app's surfaces go uncharged) — belong to the one memory model (TASK-0349). The
frame's 64-layer bound (a frame draws the topmost 62 windows; an occluded one still spends its
body) and a board GPU context's 128 bindings belong to the GPU path (TASK-0350: occlusion
culling, growable bindings).
