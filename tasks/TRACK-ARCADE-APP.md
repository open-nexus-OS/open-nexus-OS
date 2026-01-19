---
title: TRACK Arcade app (Breakout + Asteroids + Snake): first-party microgames proving NexusGame SDK (deterministic, low-latency, cap-first)
status: Draft
owner: @ui @runtime
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusGame SDK (foundation): tasks/TRACK-NEXUSGAME-SDK.md
  - NexusGfx SDK (render/present): tasks/TRACK-NEXUSGFX-SDK.md
  - NexusMedia SDK (audio): tasks/TRACK-NEXUSMEDIA-SDK.md
  - Zero-Copy App Platform (content/grants/exports): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Perf tracing + gates (deterministic): tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
  - Perf regression gates (scenes): tasks/TASK-0145-perf-v1c-deterministic-gates-scenes.md
  - Input bring-up direction (OS): tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md
---

## Goal (track-level)

Ship a first-party **Arcade** app that bundles three small games:

- **Breakout** (brick breaker),
- **Asteroids** (thrust + rotation + projectiles),
- **Snake-like** (grid movement, growth, hazards).

This is primarily a **platform proof**:

- NexusGame loop + input + timing is sufficient for real games,
- rendering/audio integration is stable and deterministic,
- latency and frame pacing can be measured and gated,
- and developers can copy the patterns for their own games.

## Product stance (why this exists)

- **Instant fun**: zero onboarding; tap a mode and play.
- **Deterministic by design**: record/replay input fixtures; stable goldens.
- **Low-latency showcase**: crisp touch/keyboard/controller feel (within OS constraints).
- **Open source reference**: a canonical, readable “how to build games on Nexus” example.

## Scope boundaries (anti-drift)

- No online multiplayer in v0.
- No microtransactions or ads.
- No giant asset pipeline requirements.
- No “licensed clone” or brand/level/asset copying from commercial arcade titles.

## Legal / licensing guardrails (hard requirements)

- **No trademarks**: do not use protected names/logos (e.g., “Atari”, “Asteroids”, “Breakout” branding).
- **No original art/audio/levels** from legacy titles. All assets must be:
  - authored in-repo, or
  - CC0 / CC-BY with attribution included in app credits.
- “Inspired-by” gameplay is fine; **trade dress** and signature level layouts are not.

## Authority model

- **Input** via input services (`inputd` etc.), not device nodes.
- **Audio** via `audiod` (single authority).
- **Rendering** via NexusGfx present/surface contracts.
- **Policy** remains centralized (no ambient network/device access).

## Keystone gates / dependencies

Arcade depends on these being real (at least host-first, then OS-wired):

- **NexusGame SDK**: stable game loop helper + deterministic input playback.
- **NexusGfx**: present integration + vsync-paced frame tick.
- **NexusMedia/audio**: simple SFX mixer path via `audiod` (bounded).
- **Perf gates**: frame-time traces and regression thresholds.

## Architecture (app shape)

One app bundle: `userspace/apps/arcade` with three internal modes:

- shared engine:
  - frame tick + fixed-step sim (configurable)
  - input mapping (touch/keyboard/controller)
  - pause/resume + quick restart
  - deterministic RNG policy (seeded; recorded in replays)
- per-mode modules:
  - breakout
  - asteroids
  - snake

Save data is minimal:

- highscores and last-selected mode (bounded),
- optional “practice” settings (difficulty, assist toggles).

## Determinism + testing contract (must-have)

Host-first tests must exist for each mode:

- deterministic simulation step with recorded input fixtures,
- stable score and end-state assertions,
- optional frame-buffer goldens (only after render contract is stable).

OS/QEMU proofs:

- “launch + play scripted demo + exit” markers per mode.

Markers (examples; stable strings):

- `arcade: ready`
- `arcade: mode start kind=breakout|asteroids|snake seed=...`
- `arcade: demo finished kind=... score=...`

## Phase map

### Phase 0 (host-first: real gameplay + tests)

- Arcade app skeleton + mode picker.
- Implement each mode with deterministic core loop.
- Host tests: input fixtures, score assertions, bounded runtime.
- Minimal audio stubs (can be mocked on host).

### Phase 1 (OS wiring: input/audio/render real)

- Input mapping on OS (touch + keyboard at minimum).
- Real rendering via NexusGfx present path.
- Real SFX via audiod.
- QEMU selftests: scripted demos produce markers.

### Phase 2 (polish + performance gates)

- Difficulty settings + accessibility toggles (reduced motion, high contrast).
- Perf traces and regression thresholds per mode.
- Optional controller mapping refinement and haptics hooks (if available).

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-ARCADE-000: Arcade app skeleton (mode picker + shared loop + markers)**
- **CAND-ARCADE-010: Breakout mode v0 (physics-lite, paddle control, bricks, scoring)**
- **CAND-ARCADE-020: Asteroids mode v0 (ship thrust/rotate, rocks split, bullets, scoring)**
- **CAND-ARCADE-030: Snake mode v0 (grid step, growth, hazards, scoring)**
- **CAND-ARCADE-040: Deterministic input fixtures + demo runner (host + OS)**
- **CAND-ARCADE-050: Perf gates for Arcade modes (frame-time thresholds)**

## Done criteria (track-level)

- All three modes playable with bounded settings.
- Host tests prove determinism (replay fixtures).
- OS/QEMU markers prove launch + scripted demo per mode.
- No forbidden crates added; no secrets in logs; no `unwrap/expect` on untrusted inputs.
