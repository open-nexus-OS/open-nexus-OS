---
title: TASK-0255 Audio v0.9b (OS/QEMU): audiod service + i2sd/codecd stubs + mediasessiond hooks + `nx audio` + selftests
status: Draft
owner: @media
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusMedia SDK track (audio/video/image): tasks/TRACK-NEXUSMEDIA-SDK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Audio core (host-first): tasks/TASK-0254-audio-v0_9a-host-mixer-ringbuffer-levels-deterministic.md
  - Audiod v16b baseline: tasks/TASK-0100-ui-v16b-audiod-mixer.md
  - Media v2.1a baseline: tasks/TASK-0217-media-v2_1a-host-audiod-deterministic-graph-mixer.md
  - Power/idle: tasks/TASK-0236-power-v1_0a-host-governor-wakelocks-residency-standby-deterministic.md
  - Device MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU integration for Audio v0.9:

- `audiod` service (ringbuffer + mixer + device graph),
- `i2sd`/`codecd` device layer stubs,
- mediasessiond hooks (play/pause/duck, wakelock),
- tone generator + WAV player.

The prompt proposes these services. `TASK-0100` and `TASK-0217` already plan audiod mixer (per-app streams, session/graph based). This task delivers the **OS/QEMU integration** with device layer stubs and mediasessiond hooks, complementing the existing mixer implementations.

## Goal

On OS/QEMU:

1. **DTB updates**:
   - extend `pkg://dts/virt-nexus.dts`:
     - node `i2s@10003000` with IRQ and clocks (stubbed)
     - codec node `codec@1a` on I²C with simple properties (`compatible="nexus,codec-null"`), routes `i2s -> codec -> hp/speaker`
   - rebuild DTB
2. **audiod service** (`source/services/audiod/`):
   - engine: fixed **48 kHz, stereo, s16le**, block size 480 frames (10 ms)
   - ringbuffer: single producer (client) → mixer pull; size 4× block (using library from `TASK-0254`)
   - mixer: N streams summed with per-stream gain; master Level applied last; soft-clip to int16 (using library from `TASK-0254`)
   - sinks:
     - `null` (default, deterministic timebase via system timer)
     - `file` sink (optional): writes WAV to `state:/audio/out.wav` (for testing; gated on `/state`)
   - clocking: vs timer tick every 10 ms
   - API (`audio.capnp`): `defaultFormat()` → `Format`, `sinkState()` → `SinkState`, `setLevel(lvl)` → `ok`, `getLevel()` → `Level`, `playTone(hz, ms)`, `openStream(appId)` → `(shm, frames)`, `start()`, `pause()`, `stats()` → `Stats`
   - markers: `audiod: ready`, `audiod: tick seq=… mix=… streams=…`, `audiod: underrun app=… frames=…`, `audiod: level linear=… muted=…`
3. **Device layer stubs** (`source/services/i2sd/`, `source/services/codecd/`):
   - `i2sd`: small userspace driver that would push blocks to hardware; for QEMU **disabled**; exposes ready/ok markers
   - `codecd`: validates route selection and sample format; in null mode acknowledges config
   - `audiod` can select sink `i2s` when enabled; otherwise stays on `null`
4. **Media session integration**:
   - extend `mediasessiond`:
     - on `play`/`pause`/`duck` events adjust per-session gain in `audiod`
     - acquire/release **partial wakelock** via `powerd` while `playing` (gated on `TASK-0236`)
   - markers: `mediasession: playing`, `audiod: duck -6dB`
5. **CLI diagnostics** (`nx audio ...` as a subcommand of the canonical `nx` tool):
   - `nx audio info`, `nx audio level 0.75 [--mute|--unmute]`, `nx audio tone 440 300`, `nx audio record start state:/audio/out.wav`, `nx audio record stop`, `nx audio stats`
   - markers: `nx: audio level=0.75`, `nx: audio tone 440ms=300`, `nx: audio record start`
6. **Demo utilities**:
   - `userspace/apps/tonegen` (fills its stream with sine @ given Hz)
   - `userspace/apps/wavplay` (decodes a tiny fixture WAV and pushes to ring)
   - fixtures under `pkg://fixtures/audio/`
7. **Settings integration**:
   - seed/extend `settingsd` keys: `audio.master.level` (float 0.0..1.0, user), `audio.master.muted` (bool, user), optional `audio.sink` (`"null"|"file"|"i2s"`), device route effective only if available
   - provider applies to `audiod`
