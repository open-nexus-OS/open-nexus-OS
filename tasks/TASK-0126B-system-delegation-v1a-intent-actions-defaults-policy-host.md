---
title: TASK-0126B System Delegation v1a (host-first): action-based intents (chat/contacts/compose/maps) + per-action defaults + deterministic routing + policy hooks
status: Draft
owner: @runtime @ui
created: 2026-01-28
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - Share v2 intents baseline: tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Chooser + defaults (MIME): tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
  - MIME defaults registry: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Policy v1 capability matrix + intents adapter: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Scoped grants + content://: tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Chat transfer (default eligibility): tasks/TASK-0126C-chat-transfer-v1a-host-export-import-eligibility.md
  - Chat inline action cards (track/confirm/open): tasks/TASK-0126D-chat-action-cards-v0-host-inline-track-confirm-open.md
---

## Context

Share v2 is MIME-driven. For “system surfaces” (chat compose, contacts picker, maps pick-location),
MIME alone is not expressive enough.

We want a **WeChat-like convenience** (apps delegate to a system app) without a “super-app”:
the OS provides a stable delegation mechanism; apps don’t embed other apps and don’t gain extra data access.

This task is host-first and defines the **action-based** extension to `intentsd` plus deterministic routing rules.

## Goal

Deliver:

1. Action-based target registration and query:
   - `registerActionTarget(action, targetMeta)`
   - `queryActionTargets(action)` returns deterministic ordering (default + MRU + stable tie-breakers)
2. Action dispatch and result:
   - `dispatchAction(action, payload)` returns request id
   - `awaitResult(rid, timeout)` returns deterministic status and optional bounded output
3. Action defaults:
   - set default handler for an action (parallel to `mimed.setDefault(mime, appId)`)
   - persistence to `/state` when available; host tests simulate persistence
4. Policy hooks (no duplicate policy logic):
   - `intentsd` dispatch is gated by `policyd.require(...)` (via existing intents capability patterns)
   - receiver registration is gated by `intents.receive` (consistent with `TASK-0136`)
5. Deterministic tests.

## Proposed v1 action catalog (minimal, non-sensitive)

This is a starting set for v1. It is intentionally small and expandable:

- `chat.compose` (user-mediated; confirm required in UI)
- `social.compose` (user-mediated; confirm required in UI)
- `contacts.pick` (returns exactly one selection; no enumeration)
- `maps.pick_location` (one-shot location; no live tracking)

Non-goals for v1a:
- payments, identity verification, background sending, live location sharing.

Future note (out of scope for v1a, but part of the track direction):
- External message ingress (e.g., SMS/MMS) should be routed into the **default chat app** as the unified inbox,
  via a transport adapter service + delegation, not via separate “SMS apps”.

Default eligibility note (directional):
- For certain powerful actions (notably `chat.*`), the OS should only allow setting a **default handler**
  if the app advertises chat export/import (transfer) support (see `TASK-0126C` and `tasks/TRACK-SYSTEM-DELEGATION.md`).

## Constraints / invariants (hard requirements)

- Deterministic ordering and deterministic allow/deny reasons.
- Bounded payload sizes; allowed URI schemes remain `content://` only where URIs are used.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success markers: return `error:policy` / `error:invalid` where applicable.
- Identity is channel-bound (OS builds): do not trust caller strings as identity.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add or extend tests (suggested: `tests/system_delegation_v1_host/`):

- `queryActionTargets` ordering is deterministic given:
  - registered targets,
  - default selection,
  - MRU bump sequence.
- dispatch/awaitResult roundtrip works via loopback receiver.
- `test_reject_*` cases:
  - reject unknown action (stable error)
  - reject oversize payload (stable error)
  - reject non-`content://` URIs if action uses URIs
  - reject dispatch without required policy grant (stable `error:policy`)

## Touched paths (allowlist)

- `source/services/intentsd/` (extend)
- `source/services/shared_policy/` (reuse/extend policy helpers where appropriate; no duplication)
- `tools/nexus-idl/schemas/intent.capnp` (extend: action field + action defaults APIs)
- `tests/` (new host tests)
- `docs/share/overview.md` (optional minimal note; keep it short if added)

## Plan (small PRs)

1. Extend IDL with `action` (string or enum) + action-default APIs + bounded payload struct.
2. Extend intentsd in-memory registry for action targets + deterministic ordering.
3. Implement dispatch/awaitResult parity with existing MIME-based flow.
4. Add host tests + rejection tests.
