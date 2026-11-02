---
schema: 1
id: TRACK-PUZZLE-APP
title: "Puzzle game app: touch-first, UI-heavy, accessibility-forward reference game (deterministic)"
status: draft
owner: ["@ui"]
created: 2026-01-19
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "Reference Games umbrella"
    target: tasks/TRACK-REFERENCE-GAMES.md
  - label: "NexusGame SDK (foundation)"
    target: tasks/TRACK-NEXUSGAME-SDK.md
  - label: "NexusGfx SDK (render/present)"
    target: tasks/TRACK-NEXUSGFX-SDK.md
  - label: "Accessibility wiring (Settings + app hardening)"
    target: tasks/TASK-0118-ui-v20e-accessibility-settings-app-wiring-os-proofs.md
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