8. **OS selftests + postflight**.

## Non-Goals

- Kernel changes.
- Real hardware (QEMU/null sink only).
- Full audio graph (handled by `TASK-0217`).

## Constraints / invariants (hard requirements)

- **No duplicate audio authority**: `audiod` is the single authority for audio mixing and routing. Do not create parallel audio services.
- **No duplicate mixer authority**: `audiod` uses the mixer library from `TASK-0254`. `TASK-0100`/`TASK-0217` should share the same mixer contracts to avoid drift.
- **Determinism**: ringbuffer, mixer, levels, and file sink must be stable given the same inputs.
- **Bounded resources**: ringbuffer size is bounded; mixer streams are capped.
- **Device access**: assumes `TASK-0010` (device MMIO access model) is Done; real I²S/I²C hardware paths may additionally
  require device-class caps (bus/controller access) beyond the v1 MMIO primitive.
- **Power/idle gating**: mediasessiond wakelock requires `TASK-0236` (power governor, wake-locks) or equivalent.
- **Persistence gating**: file sink requires `/state` (`TASK-0009`) or equivalent. Without `/state`, file sink must be disabled or explicit `stub/placeholder` (no "written ok" claims).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (audio authority drift)**:
  - Do not create a parallel audio service that conflicts with `audiod`. `audiod` is the single authority for audio mixing and routing.
- **RED (mixer authority drift)**:
  - Do not create parallel mixer implementations. `audiod` uses the mixer library from `TASK-0254`. `TASK-0100`/`TASK-0217` should share the same mixer contracts to avoid drift.
- **YELLOW (ringbuffer vs stream model)**:
  - `TASK-0100` uses per-app stream model. This task uses pull-based ringbuffer. Document the relationship explicitly: ringbuffer can be used as the backing store for streams, or this task can extend `TASK-0100` to support ringbuffer-based streams.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Audio core: `TASK-0254`
- Audiod baseline: `TASK-0100` (per-app streams, volume/mute)
- Media v2.1a baseline: `TASK-0217` (session/graph based, 48k/10ms)
- Power/idle: `TASK-0236` (prerequisite for wakelock)
- Device MMIO access: `TASK-0010` (prerequisite for I²S/codec)
- Persistence: `TASK-0009` (prerequisite for file sink)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `audiod: ready`
- `audiod: tick seq=… mix=… streams=…`
- `audiod: underrun app=… frames=…`
- `audiod: level linear=… muted=…`
- `mediasession: playing`
- `audiod: duck -6dB`
- `SELFTEST: audio tone ok`
- `SELFTEST: audio stream start/stop ok`
- `SELFTEST: audio record ok` (only if `/state` available; otherwise explicit `stub/placeholder`)
- `SELFTEST: audio duck ok`

## Touched paths (allowlist)

- `pkg://dts/virt-nexus.dts` (extend: I²S + codec-null nodes)
- `source/services/audiod/` (new; or extend existing)
- `source/services/i2sd/` (new; device stub)
- `source/services/codecd/` (new; device stub)
- `source/services/mediasessiond/` (extend: play/pause/duck hooks, wakelock)
- `source/services/settingsd/` (extend: audio provider keys)
- `tools/nx/` (extend: `nx audio ...` subcommands; no separate `nx-audio` binary)
- `userspace/apps/tonegen/` (new)
- `userspace/apps/wavplay/` (new)
- `pkg://fixtures/audio/` (new)
- `source/apps/selftest-client/` (markers)
- `docs/audio/overview.md` (new)
- `docs/audio/api.md` (new)
- `docs/tools/nx-audio.md` (new)
- `tools/postflight-audio-v0_9.sh` (new)

## Plan (small PRs)

1. **DTB updates + audiod service**
   - DTB: I²S + codec nodes
   - audiod service (ringbuffer + mixer + sinks)
   - markers

2. **Device stubs + mediasessiond hooks**
   - i2sd/codecd stubs
   - mediasessiond hooks (play/pause/duck, wakelock)
   - markers

3. **CLI + demo utilities + settings**
   - `nx audio` CLI
   - tonegen/wavplay apps
   - settings provider
   - markers

4. **OS selftests + postflight**
   - OS selftests
   - postflight

## Acceptance criteria (behavioral)

- `audiod` ticks advance, tones/streams mix without underruns, recording produces stable WAV (if `/state` available), mediasession ducking works.
- All four OS selftest markers are emitted (or explicit `stub/placeholder` if gated).
