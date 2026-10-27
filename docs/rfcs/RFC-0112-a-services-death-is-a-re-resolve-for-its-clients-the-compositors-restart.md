<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0112: A service's death is a re-resolve for its clients — the compositor's restart brings the desktop back with every app's band as a ghost until its next present

- Status: Draft (seeded 2026-10-27 from TASK-0333 6d-4's assessment; nothing built; scheduled as TASK-0333 6g, after 6f — the operator's decision 2026-10-27)
- Owners: @runtime @reliability @ui
- Created: 2026-10-27
- Last Updated: 2026-10-27
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6d-4, open)
  - ADRs: `docs/adr/0057-service-restart-capability-re-resolve.md` (the pinched pilot: re-mint
    the owner-bound request endpoint, re-distribute, STALE on the route table),
    `docs/adr/0073-…` (death is an event), `docs/adr/0075-…` (the compositor never waits on a
    client), `docs/adr/0083-…` (the lifecycle contract the restart must keep)
  - RFCs: RFC-0079 (EOF: a client learns of a server's death on its receive), RFC-0086 (the
    window feed), RFC-0111 (the lifecycle)

## Status at a Glance

- **Phase 0 (measure what dies with windowd today)**: ⬜
- **Phase 1 (init's respawn arm for windowd: re-mint, re-pin the fleet, STALE)**: ⬜
- **Phase 2 (every client reconnects on EOF: shell, app-host, imed, inputd, screencapd, execd's
  children)**: ⬜
- **Phase 3 (the screen survives: gpud keeps the last frame, the bands come back as ghosts)**: ⬜

Definition: "Complete" = `windowd: restarted (ghosts=N)` on the visible lane after the harness
kills windowd, and on the board the desktop returns with every app window drawn from its last
frame until the app presents again.

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

## Proposed design (seed)

- **Phase 0 — measure**: kill windowd on the visible lane; record which services log EOF, which
  slots hold dead SEND caps (`init: FAIL declared slot route send` is the signature), how long
  the screen stays black, what gpud does with the orphaned planes.
- **Phase 1 — init**: `respawnable()` grows to windowd; the arm re-mints the request endpoint,
  pins it into windowd's declared server slots and into every declared `→ windowd` route slot of
  every live service (the topology lists them: this is ONE loop over `SERVICE_SPECS`, not a
  windowd-specific list), marks the route-table entry STALE, and tells execd (a new init → execd
  verb) to re-grant its children.
- **Phase 2 — clients**: every windowd client already receives on a channel windowd minted; the
  EOF on it is the signal. The shell re-subscribes its feed and re-creates its surfaces; app-host
  re-creates the surface in its old band (the band id is the app's, keyed by kernel service id
  as the sightings are) and presents; imed/inputd/screencapd re-resolve on their next send.
- **Phase 3 — the screen**: gpud keeps the last frame until a present arrives; the new windowd
  publishes the bands it learns from abilitymgr's live instances as ghosts
  (`windowd: restarted (ghosts=N)`) and replaces each on its first present.

## Security considerations

Re-pinning a SEND into another process's slot is init's existing power (declared_slots); the
re-grant to app-host children is execd's existing power over its children; no client learns a
cap it did not hold before. A client that reconnects to the new windowd is identified by the same
kernel service id as before — the band it reclaims is its own.

## Proof / validation strategy

- Host: the respawn arm's slot plan as a table test over `SERVICE_SPECS` (every `→ windowd`
  route re-pinned, nothing else); app-host reconnect as a state-machine test.
- QEMU (visible lane): the injector kills windowd via a QMP-driven harness verb; the ladder
  waits for `windowd: restarted (ghosts=` and a new `SELFTEST: ui v7 screenshot ok` that differs
  from the pre-kill frame only where the apps re-presented.
- Board: kill windowd over the console; the desktop returns.

## Implementation Checklist

- [ ] Phase 0 measured (the kill on the visible lane, the EOF census)
- [ ] Phase 1 built: init re-mints, re-pins the fleet, STALE, execd re-grants
- [ ] Phase 2 built: shell and app-host reconnect on EOF
- [ ] Phase 3 built: gpud keeps the frame, ghosts until the next present
- [ ] Task linked (TASK-0333 6d-4 or its own task); markers in `scripts/qemu-test.sh`
