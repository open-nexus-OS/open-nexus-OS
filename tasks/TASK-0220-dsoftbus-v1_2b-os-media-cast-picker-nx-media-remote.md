---
title: TASK-0220 DSoftBus v1.2b (OS/QEMU): Media Remote cast/transfer/group UI (SystemUI) + nx-media remote CLI + selftests/docs (loopback default)
status: Draft
owner: @media
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSoftBus v1.1 OS wiring (busdir/rpcmux/health): tasks/TASK-0212-dsoftbus-v1_1d-os-busdir-ui-selftests.md
  - DSoftBus v1.2 host core (media remote + mediacast): tasks/TASK-0219-dsoftbus-v1_2a-host-media-remote-proto-cast.md
  - Media UX v2.1 OS baseline (mini-player + nx-media): tasks/TASK-0218-media-v2_1b-os-focus-ducking-miniplayer-nx-media.md
  - DSoftBus UDP devnet gating (must remain gated): tasks/TASK-0196-dsoftbus-v1_1b-devnet-udp-discovery-gated.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With busdir/rpcmux in OS (v1.1) and media remote semantics proven host-first (v1.2a),
we wire a “cast picker” UX and remote controls into SystemUI and `nx-media`, proven in QEMU loopback mode.

Audio remains local on each node; v1.2 is control-plane only.

## Goal

Deliver:

1. SystemUI cast picker + remote controls:
   - mini-player adds a Cast button
   - picker lists peers from `busdir.list("media.remote@1")` (or stable name mapping)
   - shows peer health (up/degraded/down)
   - actions:
     - Transfer
     - Group Solo
     - Group Party
     - Disconnect
   - show drift badge for party mode (green/yellow/red thresholds; deterministic mapping)
   - markers:
     - `ui: cast picker open`
     - `ui: cast transfer fp=<fp>`
     - `ui: cast group mode=<solo|party> fp=<fp>`
     - `ui: cast disconnect`
2. `nx-media` extensions (host tool):
   - `cast peers/transfer/group/disconnect`
   - `remote list/control`
   - NOTE: QEMU selftests must not require running host CLIs inside QEMU
3. OS selftests (bounded):
   - spawn loopback peer B exposing `media.remote@1`
   - transfer:
     - `SELFTEST: media cast transfer ok`
   - group solo:
     - `SELFTEST: media cast group solo ok`
   - group party:
     - `SELFTEST: media cast party sync ok`
   - disconnect:
     - `SELFTEST: media cast disconnect ok`
4. Policy/schema:
   - `schemas/dsoftbus_media_v1_2.schema.json`:
     - drift threshold, sync interval, transfer timeout, share enable
   - caps:
     - `media.remote.publish` for mediaremoted
     - `media.remote.control` for SystemUI and `nx-media`
     - reuse `share.send/recv` for file offer fallback
5. Docs:
   - protocol summary and determinism model
   - transfer vs group modes
   - drift model and bounds
   - nx-media remote usage

## Non-Goals

- Kernel changes.
- Enabling UDP mode by default (must remain devnet-gated).
- Remote audio streaming.

## Constraints / invariants (hard requirements)

- Loopback default; UDP path remains gated (aligned with `TASK-0196`).
- No fake success: selftests validate state via service queries and deterministic metrics, not log greps.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_media_remote_v1_2_host -- --nocapture` (from v1.2a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=200s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: media cast transfer ok`
    - `SELFTEST: media cast group solo ok`
    - `SELFTEST: media cast party sync ok`
    - `SELFTEST: media cast disconnect ok`

## Touched paths (allowlist)

- `userspace/systemui/tray/mini_player/` (cast picker UI)
- `source/services/mediaremoted/` (OS bring-up)
- `userspace/libs/mediacast/` (OS wiring)
- `tools/nx-media/` (extend)
- `source/apps/selftest-client/`
- `schemas/dsoftbus_media_v1_2.schema.json`
- `docs/media/remote.md` + `docs/tools/nx-media.md` + `docs/ui/testing.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. OS service wiring: mediaremoted publish + busdir integration
2. mini-player cast picker + remote control surface
3. nx-media remote/cast commands
4. selftests + docs + postflight wrapper (delegating)

## Acceptance criteria (behavioral)

- In QEMU, Media Remote discovery/control/transfer/group/disconnect are proven deterministically in loopback mode via selftest markers.

