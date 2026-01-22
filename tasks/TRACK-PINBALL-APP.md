---
title: TRACK Pinball app: physics + audio + high-FPS pacing reference game (deterministic, NexusGame SDK showcase)
status: Draft
owner: @ui @runtime
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Reference Games umbrella: tasks/TRACK-REFERENCE-GAMES.md
  - NexusGame SDK (foundation): tasks/TRACK-NEXUSGAME-SDK.md
  - NexusGfx SDK (render/present): tasks/TRACK-NEXUSGFX-SDK.md
  - NexusMedia SDK (audio): tasks/TRACK-NEXUSMEDIA-SDK.md
  - Perf tracing + gates (deterministic): tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
---

## Goal (track-level)

Ship a first-party **Pinball** game that proves “pro realtime” readiness:

- stable high-FPS frame pacing (as available),
- physics step determinism (replayable),
- tight input response,
- layered audio SFX and music routing via `audiod`,
- performance tracing + regression gates.

## Scope boundaries (anti-drift)

- No licensed tables or branded themes.
- No online features in v0.
- Keep physics engine minimal; correctness and determinism first.

## Product scope (v0)

- single table, single-ball mode
- flippers, bumpers, ramps, scoring
- pause/resume, quick restart
- basic audio pack (CC0/CC-BY)

## Determinism + proofs

- fixed-step physics sim with deterministic integration policy
- input replay fixtures drive a scripted “demo ball” run:
  - stable final score
  - stable end-state
- perf traces captured for key scenes (launch, multiball later)

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-PINBALL-000: Pinball core loop v0 (table + scoring + markers)**
- **CAND-PINBALL-010: Deterministic physics v0 (fixtures + replay proofs)**
- **CAND-PINBALL-020: Audio pack + routing v0 (audiod; bounded)**
- **CAND-PINBALL-030: Perf gates v0 (frame-time thresholds)**
