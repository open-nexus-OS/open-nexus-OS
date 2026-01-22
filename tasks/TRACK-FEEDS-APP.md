---
title: TRACK Feeds app (RSS/Atom): offline reading + share to Notes (reference app for NexusNet + ZeroCopy)
status: Draft
owner: @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusNet SDK (HTTP/providers): tasks/TRACK-NEXUSNET-SDK.md
  - Zero-Copy App Platform (content/grants/share): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Notes (share target): tasks/TRACK-NOTES-APP.md
  - Share v2 / Intents (registry + dispatch): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
---

## Goal (track-level)

Deliver a first-party **Feeds** app that proves:

- bounded HTTP fetch + parsing of untrusted feeds,
- offline article caching and “read later” semantics,
- share integration to Notes and other apps via Intents,
- deterministic host-first tests with fixtures.

## Scope boundaries (anti-drift)

- No social network features in v0.
- No unbounded background polling; refresh is user-driven or bounded schedule.

## Product scope (v0)

- subscribe to feeds (RSS/Atom)
- list view (new/unread, folders optional)
- reader view (sanitized HTML subset; images opt-in)
- offline cache (bounded)
- share:
  - “Share to Notes” (rich content excerpt)
  - “Open in Browser”

## Security invariants (feeds are hostile input)

- strict parse budgets (max bytes, max items, max nesting)
- sanitize HTML (no scripts; no external loads by default)
- stable error model; never panic on malformed feeds

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-FEEDS-000: Feed parser + sanitizer v0 (fixtures; bounds; property tests)**
- **CAND-FEEDS-010: Feeds app UI v0 (subscribe/read/cache; share)**
- **CAND-FEEDS-020: Refresh policy v0 (bounded schedules; audit-friendly)**
