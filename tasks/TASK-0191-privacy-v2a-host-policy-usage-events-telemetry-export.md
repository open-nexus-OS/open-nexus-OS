---
title: TASK-0191 Privacy v2a (host-first): structured policy usage events + deterministic aggregation (timeline/stats) + NDAP export + tests
status: Draft
owner: @security
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Policy v1.1 core semantics + audit direction: tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - Privacy Dashboard v1.1 (OS UI owner): tasks/TASK-0168-policy-v1_1-os-runtime-prompts-privacy-dashboard-cli.md
  - Policy audit sink direction (logd): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
---

## Context

Policy v1.1 already includes audit emission direction and a v1.1 Privacy Dashboard UI surface.
Privacy v2 upgrades the dashboard with:

- a capability-usage timeline,
- per-app/per-cap usage stats (allow/deny counts + last used),
- deterministic export of a time window,
- and clear retention/quotas.

Important: the repo avoids introducing a separate `auditd` authority when `logd` is the chosen sink.
This task is host-first and defines the **event schema + deterministic aggregation + export format**.
OS services and UI wiring are handled in `TASK-0192`.

## Goal

Deliver:

1. Structured “policy usage” event model:
   - `Use { tsNs, appId, cap, scope, decision, mode }`
   - emitted for:
     - `require()` decisions (allow/deny/expired)
     - grant lifecycle (grant/revoke/purgeExpired) as events with deterministic decision strings
   - deterministic timestamp source in tests (injected monotonic clock)
2. Aggregation library (`userspace/libs/privacy-telemetry` or similar):
   - read-through ingest from an event stream (host tests use in-memory JSONL)
   - produce:
     - `window(since,until,filters,limit) -> Summary { items, stats }`
     - `topCaps(appId, limit)`
   - stable ordering rules:
     - timeline sorted by `(tsNs asc, appId asc, cap asc, scope asc)`
     - stats sorted by `(allow+deny desc, lastNs desc, cap asc)`
3. Deterministic export format: **NDAP v1**
   - an “audit pack” containing:
     - a manifest (stable JSON with sorted keys)
     - a `cap_use.jsonl` slice for the requested window
   - packaging must be deterministic:
     - stable file order
     - zeroed mtimes
     - stable compression settings (or none)
   - NOTE: this is an export artifact, not an OS bundle contract; JSON is acceptable but must be deterministic
4. Host tests (`tests/privacy_v2_host/`):
   - ingest/order determinism
   - window queries and filters
   - topCaps determinism
   - NDAP export byte-stability under fixed inputs
   - quota/retention trimming behavior (deterministic)

## Non-Goals

- Kernel changes.
- Shipping a new standalone `auditd` authority.
- UI/Settings implementation (v2b).

## Constraints / invariants (hard requirements)

- Deterministic ordering and stable output bytes for NDAP.
- Bounded memory and bounded event tables.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (audit sink availability)**:
  - OS-facing “real timeline from real decisions” is gated on a real audit sink (`logd`) and `/state` persistence.
  - This task stays host-first and must not claim OS availability.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p privacy_v2_host -- --nocapture`

## Touched paths (allowlist)

- `userspace/libs/privacy-telemetry/` (new)
- `tests/privacy_v2_host/` (new)
- `docs/privacy/ndap.md` (optional; can land in v2b docs)

## Plan (small PRs)

1. Define event schema/types + deterministic ordering rules + host tests
2. Add aggregation queries (window/topCaps) + host tests
3. Add NDAP export writer + host tests (byte-stable)

## Acceptance criteria (behavioral)

- Host tests deterministically prove usage ingest, aggregation, and export stability.

