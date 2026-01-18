---
title: TRACK NexusMedia SDK (audio/video/image): contracts + phased roadmap (deterministic, capability-first, zero-copy)
status: Living
owner: @media @runtime
created: 2026-01-18
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusGfx SDK track (render/compute): tasks/TRACK-NEXUSGFX-SDK.md
  - NexusNet SDK track (cloud + DSoftBus): tasks/TRACK-NEXUSNET-SDK.md
  - Drivers & accelerators foundations (GPU/VPU/Audio/Camera): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Zero-copy data plane (VMOs): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers (soft real-time spine): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Deterministic parallelism policy: tasks/TASK-0276-parallelism-v1-deterministic-threadpools-policy-contract.md
  - Policy as Code (future unification): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - App capability matrix (future): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Media decoders v1 (host-first): tasks/TASK-0099-ui-v16a-media-decoders.md
  - Audio core v0.9a (host-first): tasks/TASK-0254-audio-v0_9a-host-mixer-ringbuffer-levels-deterministic.md
  - Audio OS/QEMU integration v0.9b: tasks/TASK-0255-audio-v0_9b-os-audiod-i2sd-codecd-mediasession-hooks-selftests.md
---

## Goal (track-level)

Deliver a first-party, system-optimized **NexusMedia SDK** that enables:

- **Audio**: low-jitter playback/synthesis/mixing with explicit buffer ownership and policy gates.
- **Video**: deterministic decode/compose primitives for bring-up, with a clean path to VPU/GPU backends.
- **Images/Photos**: safe, bounded decode + transforms + export with deterministic proofs.

While preserving Open Nexus OS invariants:

- **capability-first security** (no ambient “device/global media” authority),
- **zero-copy data plane** (VMO/filebuffer end-to-end for bulk),
- **soft real-time** pacing (deadlines/QoS/backpressure; no busy-wait),
- **deterministic tooling** (host-first goldens/checksums; stable markers only after real behavior),
- **small TCB** (validation in userland; kernel stays minimal).

## Scope boundaries (anti-drift)

- **This track defines contracts and roadmap**, not a single implementation task.
- **Do not build a POSIX/FFmpeg/GStreamer clone** as the “core architecture”.
- **Do not create parallel authorities** (e.g. a second mixer service). Authority remains explicit per domain.

## Shared primitives (cross-domain contracts)

These are the “shared primitives” that make later API evolution cheap:

- **Buffers/images**: VMO/filebuffer descriptors, slices, budgets, RO-sealing conventions.
- **Time/sync**: timeline fences, waitsets, deadlines; deterministic tick scheduling.
- **Format descriptors**: stable `AudioFormat` / `PixelFormat` / `CodecId` / `ColorSpace` identifiers.
- **Error model**: stable error codes + bounded “why” diagnostics (no stringly-typed semantics).
- **Policy hooks**: capability names + audit event schemas (policyd remains the authority).

This SDK must consume the same cross-cutting contracts used by NexusGfx (see `tasks/TRACK-NEXUSGFX-SDK.md`).

## Capability names (v0 catalog; stable strings)

These are **string identifiers** used by `policyd`/policy files and enforcement adapters. They are intentionally:

- lowercase,
- dot-separated,
- stable over time (additive expansion preferred),
- not tied to a specific backend (CPU vs VPU vs remote).

### Audio

- `audio` (umbrella; avoid using directly except for admin/system allowlists)
- `audio.stream.open`
- `audio.stream.write`
- `audio.stream.control` (pause/resume/close)
- `audio.level.read`
- `audio.level.set`
- `audio.tone.play` (bring-up/demo only; may be system-only)
- `audio.record` (if/when capture is supported; may be gated by runtime consent)

### Video

- `video.decode` (decode a supported format into bounded frames)
- `video.playback` (start/stop a playback pipeline; includes frame pacing)
- `video.export` (encode/export; may be gated and quota-limited)
- `video.capture` (screen/camera capture pipelines; typically requires runtime consent)

### Images / Photos

- `image.decode` (bounded)
- `image.transform` (resize/rotate/filter; bounded)
- `image.export` (bounded)
- `image.print` (if print pipeline exists; may be system-only depending on design)

### Capture devices (virtual or real)

- `camera` (runtime consent expected; see perms/privacy tasks)
- `microphone` (runtime consent expected)

### Notes (policy + consent layering)

- Sensitive caps (camera/microphone/screen capture) typically require **both**:
  - `policyd` capability allow (static policy / grants), and
  - runtime consent broker (`permsd`) + indicators (`privacyd`) where applicable.

## Authority model (who owns what)

Keep “authority” explicit to avoid security and lifecycle drift:

- **Audio mixing/routing authority**: `audiod` is the single authority (service). SDK provides client types and helpers.
- **Decode/transform**:
  - v0/v1 (bring-up): pure Rust, host-first libraries are acceptable where bounded/deterministic.
  - v2+ (pro): heavy decode/encode may move behind dedicated services (VPU/GPU backends), still accessed via SDK.
- **Capture authority**: camerad/micd (services) own device-facing capture; SDK provides client APIs; permissions via policyd/permsd.

## Phase map (what “done” means by phase)

- **Phase 0 (host-first primitives + proofs)**
  - Deterministic audio core libraries exist (ringbuffer/mixer/levels/file sink).
  - Deterministic decode libraries exist for a minimal, QEMU-safe set (WAV/Vorbis + GIF/APNG/MJPEG).
  - SDK types are defined (formats, buffers, time model, error codes), even if some backends are stubbed.

- **Phase 1 (OS/QEMU wiring, still honest)**
  - `audiod` service is real, policy-gated, with deterministic selftests/markers.
  - Minimal capture stubs (virtual camera/mic) exist with permission checks and privacy indicators.
  - Apps (music/video/images) demonstrate the SDK path end-to-end (host proofs + OS markers where valid).

- **Phase 2 (pro workloads)**
  - Timelines + composition primitives for video editing (decode/compose hooks) with bounded scheduling.
  - Hardware backends (VPU/GPU) can implement the same contracts behind the SDK via DriverKit/device brokers.
  - Remote/distributed media flows (optional) layer on DSoftBus with explicit bounds and policy gates.

## Candidate subtasks (to be extracted into real tasks)

This section is intentionally a map of “what we’ll extract”, not a backlog to implement all at once.
When extracting, keep tasks small and host-first.

### Audio

- **CAND-MEDIA-010: NexusAudio SDK v0 (types + stream client ergonomics)**
  - stable `AudioFormat`, `StreamHandle`, `Level`, `Underrun` model
  - maps cleanly onto `audiod` IDL/service surface
  - proof: deterministic host tests; no OS claims

### Video

- **CAND-MEDIA-020: NexusVideo SDK v0 (frame types + decode iterator contract)**
  - stable `VideoFrame` (BGRA) + `FrameClock` model
  - decode remains bounded and deterministic for fixtures

- **CAND-MEDIA-021: Video timeline substrate v0 (tracks/clips/edits)**
  - *composition contract only* (no heavy codec surface yet)
  - proof: deterministic “edit script → frame hash sequence” on host

### Images / Photos

- **CAND-MEDIA-030: NexusImage SDK v0 (decode/transform/export)**
  - bounded decode; deterministic transforms; stable export semantics

## Extraction rules (how candidates become real tasks)

A candidate becomes a real `TASK-XXXX` only when:

- it is implementable under current gates (or explicitly creates prerequisites),
- it has **proof** (deterministic host tests and/or QEMU markers where valid),
- it declares what is *stubbed* (explicitly) vs. what is real,
- it names the authority boundary (service vs library vs SDK) and does not create a competing authority.
