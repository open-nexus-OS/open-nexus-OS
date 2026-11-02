---
schema: 1
id: TRACK-REFERENCE-GAMES
title: "Reference Games (Arcade + Pinball + Puzzle): first-party games proving NexusGame SDK end-to-end"
status: draft
owner: ["@ui", "@runtime"]
created: 2026-01-19
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "NexusGame SDK (foundation)"
    target: tasks/TRACK-NEXUSGAME-SDK.md
  - label: "Arcade (microgames bundle)"
    target: tasks/TRACK-ARCADE-APP.md
  - label: "Pinball (physics + pacing showcase)"
    target: tasks/TRACK-PINBALL-APP.md
  - label: "Puzzle (UI-heavy + touch showcase)"
    target: tasks/TRACK-PUZZLE-APP.md
---

## Goal (track-level)

Ship three first-party games that are:

- fun on day 1,
- legally safe (no trademarks/assets copied),
- and rigorous platform proofs (determinism + perf gates + OS markers).

## The three games

- **Arcade**: a single app bundling three small modes (brick breaker, asteroids-like, snake-like).
- **Pinball**: “pro realtime” pacing + physics + audio.
- **Puzzle**: UI-heavy, touch-first, accessibility-forward.

## Shared contracts (hard requirements)

- **Deterministic input replays** for tests and perf regression gates.
- **Bounded assets** and explicit licensing (CC0/CC-BY or authored).
- **No ambient authority** (no network by default; no device nodes).
