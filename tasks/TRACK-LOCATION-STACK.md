---
title: TRACK Location Stack (GPS/GNSS + fusion): locationd authority + gnssd driver, consent-gated, deterministic fixtures
status: Draft
owner: @runtime @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority registry (names are binding): tasks/TRACK-AUTHORITY-NAMING.md
  - Keystone closure plan: tasks/TRACK-KEYSTONE-GATES.md
  - Device MMIO access model (userspace drivers): tasks/TASK-0010-device-mmio-access-model.md
  - Sensor foundation (host-first): tasks/TASK-0258-sensor-bus-v0_9a-host-sensor-hal-accel-driver-deterministic.md
  - Permissions/Privacy (runtime consent + indicators): tasks/TASK-0103-ui-v17a-permissions-privacyd.md
---

## Goal (track-level)

Provide a coherent, testable location stack that enables:

- maps/navigation apps (offline + online),
- fitness tracking (optional),
- camera geotagging (optional),

while preserving OS invariants:

- capability-first security (no ambient location),
- deterministic proofs (fixtures + replay; no fake success),
- bounded resource usage.

## Authority model (binding)

### `locationd` (authority)

Single authority that exposes location to apps:

- position (lat/lon), accuracy
- heading/bearing (where supported)
- speed (where supported)
- time model + monotonic timestamps

`locationd` is where policy checks and auditing are centralized (in cooperation with `policyd`/`permsd`).

### `gnssd` (device-facing driver service)

Device-class service that talks to GNSS hardware (or fixtures in bring-up) and provides bounded raw fixes to `locationd`.

Rationale: keep device protocols and parsing isolated; `locationd` stays stable.

## Capability names (directional, stable strings)

- `location.read` (foreground location)
- `location.background` (background updates; privileged by default)
- `location.mock` (fixture injection; system-only)

## Consent + privacy stance

Location is sensitive:

- require policy allow + runtime consent where applicable (`permsd` + `privacyd`)
- deterministic deny reasons (fail closed)
- indicator policy is explicit (when location is actively used)

## Gates / blockers

### Gate 1 — IPC + cap transfer (Keystone Gate 1)

Reference: `tasks/TRACK-KEYSTONE-GATES.md`.

Needed for: service boundaries (`locationd` clients; `gnssd` → `locationd`).

### Gate 2 — Userspace device access (MMIO) for real GNSS

Reference: `tasks/TASK-0010-device-mmio-access-model.md`.

Needed when GNSS is MMIO-backed on target hardware. Bring-up can start with fixtures without MMIO.

### Gate 3 — Sensor HAL reuse (avoid drift)

Reference: `tasks/TASK-0258-sensor-bus-v0_9a-host-sensor-hal-accel-driver-deterministic.md`.

Location fusion should reuse shared time/units conventions and deterministic fixture machinery.

## Phase map (what “done” means by phase)

### Phase 0 — Host-first location fixtures + API surface

- define `LocationFix` types + bounded errors
- fixture/replay backend produces deterministic fix streams
- host tests: “fixture → stable route of positions” (hash over first N fixes)

### Phase 1 — OS wiring + consent gates

- `locationd` exists as a service; policy/consent enforced
- QEMU markers only after real allow/deny and stream subscription behavior

### Phase 2 — GNSS driver integration (real device targets)

- `gnssd` driver service talks to GNSS hardware (or virt device)
- bounded parsing, reset/recovery

### Phase 3 — Fusion + navigation-grade signals

- fuse GNSS + sensors for heading/speed stability (bounded)
- deterministic replay tests validate fusion outputs

## Candidate subtasks (to be extracted into TASK-XXXX)

- **CAND-LOC-000: Location API v0 (types + error model) + fixture replay tests**
- **CAND-LOC-010: locationd v0 (policy/consent gating + subscriptions)**
- **CAND-LOC-020: gnssd v0 (bounded fix parsing + reset/recovery)**
- **CAND-LOC-030: Fusion v0 (heading/speed smoothing) + deterministic replay**

## Extraction rules

Candidates become real tasks only when they:

- define explicit bounds (fix rate, buffer sizes, parse limits),
- include negative tests (`test_reject_*`) for denied access and malformed fix payloads,
- keep authority boundaries (apps talk to `locationd`, not `gnssd`),
- and keep “mock location” strictly privileged.
