---
title: TRACK Maps App (Organic Maps-class): fast offline/online maps + search + routing + 3D navigation (OSM-based, deterministic, policy-gated)
status: Draft
owner: @apps @ui @runtime
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - System Delegation / System Surfaces (pick-location + navigation intents): tasks/TRACK-SYSTEM-DELEGATION.md
  - Authority registry (names are binding): tasks/TRACK-AUTHORITY-NAMING.md
  - Keystone closure plan: tasks/TRACK-KEYSTONE-GATES.md
  - NexusGfx SDK (render/compute contracts): tasks/TRACK-NEXUSGFX-SDK.md
  - Zero-copy data plane (VMOs): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - NexusNet SDK (bounded networking): tasks/TRACK-NEXUSNET-SDK.md
  - Content/URIs (pathless) + picker + grants: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md, tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md, tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Content quotas/versions (offline packs): tasks/TASK-0232-content-v1_2a-host-content-quotas-versions-naming-nx-content.md
  - Packages / signed bundles (offline regions): tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md, tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Permissions/Privacy (runtime consent + indicators): tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - Sensor foundation (host-first): tasks/TASK-0258-sensor-bus-v0_9a-host-sensor-hal-accel-driver-deterministic.md
  - Location stack (this track's dependency): tasks/TRACK-LOCATION-STACK.md
---

## Goal (track-level)

Deliver a first-party **Maps** app comparable to Organic Maps:

- **fast map rendering** (smooth pan/zoom, low memory),
- **offline-first**: downloadable regions + offline search + offline routing,
- **online mode**: optional online tiles/search/routing with strict bounds,
- **turn-by-turn navigation** (voice optional; phase-gated),
- **3D navigation (final phase)**: 3D camera + extruded buildings/terrain-style cues where available,
- capability-first security and deterministic proofs (host-first; OS/QEMU markers only after real behavior).

Data source stance:

- OSM-based datasets (OpenStreetMap-derived), with signed offline pack distribution.

## Non-goals (avoid drift)

- Not a “full GIS workstation”.
- Not a proprietary bridge to closed map providers.
- Not a “webview maps app” as the primary path; UI must be first-party and deterministic.

## Authority model (must match registry)

Maps consumes canonical authorities:

- `locationd`: location authority (position/heading/speed) (see `tasks/TRACK-LOCATION-STACK.md`)
- `policyd`: permission decisions (location/network/pack install)
- `logd`: audit sink (location access decisions, downloads; no secrets)
- `windowd`: UI/present

Maps must not implement location policy or device access itself.

## System Delegation integration (system surfaces)

Maps should expose system delegation surfaces so other apps don’t re-implement mapping UX:
- `maps.pick_location`: one-shot pick (returns a single location object; no live tracking).
- `maps.navigate_to`: open navigation UI with a destination (user-mediated; policy-gated).

## Capability gates (directional, stable strings)

- `location.read` (foreground location)
- `location.background` (background updates; likely system-only by default)
- `location.mock` (test/fixture injection; system-only)
- `network.http.request` (online tiles/search/routing where enabled; bounded)
- `content.read` / `content.write` via scoped grants (import/export GPX etc.)

## Keystone gates / blockers

### Gate A — Durable `/state` substrate (offline packs + caches)

Reference: `tasks/TRACK-KEYSTONE-GATES.md` (Gate 3).

Needed for: region packs storage, indexes, caches, last-known location, bookmarks.

### Gate B — Content broker + scoped grants (pathless)

References: `TASK-0081`, `TASK-0083`, `TASK-0084`, `TASK-0232`.

Needed for: pack install/open, GPX import/export, safe file access without ambient paths.

### Gate C — Bounded networking (online mode)

Reference: `tasks/TRACK-NEXUSNET-SDK.md`.

Needed for: online tile download, online geocoding, online routing with bounded retries/timeouts.

### Gate D — Rendering substrate

References:

- `tasks/TRACK-NEXUSGFX-SDK.md`
- `tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md`
- `tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md`

Needed for: deterministic map rendering goldens and later 3D navigation.

### Gate E — Location stack exists and is policy-gated

Reference: `tasks/TRACK-LOCATION-STACK.md`.

Needed for: navigation, “center on me”, recording tracks, speed/heading.

## Phase map (what “done” means by phase)

### Phase 0 — Online-first map viewer (fastest path to a usable app)

Goal: ship a responsive map quickly, without blocking on offline routing.

- online tiles (bounded HTTP) + simple cache
- pan/zoom/rotate, place pin, copy/share location
- online search (bounded; strict parser limits)
- **host-first proofs** for networking bounds + rendering determinism for fixtures

### Phase 1 — Offline regions (packs) + offline search v1

- downloadable regions as signed packs (bundle/content artifacts)
- offline search index for POI/place names within installed regions
- quota-aware cache + deterministic eviction rules

### Phase 2 — Routing v1 (offline + online fallback)

- offline routing within installed regions (bounded compute; deterministic fixtures)
- online routing as optional fallback (bounded; policy-gated)
- turn-by-turn instructions + reroute semantics (bounded; deterministic)

### Phase 3 — Navigation UX (turn-by-turn polish)

- lane guidance / better instruction rendering (bounded)
- voice prompts optional and policy-gated
- background nav mode (likely privileged; explicit)

### Phase 4 — 3D navigation (final phase)

Goal: 3D camera and “3D cues” similar to Organic Maps:

- 3D camera (tilt/rotate) + smooth transitions
- extruded buildings where data exists
- terrain-like cues where feasible (or stylized elevation shading)
- strict perf gates (host-first); no “perf ok” without real perf stack

This phase is intentionally gated on NexusGfx maturity and (eventual) GPU backends.

## Candidate subtasks (to be extracted into TASK-XXXX)

- **CAND-MAPS-000: Map renderer v0 (2D tiles + labels) + deterministic goldens**
- **CAND-MAPS-010: Online tiles v0 (bounded HTTP + cache) + host tests**
- **CAND-MAPS-020: Offline region packs v0 (signed bundles + quotas + install/uninstall)**
- **CAND-MAPS-030: Offline search v0 (index format + bounded query)**
- **CAND-MAPS-040: Routing v0 (offline fixtures, deterministic turn list)**
- **CAND-MAPS-050: Navigation UX v0 (turn-by-turn, reroute semantics, bounded)**
- **CAND-MAPS-060: 3D navigation v0 (camera + extrusions) + perf gates**

## Extraction rules

Candidates become real tasks only when they:

- define explicit bounds (bytes/time/region sizes/query limits),
- provide deterministic host proofs (goldens/fixtures),
- keep authority boundaries (location via `locationd`, policy via `policyd`),
- and do not add unreviewed dependency stacks that violate OS build hygiene.
