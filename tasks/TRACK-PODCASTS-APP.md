---
title: TRACK Podcasts app: offline downloads + queue + provider-ready (reference app for NexusMedia + NexusNet)
status: Draft
owner: @media @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - System Delegation / System Surfaces (share/open-with, send via chat): tasks/TRACK-SYSTEM-DELEGATION.md
  - Media Apps umbrella (shared UX + sessions): tasks/TRACK-MEDIA-APPS.md
  - NexusMedia SDK (decode/playback): tasks/TRACK-NEXUSMEDIA-SDK.md
  - NexusNet SDK (HTTP/providers): tasks/TRACK-NEXUSNET-SDK.md
  - Media sessions + SystemUI controls umbrella: tasks/TASK-0101-ui-v16c-media-sessions-systemui-controls.md
  - Media UX v2.1 (focus/ducking/per-app volume): tasks/TASK-0218-media-v2_1b-os-focus-ducking-miniplayer-nx-media.md
  - Downloads helper (saveAs): tasks/TASK-0112-ui-v19b-contentd-saveas-downloads.md
  - Files app integration: tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
---

## Goal (track-level)

Deliver a first-party **Podcasts** app that is daily-usable and proves:

- long-form audio playback via media sessions (`mediasessd`),
- offline downloads with bounded storage and resume,
- queue + playback speed + skip controls,
- and provider-ready discovery without hard-wiring a single vendor.

## Scope boundaries (anti-drift)

- No “Spotify replacement” in v0.
- No ambient network access; all fetches are capability-gated and bounded.
- No DRM formats in v0.

## Product scope (v0)

- subscribe to shows (via RSS feeds; URL paste + search later)
- show detail page (episodes list)
- episode playback:
  - play/pause/seek
  - skip forward/back
  - speed (0.5x–3.0x bounded set)
  - sleep timer (optional)
- offline:
  - download episode (to `state:/Downloads/` or app-managed cache)
  - resume partial downloads (bounded)
  - auto-delete policy (keep last N / keep played) optional
- library:
  - “New”, “In Progress”, “Downloaded”, “Saved”

## Architecture stance

- RSS/Atom parsing is treated as hostile input (strict budgets).
- Audio is played through NexusMedia and exposed via media sessions for system controls.
- Downloads use `contentd.saveAs`-style helpers; no direct filesystem bypass.

## System Delegation integration

Podcasts should reuse system delegation for:
- share episode link/metadata to Notes/Chat (Intents/Chooser),
- open downloaded files via Open With defaults (mimed + picker), not custom handler logic.

## Provider model (future-friendly)

v0 can be RSS-first. Later, installable “catalog providers” can exist (search + discovery),
but playback remains local/standard formats.

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-POD-000: Podcast RSS parser v0 (bounds + fixtures)**
- **CAND-POD-010: Podcasts app UI v0 (library + show detail + episode list)**
- **CAND-POD-020: Playback v0 (queue + speed + skip; mediasessd integration)**
- **CAND-POD-030: Downloads v0 (bounded; resume; auto-delete policy)**
