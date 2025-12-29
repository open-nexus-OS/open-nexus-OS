---
title: TASK-0167 Policy v1.1 (host-first): scoped grants + once/session/persistent modes + expiry + enumerate/revoke + deterministic audit emission
status: Draft
owner: @runtime
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Policy v1 capability matrix baseline: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - permsd baseline: tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - Observability/audit sink direction (logd): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - UI privacy dashboard (v1.1): tasks/TASK-0168-policy-v1_1-os-runtime-prompts-privacy-dashboard-cli.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0136` establishes a capability catalog + per-app grants + foreground guards + adapters + audit events.
Policy v1.1 upgrades the grant model to be:

- scoped (resource scoping, not just boolean caps),
- time-bounded (expiry),
- mode-based (once/session/persistent),
- enumerable and revocable via a stable API.

This task is host-first (deterministic semantics and tests). OS/QEMU wiring and UI live in `TASK-0168`.

Audit direction:

- `TASK-0136` explicitly avoids introducing a new `auditd` authority; audit sink should align with `logd` when available,
  with a deterministic UART-marker fallback during bring-up.

## Goal

Deliver:

1. Policyd grant model v1.1:
   - `Grant { appId, cap, scope, mode, issuedNs, expiresNs, rationale }`
   - `Mode`: once | session | persistent
   - `Scope.resource` is a **resource URI pattern**; wildcards are denied by default (dev-mode only)
2. Fast-path require semantics:
   - `require(query)` returns allow/deny deterministically
   - consumes `once` grants on success
   - `session` grants are only valid while app is in “foreground” (host tests inject foreground state)
   - `persistent` grants can have expiry; expired grants deny and are purgeable
3. Enumeration and revocation:
   - list grants per-app
   - enumerate all grants (admin/system)
   - revoke by appId+cap (and optionally by scope if needed; decide and document)
4. Storage format (host-first):
   - define JSONL persistence format under `state:/policy/grants/<appId>.jsonl`
   - host tests use a tempdir adapter; OS persistence is gated on `/state`
5. Deterministic audit emission:
   - for `grant`, `revoke`, and denied `require`
   - sink abstraction:
     - preferred: structured log record via `nexus-log` → `logd` (once `TASK-0006` exists)
     - fallback: deterministic UART markers explicitly labeled as bring-up audit
6. Deterministic host tests:
   - once grant consumption
   - session grant invalidation on “focus lost”
   - expiry handling via injected clock
   - wildcard rule (deny unless dev-mode)
   - enumerate/revoke behavior
   - audit records emitted to test sink deterministically

## Non-Goals

- Kernel changes.
- A full “Policy as Code” unification across all domains (still `TASK-0047`).
- A new standalone `auditd` authority/service (audit sink alignment remains `logd` direction).

## Constraints / invariants (hard requirements)

- Determinism: injected clock; stable ordering; stable tie-breaks.
- Bounded storage: caps on grants per app, maximum scope string length, maximum rationale length.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: “allow once” is consumed only on actual allow.

## Red flags / decision points (track explicitly)

- **YELLOW (scope matching rules)**:
  - resource patterns can drift into “mini-glob language”. v1.1 should define a conservative subset:
    - exact match, prefix match, and `**` suffix only (or similar) with explicit tests.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p policy_v1_1_host -- --nocapture` (or equivalent)
  - Required tests:
    - once/session/persistent semantics
    - expiry + purgeExpired
    - wildcard dev-mode gating
    - enumerate/revoke
    - deterministic audit emission to test sink

## Touched paths (allowlist)

- `source/services/policyd/` (extend)
- `tools/nexus-idl/schemas/policyd.capnp` (extend schema, versioned)
- `tests/policy_v1_1_host/` (new)
- `docs/policy/overview.md` (added/extended in `TASK-0168`)

## Plan (small PRs)

1. Add schema + implement grant store + require semantics with injected clock
2. Add revoke/enumerate + dev-mode wildcard rule
3. Add audit sink abstraction + deterministic host tests

## Acceptance criteria (behavioral)

- Host tests prove scoped grants + expiry + enumeration/revocation deterministically.
