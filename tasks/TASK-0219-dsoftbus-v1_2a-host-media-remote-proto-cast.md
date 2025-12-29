---
title: TASK-0219 DSoftBus v1.2a (host-first): Media Remote (media.remote@1) protocol + remote control + transfer/group state sync + share@1 fallback + deterministic tests
status: Draft
owner: @media
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSoftBus secure channels + share@1 resume: tasks/TASK-0195-dsoftbus-v1_1a-host-secure-channels-encrypted-streams-share.md
  - DSoftBus directory + rpcmux + health: tasks/TASK-0211-dsoftbus-v1_1c-host-busdir-rpcmux-health-flow.md
  - Media UX v2.1 (audiod + mediasessd integration): tasks/TASK-0217-media-v2_1a-host-audiod-deterministic-graph-mixer.md
  - Media UX v2 semantics (playerctl/handoff): tasks/TASK-0184-media-ux-v2a-host-handoff-playerctl-deterministic-clock.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want deterministic “cast/remote control” over DSoftBus while staying offline and QEMU-tolerant:

- discover peers that expose `media.remote@1`,
- remote-control their media sessions,
- transfer playback control to a peer (audio remains local on each node),
- optionally offer a file via `share@1` and then instruct the peer to play it,
- support simple group sessions where play/pause/seek state is synchronized with bounded drift using deterministic clocks.

This task is host-first and uses loopback/localSim style transports; UDP remains devnet-gated and out of scope here.

## Goal

Deliver:

1. Cap’n Proto IDL for `media.remote@1`:
   - `hello/query/control/loadUri/offerOk/ping`
   - naming alignment:
     - use existing `mediasessd` service name (repo uses `mediasessd`, not `mediasessiond`)
   - capabilities list and stable error mapping
2. `mediaremoted` host-first service:
   - bridges DSoftBus rpcmux calls to local `mediasessd` + `audiod` control surfaces
   - publishes itself via busdir with deterministic metadata
3. `mediacast` library (host-first):
   - `Transfer`:
     - attempt `loadUri(pkg://...)` on peer when resolvable
     - fallback: `share@1` offer → wait → `offerOk` → play
     - local pauses only after remote confirms `playing`
   - `GroupSolo`:
     - mirror play/pause/seek deterministically
   - `GroupParty`:
     - drift-bounded sync:
       - use audiod block clocks (10ms blocks) and exchange block indices
       - align to `now + N blocks`
       - resync if drift > threshold with bounded seek steps
     - NOTE: this is control-plane sync only; no shared audio buffer
4. Deterministic host tests `tests/dsoftbus_media_remote_v1_2_host/`:
   - directory discovery of `media.remote@1` peers via busdir
   - remote load/play and query state
   - transfer: A stops after B starts; position monotonic in blocks
   - group solo: pause/seek mirrored
   - group party: injected delay triggers resync; drift bounded
   - share fallback: offer+sha256 ok, then play

## Non-Goals

- Kernel changes.
- UDP discovery/transport (devnet gated tasks).
- Streaming remote audio output (explicitly not in v1.2).

## Constraints / invariants (hard requirements)

- Determinism:
  - injected monotonic clocks in tests
  - stable ordering of peers and sessions
  - stable drift calculation and resync policy
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success:
  - “transfer ok” only when remote reports playing and local is paused/stopped.

## Red flags / decision points (track explicitly)

- **YELLOW (group sync semantics)**:
  - “party mode” must define a strict, deterministic model (block-aligned) and avoid wallclock/NTP claims.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_media_remote_v1_2_host -- --nocapture`

## Touched paths (allowlist)

- `source/services/mediaremoted/` (new)
- `userspace/libs/mediacast/` (new)
- `tools/nexus-idl/schemas/media_remote.capnp` (or canonical schema location)
- `tests/dsoftbus_media_remote_v1_2_host/` (new)
- docs may land in v1.2b

## Plan (small PRs)

1. IDL + mediaremoted bridge + deterministic fixtures
2. mediacast transfer + share fallback + tests
3. group solo/party sync + drift bounds + tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove media remote discovery/control, transfer, group sync, and share fallback.
