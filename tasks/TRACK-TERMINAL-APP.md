---
title: TRACK Terminal app: tabs + PTY + safe clipboard (power-user reference app; host-first)
status: Draft
owner: @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Zero-Copy App Platform (clipboard/share/save): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Files app (Open With / save/export): tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - Accessibility wiring (Settings + app hardening): tasks/TASK-0118-ui-v20e-accessibility-settings-app-wiring-os-proofs.md
---

## Goal (track-level)

Deliver a first-party **Terminal** app that proves the power-user story without breaking OS invariants:

- multi-tab terminal sessions,
- deterministic UX for core interactions,
- safe clipboard handling,
- and a clean path to remote shells later (SSH) without baking it into v0.

## Scope boundaries (anti-drift)

- v0 is local-only (no SSH in v0).
- No “full Linux compatibility layer” implied.
- No ambient filesystem access; terminal sessions should respect app sandboxing policies.

## Product scope (v0)

- tabs (create/close/rename)
- scrollback (bounded)
- copy/paste (policy/bounds)
- basic shortcuts (clear, search in scrollback optional)
- “Export transcript” (bounded; share/save)

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-TERM-000: Terminal UI v0 (tabs + scrollback + copy/paste; host tests)**
- **CAND-TERM-010: PTY/session wiring v0 (OS integration; bounded)**
- **CAND-TERM-020: Export/share transcripts v0 (content:// + grants)**
