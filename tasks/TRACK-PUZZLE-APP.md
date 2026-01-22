---
title: TRACK Puzzle game app: touch-first, UI-heavy, accessibility-forward reference game (deterministic)
status: Draft
owner: @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Reference Games umbrella: tasks/TRACK-REFERENCE-GAMES.md
  - NexusGame SDK (foundation): tasks/TRACK-NEXUSGAME-SDK.md
  - NexusGfx SDK (render/present): tasks/TRACK-NEXUSGFX-SDK.md
  - Accessibility wiring (Settings + app hardening): tasks/TASK-0118-ui-v20e-accessibility-settings-app-wiring-os-proofs.md
---

## Goal (track-level)

Ship a first-party **Puzzle** game that proves:

- touch-first interaction patterns,
- UI-heavy scenes with deterministic animation,
- strong accessibility (screen reader labels, focus order, reduced motion),
- and save/resume semantics without data loss.

## Scope boundaries (anti-drift)

- No network in v0.
- Keep puzzle rules simple; determinism and UX polish are the proof.

## Product scope (v0)

Pick one puzzle mechanic (examples):

- “match / merge” grid (Threes/2048-like but original),
- or “loop/pipe” connect puzzle,
- or a minimal “word tile” puzzle.

Required:

- levels / sessions are bounded
- undo/redo optional, but deterministic if implemented
- save/resume last session

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-PUZZLE-000: Puzzle core v0 (rules + deterministic tests)**
- **CAND-PUZZLE-010: Puzzle UI v0 (touch + animations; a11y wiring)**
- **CAND-PUZZLE-020: Save/resume v0 (bounded; crash-safe)**
