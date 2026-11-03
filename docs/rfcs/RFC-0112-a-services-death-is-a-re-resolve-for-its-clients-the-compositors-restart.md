---
schema: 1
id: RFC-0112
title: "A service's death is a re-resolve for its clients — the compositor's restart brings the desktop back with every app's band as a ghost until its next present"
status: in_progress
status_note: "seeded 2026-10-27 from TASK-0333 6d-4's assessment; amended 2026-10-31 from the code map of TASK-0348"
owners: ["@runtime", "@reliability", "@ui"]
created: 2026-10-27
updated: 2026-11-02
subsystem: ui
tasks: [TASK-0348, TASK-0333]
links:
  - label: Tasks
    target: "`tasks/TASK-0348-zero-copy-surfaces-a-compositor-without-fixed-tables-and-its-restart.md` (Phase 6), `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6d-4, 6g)"
  - label: Link
    target: "RFC-0114 (surfaces are shared buffers — why the screen can survive with the apps' own pixels)"
  - label: ADRs
    target: "`docs/adr/0057-service-restart-capability-re-resolve.md` (the pinched pilot: re-mint the owner-bound request endpoint, re-distribute, STALE on the route table), `docs/adr/0073-…` (death is an event), `docs/adr/0075-…` (the compositor never waits on a client), `docs/adr/0083-…` (the lifecycle contract the restart must keep)"
  - label: RFCs
    target: "RFC-0079 (EOF: a client learns of a server's death on its receive), RFC-0086 (the window feed), RFC-0111 (the lifecycle)"
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0112: A service's death is a re-resolve for its clients — the compositor's restart brings the desktop back with every app's band as a ghost until its next present

> Status (migrated 2026-11-02): In Progress (seeded 2026-10-27 from TASK-0333 6d-4's assessment; amended 2026-10-31 from the code map of TASK-0348 — built as TASK-0348's Phase 6, after zero-copy surfaces; Phases 1–2 built and QEMU-proven 2026-11-02, Phase 3 built and proven on QEMU and the board 2026-11-03 — amendment (c))

## Status at a Glance

- **Phase 0 (measure what dies with windowd today)**: ✅ (2026-11-02, the wiring inventory —
  amendment below; the code survey replaced the kill run)
