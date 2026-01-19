---
title: TASK-0254 Audio v0.9a (host-first): mixer + ringbuffer + levels + deterministic tests
status: Draft
owner: @media
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusMedia SDK track (audio/video/image): tasks/TRACK-NEXUSMEDIA-SDK.md
  - Audiod v16b baseline: tasks/TASK-0100-ui-v16b-audiod-mixer.md
  - Media v2.1a baseline: tasks/TASK-0217-media-v2_1a-host-audiod-deterministic-graph-mixer.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic audio pipeline foundation:

- pull-based ringbuffer (single producer → mixer pull),
- mixer (N streams summed with per-stream gain, master Level applied last, soft-clip to int16),
- levels (linear 0.0..1.0, mute override),
- file sink (WAV output for testing).

The prompt proposes a ringbuffer-based mixer with levels. `TASK-0100` and `TASK-0217` already plan audiod mixer (per-app streams, session/graph based). This task delivers the **host-first core** (ringbuffer, mixer, levels, file sink) that can be reused by both existing audiod implementations and the new v0.9 architecture.

## Goal

Deliver on host:

1. **Ringbuffer library** (`userspace/libs/audio-ringbuffer/`):
   - single producer (client) → mixer pull
   - size 4× block (480 frames × 4 = 1920 frames)
   - deterministic wrap-around behavior
   - underrun detection
2. **Mixer library** (`userspace/libs/audio-mixer/`):
   - N streams summed with per-stream gain
   - master Level applied last (linear 0.0..1.0, mute override)
   - soft-clip to int16 (saturating arithmetic)
   - deterministic mixing (stable rounding rules)
3. **Level library** (`userspace/libs/audio-level/`):
   - linear 0.0..1.0 range
   - mute override (mute → output = 0 regardless of level)
   - unmute restores previous level
   - linear→dB mapping documented
4. **File sink library** (`userspace/libs/audio-sink-file/`):
   - WAV header generation (48 kHz, stereo, s16le)
   - write 200 ms; WAV header/length as expected; hash stable
   - deterministic file output (fixed mtime/uid/gid)
5. **Host tests** proving:
   - mix correctness: sum of two sines @ −12 dB each equals expected peak; soft-clip bounded
   - ringbuffer: write/read indices wrap; no data races; underrun increments
   - level semantics: mute overrides level; unmute restores; linear→dB mapping documented
   - file sink: write 200 ms; WAV header/length as expected; hash stable

## Non-Goals

- OS/QEMU integration (deferred to v0.9b).
- Real hardware (QEMU/null sink only).
- Full audio graph (handled by `TASK-0217`).

## Constraints / invariants (hard requirements)

- **No duplicate mixer authority**: This task provides mixer library. `TASK-0100` and `TASK-0217` already plan audiod mixer. This task should extend or unify with existing mixer contracts to avoid drift.
- **Determinism**: ringbuffer, mixer, levels, and file sink must be stable given the same inputs.
- **Bounded resources**: ringbuffer size is bounded; mixer streams are capped.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (mixer authority drift)**:
  - Do not create parallel mixer implementations. This task should extend or unify with existing mixer contracts from `TASK-0100`/`TASK-0217` to avoid drift.
- **YELLOW (ringbuffer vs stream model)**:
  - `TASK-0100` uses per-app stream model. This task uses pull-based ringbuffer. Document the relationship explicitly: ringbuffer can be used as the backing store for streams.

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- Audiod baseline: `TASK-0100` (per-app streams, volume/mute)
- Media v2.1a baseline: `TASK-0217` (session/graph based, 48k/10ms)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p audio_v0_9_host` green (new):

- mix correctness: sum of two sines @ −12 dB each equals expected peak; soft-clip bounded
- ringbuffer: write/read indices wrap; no data races; underrun increments
- level semantics: mute overrides level; unmute restores; linear→dB mapping documented
- file sink: write 200 ms; WAV header/length as expected; hash stable

## Touched paths (allowlist)

- `userspace/libs/audio-ringbuffer/` (new)
- `userspace/libs/audio-mixer/` (new; or extend existing)
- `userspace/libs/audio-level/` (new)
- `userspace/libs/audio-sink-file/` (new)
- `tests/audio_v0_9_host/` (new)
- `docs/audio/overview.md` (new, host-first sections)

## Plan (small PRs)

1. **Ringbuffer + mixer**
   - ringbuffer library
   - mixer library (or extend existing)
   - host tests

2. **Levels + file sink**
   - level library
   - file sink library
   - host tests

3. **Docs**
   - host-first docs

## Acceptance criteria (behavioral)

- Ringbuffer write/read indices wrap correctly; no data races; underrun increments.
- Mixer sum of two sines @ −12 dB each equals expected peak; soft-clip bounded.
- Level mute overrides level; unmute restores; linear→dB mapping documented.
- File sink WAV header/length as expected; hash stable.
