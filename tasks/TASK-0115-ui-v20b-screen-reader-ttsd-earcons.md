---
title: TASK-0115 UI v20b: screen reader (readerd) + TTS stub (ttsd) + earcons via audiod + prefs
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - A11y tree/actions baseline: tasks/TASK-0114-ui-v20a-a11yd-tree-actions-focusnav.md
  - Audiod mixer: tasks/TASK-0100-ui-v16b-audiod-mixer.md
  - Prefs store: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Policy as Code (reader enable): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
---

## Context

Once we have a11y events, we can build a screen reader. For v1, TTS is a stub:

- concatenative PCM/earcons from packaged WAV snippets,
- output via `audiod` (if available),
- deterministic behavior for tests and QEMU markers.

## Goal

Deliver:

1. `readerd` service:
   - subscribes to `a11yd.onEvent`
   - announces focus/activation/value changes
   - emits markers:
     - `readerd: ready`
     - `reader: speak "<text>"`
     - `reader: earcon <name>`
2. `ttsd` stub:
   - loads small PCM syllable/phoneme set from `pkg://tts/` (or compiled-in fixtures)
   - stitches PCM deterministically; fallback to letter-spell
   - outputs PCM stream to `audiod`
   - markers:
     - `ttsd: ready`
3. Prefs:
   - speech rate/pitch/volume and verbosity (brief/verbose)
4. Host tests:
   - focus event triggers speak + audiod VU rises (mocked audiod)

## Non-Goals

- Kernel changes.
- Real TTS engine.

## Constraints / invariants

- Deterministic output selection for a given input string (stub rules documented).
- Bounded queues and PCM buffers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v20b_host/`:

- send focus event → `reader: speak` occurs and PCM stream written to mocked audiod
- earcons emitted deterministically for focus/toggle

### Proof (OS/QEMU) — gated

UART markers:

- `readerd: ready`
- `ttsd: ready`
- `SELFTEST: ui v20 reader ok` (owned by v20e)

## Touched paths (allowlist)

- `source/services/readerd/` (new)
- `source/services/ttsd/` (new or combined)
- `tests/ui_v20b_host/`
- `docs/a11y/screen-reader.md` (new)

## Plan (small PRs)

1. ttsd stub + audiod stream writing + markers
2. readerd focus subscription + speak/earcon rules + markers
3. prefs wiring + tests + docs
