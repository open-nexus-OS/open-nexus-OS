---
schema: 1
id: TRACK-WEATHER-APP
title: "Weather app: location-gated forecasts + cache-first UX (reference app for NexusNet + Location)"
status: draft
owner: ["@ui"]
created: 2026-01-19
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "Location stack (locationd + consent)"
    target: tasks/TRACK-LOCATION-STACK.md
  - label: "NexusNet SDK (HTTP/providers)"
    target: tasks/TRACK-NEXUSNET-SDK.md
  - label: "Zero-Copy App Platform (caches/export)"
    target: tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
---

## Goal (track-level)

Ship a first-party **Weather** app that proves:

- location consent and indicators are respected,
- network access is capability-gated and bounded,
- a cache-first UX works offline (last known forecast snapshot),
- provider integration is pluggable (no vendor lock-in).

## Scope boundaries (anti-drift)

- No background tracking in v0 (foreground location only by default).
- No ads, no tracking, no hidden network beacons.

## Product scope (v0)

- current conditions + hourly + 7-day forecast
- multiple saved locations (manual search)
- optional “use my location” (permission-gated)
- offline snapshot view (“last updated …”)

## Provider model

- A default provider can exist, but the surface is shaped for installable providers later.
- All HTTP requests are bounded (timeouts, max bytes).

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-WEATHER-000: Weather provider contract v0 (bounded HTTP + parse limits)**
- **CAND-WEATHER-010: Weather app UI v0 (cache-first; multi-location)**
- **CAND-WEATHER-020: Location consent wiring v0 (foreground only; indicators)**
