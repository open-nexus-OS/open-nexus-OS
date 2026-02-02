---
title: TASK-0103 UI v17a: permissions (permsd) + privacy indicators (privacyd) with persistent grants + markers
status: Draft
owner: @security
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy as Code (gates): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (defaults): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
---

## Context

UI v17 adds camera/mic/screen capture. The security boundary must be built first:

- a permissions broker (`permsd`) that records grants (TTL + persist),
- a privacy indicator service (`privacyd`) that exposes “camera/mic/screen in use” to SystemUI.

Virtual camera/mic, recorder, and apps build on top of this.

## Goal

Deliver:

1. `permsd` service:
   - capabilities: camera, microphone, screen
   - `check/request/revoke/list`
   - NOTE (authority): `permsd` is a runtime consent broker. Long-term, **policyd remains the single authority**
     for grants (see `TASK-0136` + `TASK-0167/0168`). Any `permsd` persistence is bring-up only and must not become
     a parallel “truth source”.
   - if persistence exists in this task, it is limited to a small, explicitly-scoped cache (e.g. “remember for session”),
     and must be removable once policyd v1.1 is in place.
   - deterministic grant semantics and stable deny reasons
   - marker: `permsd: ready`
2. `privacyd` service:
   - tracks active indicators with timestamps and owning appId(s)
   - streams events to SystemUI (later tasks consume)
   - supports system-only kill switches for indicators’ corresponding categories (camera/microphone/screen.capture)
   - markers:
     - `privacyd: ready`
     - `privacy: indicator on (cap=... app=...)`
     - `privacy: indicator off (cap=...)`
3. Host tests proving persistence/TTL and indicator toggling deterministically.

## Non-Goals

- Kernel changes.
- UI prompts/overlays (later tasks).
- A full user identity model (use appId/subject strings).
 - Replacing policyd as the capability authority; permsd is runtime consent and should be combined with `policyd` gates (see `TASK-0136`).
 - Location indicators/permissions (tracked separately once a location service contract is finalized; do not overload v17a).

## Constraints / invariants

- Default deny: no cap is granted unless a grant exists.
- Deterministic persistence and TTL behavior (injectable clock in tests).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v17a_host/`:

- request grant (persist and TTL) then check returns granted
- TTL expiry revokes deterministically
- persisted grants survive restart (host simulated)
- revoke removes grant
- privacy indicator on/off events are emitted deterministically

## Touched paths (allowlist)

- `source/services/permsd/` (new)
- `source/services/privacyd/` (new)
- `tests/ui_v17a_host/`
- `docs/privacy/overview.md` (new)

## Plan (small PRs)

1. permsd core + persistence + tests + markers
2. privacyd core + event stream + tests + markers
3. docs