- **Phase 1 (init's respawn arm for windowd: re-mint, re-pin the fleet, STALE)**: ✅ (2026-11-02,
  QEMU: the many-windows lane's restart; board cycle at the TASK-0348 checkpoint)
- **Phase 2 (every client reconnects on EOF: shell, app-host, imed, inputd, screencapd, execd's
  children)**: ✅ (2026-11-02, QEMU: twelve windows and the shell rejoin — amendment (b))
- **Phase 3 (the screen survives: gpud keeps the last frame, the bands come back as ghosts)**: ✅
  (2026-11-03, QEMU many-windows: five restarts, `windowd: restarted (ghosts=12)`, every ghost
  taken over by its app, the wait ring ended by the kept answer; board: three chord restarts,
  five objects adopted and composed by reference in the new context's first frame — amendment (c))

Definition: "Complete" = `windowd: restarted (ghosts=N)` on the many-windows lane after the
restart chord (with every app window present again from its app), and on the board the desktop
returns with every app window drawn from its last frame until the app presents again.

## Scope boundaries (anti-drift)

- **This RFC owns**: the fleet re-route protocol (which slots of which services init re-pins
  after a declared server's death, and how a client learns to re-resolve), the client-side
  reconnect contract for windowd's clients, the compositor's restart state (ghost bands).
- **This RFC does NOT own**: app state restore (`restorable` / `@persist`, 6e), the respawn
  decision itself (ADR-0057's supervision engine), gpud's own restart.

## Context

Today only `pinched` is respawnable (`respawnable()` in init's `respawn.rs`): its request
endpoint died with it (endpoints are owner-bound), init re-mints one from boot-held state and
re-distributes it to its ONE client. windowd's supervision row says `CriticalSession,
OnFailure`, but no arm exists: when windowd dies the shell, every app window, the keyboard's
relay and the sightings of ADR-0083 are gone until reboot. The phones this OS is measured
against survive this: on iOS the system UI's death ("respring") restarts the UI session and
apps come back from their saved state; on OpenHarmony the window manager's death is a
death-notification to every client, which re-registers its windows. Both treat the compositor's
death as a fleet event, not a single respawn.

What makes windowd different from pinched (the 6d-4 assessment):

1. Its request endpoint is held by the whole fleet: the shell and every app (granted by execd
   into the app-host's fixed SDK slot), imed, inputd (the priority-wired input request), screencapd,
   abilitymgr (sightings), execd. A respawn must re-pin a fresh SEND into EVERY declared windowd
   slot (declared_slots knows them) and execd must re-grant its live children.
2. Every push channel windowd minted dies with it: the app event channels, the shell's window
   feed, the settings watches. Each client sees EOF (RFC-0079) and must re-route, re-create its
   surface and present again — an app-host reconnect that does not exist today.
3. The frame on screen is windowd's: gpud scans out windowd's planes. For the desktop to "come
   back" instead of going black for the restart, gpud must keep the last composed frame until
   the new windowd presents, and the new windowd must draw the bands it does not yet own as
   ghosts from a frame it never composed — the ghost band of ADR-0073, now for every window.

## Goals

- windowd's death is survived without a reboot: the shell returns, input works, apps keep their
  processes (frozen or not) and re-present into their old bands.
- No new authority: the respawn derives everything from boot-held state (ADR-0057), the
  grants from policyd (ADR-0084), the lifecycle from abilitymgr.

## Non-Goals

- Restoring app UI state across the compositor's death (the app keeps running; its surface is
  re-created from its own live state).
- Surviving gpud's death (its own RFC).

## Amendment 2026-10-31 — what the code says (TASK-0348's map)

The seed's picture was wrong in four places; the design below follows the code:

1. **App event channels and the shell's window feed are not windowd's**: init mints each app's
   channel for execd (owner = execd), execd grants RECV to the app and the app hands windowd a
   SEND. They survive windowd; only windowd's SEND dies, and the app sees the last-sender EOF.
   What dies with windowd is what it owns: its request endpoint (depth 32), its shared response
   endpoint (8), its settings/session watch, timer and reply endpoints.
2. **Holders of windowd's endpoints**: inputd (5,6), imed (6,7), execd (8,9) and every app child
   (5,6, cloned from execd's), screencapd (7), memd (11); sessiond and settingsd hold moved SENDs
   of the two watches. abilitymgr holds none (sightings go windowd → abilitymgr).
3. **A dead capability blocks its slot**: init's pins are `CopyToSlot` (`set_if_empty`); every
   client's dead SEND still sits in its declared slot, so "re-pin into every declared slot" fails
   as is (`init: FAIL declared slot route send`).
4. **A window closed on purpose and a dead compositor look the same** to an app (both are the
   EOF on its channel), and a restarted windowd would launch the shell or greeter a second time
   (abilitymgr launches a new instance on every LAUNCH).

And one thing changed for the better: with RFC-0114 every app surface is a gpud object the app
draws into — **gpud holds the pixels, not windowd**. The screen can survive with every window's
own last pixels, not a blank band.

## Amendment 2026-11-02 — the wiring inventory (TASK-0348 Phase 6)

A survey of every capability init and execd place for windowd (slot, endpoint, owner, placer)
settles what the respawn replays and adds what the first amendment missed:

1. **Any task could plant a capability in another task.** `cap_transfer`/`cap_transfer_to`
   check the rights, never the target: a task holding a capability could place it in any task's
   empty slot — and so take the slot a parent was about to pin a route into (a forged route).
   Every caller on the OS path transfers into its own child (init → services, execd → apps, a
   work pool → its threads). **A transfer goes only into the caller's own child**, REPLACE_DEAD
   included; anything else is `EPERM` (`test_reject_cap_transfer_into_a_stranger`).
2. **A dead endpoint drops the capabilities in flight without a census.** Its queue is cleared,
   and a reply capability moved with a still-queued request vanishes uncounted: the caller
   blocked on its private inbox never learns its call ended (screencapd's, inputd's, an app's
   call into a dying windowd). Closing an endpoint now **judges every endpoint the dropped
   messages named**, exactly like a `cap_close` (RFC-0079's census), on both close paths (an
   owner's death; a MANAGE close or a reclaim).
3. **REPLACE_DEAD** is a fifth argument of `cap_transfer_to`, `flags`
   (`CAP_TRANSFER_REPLACE_DEAD = 1`, unknown bits `EINVAL`): the target slot may be empty or hold
   a capability to a **dead** endpoint, which is dropped (its census judged) and replaced; a
   live capability, or a capability of any other kind, stays `EPERM`.
4. **init keeps what the respawn needs.** The SENDs of windowd's legs (gpud's request and
   response, bundlemgrd, sessiond, settingsd, abilitymgr's request and response, imed — closed
   after boot until now —, clipboardd) move into the respawn context. The respawn mints
   windowd's own endpoints anew (request 32, response 8, reply inbox, timer, the two watches),
   pins its declared slots, re-pins the five declared `→ windowd` client legs (inputd 5/6, imed
   6/7, execd 8/9, screencapd 7, memd 11) with REPLACE_DEAD — a host table test over
   `SERVICE_SPECS` proves the plan names every one —, drains gpud's and abilitymgr's response
   endpoints (a stale answer would be the new instance's first frame), places it on cpu0 and
   resumes it. The respawner keeps one restart engine per service (a second death no longer
   drops the first one's schedule).
5. **execd** learns from init that windowd is back (`OP_COMPOSITOR_BACK` on execd's server
   endpoint, init's kernel id only): it re-pins each live app's 5/6 from its own fresh 8/9 with
   REPLACE_DEAD and sends each app `OP_COMPOSITOR_BACK` on its event channel with a fresh SEND of
   that channel moved in (the app gave its last one to the dead windowd). It closes the two
   windowd clones it kept per launch (they leaked, pinning dead ids forever).
6. **gpud** answers a reveal again (`STATUS_REVEALED` was once per boot: the new windowd would
   never reveal); its surface table outlives windowd by construction — gpud holds no endpoint
   of windowd's, so it never learns of the death — and `OP_SURFACE_ADOPT` lists it.
7. **settingsd and sessiond** drop a watcher whose channel is dead at the first failed push
   (settingsd kept one for eight failures in a table of eight — the new windowd's three watches
   would be refused); settingsd's table grows.
8. **The trigger**: services spawned by init are not killable (ADR-0082) and need not be —
   windowd ends itself on the shortcut registry's "restart the desktop" chord (a real desktop
   affordance: the screen comes back without a reboot), and any fault takes the same path.
9. **Ghosts are surfaces whose window has no client yet**: the new windowd shows every adopted
   buffer at its last place in its last stacking, like a replaced surface (RFC-0114) — until its
   app, re-attached, presents a surface of its own (the ghost's buffer is then freed), or until
   execd's sweep of the live apps ends without that owner (execd names the owners it told) —
   never a timeout. A minimized window has no ghost.
10. **A frozen app cannot read its `OP_COMPOSITOR_BACK`.** The restarted windowd tells the
    lifecycle authority it is back (`OP_COMPOSITOR_BACK` on its abilitymgr leg); abilitymgr thaws
    every frozen instance once, the apps re-attach, and the sightings start anew (ADR-0083). A
    re-attached app's new window takes the place of its owner's topmost ghost (the kernel id is
    the owner: instances of one app are interchangeable there) and the ghost goes; a window
    without a ghost cascades. The per-window state that lived only in windowd — minimized, tiled
    — died with it: a window comes back shown (the app keeps its own state).

## Amendment 2026-11-02 (b) — what Phases 1–2 built

Phases 1 and 2 are built in the order the inventory gave; where the build decided differently
from the plan above, the build is the contract:

1. **The lifecycle authority hears every compositor's start**, not a restart alone:
   `OP_COMPOSITOR_UP` (abilitymgr op 9, `[A,M,1,9]`, one-way, windowd's kernel id alone) goes
   once at windowd's start, before any sighting of that instance. What an earlier compositor saw
   is void: every frozen instance is thawed into the background, every hidden clock stops
   (`abilitymgr: compositor up (thawed=N)`; the first boot's is `thawed=0`). A dying instance is
   its death report's alone.
2. **One-instance roles are declarative**: a bundle whose `bundle_type` is `shell`, `greeter` or
   `ime` launches once; a LAUNCH while its instance runs (spawned, not dying) answers that
   instance and spawns nothing (`abilitymgr: launch kept (app=…`). The restarted windowd asks for
   the shell at its session decision like at boot — the running shell answers and rejoins.
3. **Until Phase 3, a new compositor releases what the old one composed**: gpud takes the new
   instance's framebuffer request (every instance asks once, first) as the end of the old one —
   the retained layer set goes from the display and the GPU, and every app surface object is
   unbound, closed and uncharged at once (`gpud: compositor restarted (owners=N …)`). The windows
   leave the screen for the restart and return as their apps present. Phase 3 replaces the
   release with the adopt.
4. **A notice is never lost to a full queue**: execd's `OP_COMPOSITOR_BACK` to an app whose
   channel is full (a frozen app's refuses at once) is OWED — the channel armed writable on
   execd's waitset (RFC-0033 amendment) and the notice sent once it has room
   (`execd: compositor back (apps=N owed=M failed=K)`, then `… told (pid=`). execd never waits
   on an app.
5. **app-host's bring-up is re-entrant** (`probe/reconnect.rs`): `OP_SURFACE_CLOSED` → the app
   saves its record and ends; an EOF without it → the app saves (`reason=compositor`) and waits
   in the kernel, without a clock and without the EOF flag, for its parent's notice. A BACK that
   arrives in ANY wait ends that wait and its SEND is kept — it is the only way back (a kept
   SEND makes the app a sender of its own channel, so no EOF would end the wait). The rejoin
   attaches the channel under a fresh nonce, sends the intent and takes the new compositor's
   rect; the event loop re-creates the surface (no destroy: the old one died), replays the
   attach's presentation snapshot (its generation counter starts anew with the new compositor),
   presents the whole window and announces its text focus again. A create the new compositor
   refuses ends the app (the launch's fail-closed rule); a CLOSED during a create ends it too.
   A launch the compositor's death interrupts before its first frame fails — its waits close a
   moved capability so the EOF comes — and the launcher may launch it again.
6. **inputd** reaches windowd over its declared slots alone (the route ask is gone: init may be
   busy with the very restart). The one-shot facts of a push that failed were the dead
   compositor's and are void — a restart chord replayed would restart the next one. A key while
   no compositor runs is dropped, never held for a window it may not belong to, and the outage is
   named once (`inputd: keys dropped (no compositor)`); a failed send to a live route stays a
   lost key (`inputd: FAIL key send`).
7. **sessiond** pushes a new watcher's snapshot to every watcher: a dead one — the compositor
   the new one replaces — goes then, not at the next transition.
8. **The proof** is the many-windows lane: twelve windows from the launcher, then the restart
   chord over them. The run ends on an ordered chain whose last step is a group
   (`windowd: restart requested (chord)`, then `windowd: windows drawn (n=12)` AND
   `windowd: desktop revealed (seq=` in any order — the reveal waits for the shell's own frame,
   the heaviest surface and often the last: a run that ended on the windows alone stopped before
   the shell drew), the pixel proof snapshots that frame, and `scripts/qemu-test.sh` checks the
   order (the twelve and the reveal AFTER the restart), every app told (`failed=0`) and no app
   that gave up its rejoin.
9. **A replaced desktop surface keeps its owner**: the greeter a shell replaces still presents
   into its last surface until it ends — those presents are stale, not a stranger's — so the
   replaced surface carries its own owner, and the desktop's owner changes only once the new
   surface exists (a refused create changes nothing).

## Proposed design

- **Phase 0 — measure**: kill windowd on the visible lane (a QMP harness verb); record which
  services log EOF, which slots hold dead caps, how long the screen shows the last frame, what
  gpud does with the orphaned context.
- **Phase 1 — the fleet re-pinned**:
  - Kernel: `cap_transfer_to` (31) gains **REPLACE_DEAD** — it replaces a slot only when that slot
    holds a capability to a dead endpoint (anything else stays `EPERM`); no live capability can
    ever be overwritten.
  - init: `respawnable()` grows to windowd; the arm re-spawns it from the sealed image, re-mints
    its owned endpoints, pins its own slots, and re-pins **every declared `→ windowd` slot of every
    live service with REPLACE_DEAD — one loop over `SERVICE_SPECS`** (a host table test proves the
    plan); init keeps the boot-held endpoints windowd's legs need (it closes `imed_req` today);
    `clear_stale` once pinned.
  - execd (a new init → execd verb): re-pins its own 8/9 and every live child's 5/6 with
    REPLACE_DEAD.
- **Phase 2 — clients reconnect without polling**:
  - windowd sends `OP_SURFACE_CLOSED` on an app's channel before it closes a window on purpose;
    **an EOF without it is the compositor's death**. app-host's ack waits take the EOF too.
  - execd keeps the owner's SEND of each child's event channel (the owner is exempt from the EOF
    census, so the close EOF still fires) and, once windowd is back, sends `OP_COMPOSITOR_BACK`
    on that channel **with a fresh SEND moved in it** — the app, blocked on its own channel,
    wakes with what it needs to re-attach.
  - app-host's bring-up becomes a re-entrant state machine: attach events (nonce) → re-create
    its surface (or adopt, Phase 3) → tree → present; the shell re-subscribes its feed.
  - imed, inputd, screencapd and memd use fixed slots that init re-pinned: their next send works.
  - abilitymgr makes LAUNCH of a live **singleton role** (shell, greeter, osk) answer the live
    instance instead of a second process.
- **Phase 3 — the screen survives with the apps' pixels**:
  - gpud keeps the last scanned frame and every surface object of the dead windowd's context
    (charged to their owners) until the new windowd adopts or frees them.
  - The new windowd asks `OP_SURFACE_ADOPT`: per owner, the live buffers and their last scene
    geometry (destination, stacking) from gpud's retained layer set; it composes them as
    **ghost windows** — the app's own last pixels — and emits `windowd: restarted (ghosts=N)`.
  - An app's re-attach (same kernel id) turns its ghost into its window again; a ghost whose app
    never comes back is dropped when abilitymgr reports the instance gone.

## Amendment 2026-11-03 (c) — what Phase 3 built (TASK-0348 P9)

- **gpud keeps the objects, not the frame.** At every present gpud records where the frame drew
  each object's body — the `CompositeLayer` whose key names the body kind
  (`nexus_gfx::command::layer_wire::layer_key`): destination, size, slot, draw index (z), and
  the window key (`0xFFFF` the desktop). A new compositor's framebuffer request forgets the layer
  set and marks every object an ORPHAN; what the compositor before never adopted, and every object
  being freed, is released then. The retained scanout frame itself is not kept: the new windowd's
  first frame draws the ghosts from the objects.
- **`OP_SURFACE_ADOPT` (20)** is `[op]`; the answer `[0x94, restarted u8, count u8,
  remaining u16, entries × 35 B]` in pages of at most 14 (`nexus_display_proto::surface_adopt`).
  gpud answers windowd alone; each entry is the object as `Allocated` describes it plus its body's
  place; the orphans no frame showed are released at the first page (a minimized window has no
  ghost); on the board each adopted object is bound again in the new windowd's GPU context (the
  old context took its bindings with it); the display's own binding never went.
- **windowd adopts before its first frame** (after the grant and its GPU context, before
  `ready`): each entry becomes a ghost — a window slot whose RETIRING surface is the adopted one
  (`ClientSurfaces::adopt`: shows its slot at once, takes no present, owes no ack), framed where
  the body was, shown in the old draw order; the desktop's object becomes the desktop's retiring
  surface. `windowd: restarted (ghosts=N)` when a compositor ran before.
- **A rejoin takes its ghost over**: a create without a bound channel binds to the owner's ghost
  (the kernel id captured at the create; of the surface's size first, the topmost of those —
  `ghost_pick`), never another owner's; the new surface presents into the slot and the ghost
  retires the usual way (its object freed). A create of an owner without a ghost cascades.
- **`OP_APPS_ALIVE` (41, execd → windowd)**: `[hdr, count u8, owner u64 × count]`, at most 60.
  After its compositor-back notices execd names the kernel ids of its live apps; windowd (execd's
  kernel id alone) closes the ghosts of every other owner — their apps are gone. A full queue
  makes the list owed (windowd's send armed writable on execd's waitset). Rule 9's "sweep" is
  this frame; nothing waits on a clock.

## Security considerations

Re-pinning a SEND into another process's slot is init's existing power (declared_slots); the
re-grant to app-host children is execd's existing power over its children; no client learns a
cap it did not hold before. REPLACE_DEAD can only replace a capability whose endpoint is dead —
a live capability is never overwritten, so the mode grants nothing a `CopyToSlot` into an empty
slot did not. A client that reconnects to the new windowd is identified by the same kernel
service id as before — the ghost it reclaims is its own; `OP_SURFACE_ADOPT` is served to
windowd's kernel id only.

- **`test_reject_*`**: REPLACE_DEAD over a live capability; over a non-endpoint capability;
  `OP_SURFACE_ADOPT` from a stranger; an app re-attaching with another app's nonce.
- Built (Phases 1–2): `test_reject_cap_transfer_into_a_stranger` and the REPLACE_DEAD matrix
  (`cap_rules`, host; `KSELFTEST` on boot); `test_reject_a_compositor_up_from_another_sender`,
  `test_reject_a_compositor_up_thaw_of_a_dying_instance`,
  `test_reject_a_kept_launch_for_an_ordinary_app_or_an_unspawned_role` (abilitymgr);
  `test_reject_a_long_or_foreign_frame` (the two restart frames). execd takes the notice from
  init's kernel id alone; an app keeps a moved capability only when it came with a BACK on its
  own channel — any other is closed, so no stranger's SEND can keep the channel alive; windowd
  refuses a present or destroy of a surface its sender does not own.

## Proof / validation strategy

- Host: the respawn arm's slot plan as a table test over `SERVICE_SPECS` (every `→ windowd`
  route re-pinned, nothing else); app-host reconnect as a state-machine test.
- QEMU (visible lane): the injector kills windowd via a QMP-driven harness verb; the ladder
  waits for `windowd: restarted (ghosts=` and a new `SELFTEST: ui v7 screenshot ok` that differs
  from the pre-kill frame only where the apps re-presented.
- Board: kill windowd over the console; the desktop returns.

## Implementation Checklist

- [x] Phase 0 measured (the wiring inventory — amendment 2026-11-02; the code survey replaced
      the kill run)
- [x] Phase 1 built: REPLACE_DEAD; init re-mints, re-pins the fleet in one loop; execd re-pins
- [x] Phase 2 built: `OP_SURFACE_CLOSED`, `OP_COMPOSITOR_BACK`, app-host's re-entrant bring-up,
      singleton launch (the many-windows lane's restart; amendment (b))
- [x] Phase 3 built: gpud keeps the frame and the objects, `OP_SURFACE_ADOPT`, ghosts with the
      apps' own pixels (2026-11-03, QEMU + board; amendment (c))
- [x] Task linked (TASK-0348 Phase 6); markers in `scripts/qemu-test.sh` when built
