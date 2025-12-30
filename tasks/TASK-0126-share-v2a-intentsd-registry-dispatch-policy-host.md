---
title: TASK-0126 Share v2a: intentsd registry/query/dispatch + result callbacks + shared_policy limits + host tests
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Share sheet v1 (screenshot broker baseline): tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md
  - Grants (content://): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - MIME defaults (chooser “always use”): tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Clipboard v3 target (optional): tasks/TASK-0087-ui-v13a-clipboard-v3.md
  - contentd saveAs (Save target dependency): tasks/TASK-0112-ui-v19b-contentd-saveas-downloads.md
---

## Context

`TASK-0068` defines a minimal share-sheet broker for screenshots. Share v2 upgrades sharing into an
**intent-based** model with a registry, chooser targets, and result callbacks.

This task is **host-first**: it delivers the intent registry/dispatch service (`intentsd`) and a strict
payload policy module with deterministic tests.

UI and app wiring are handled in follow-up tasks.

## Goal

Deliver:

1. `intentsd` service:
   - intent target registry (`registerTarget`)
   - query targets by MIME (`queryTargets`)
   - dispatch to a target and return a request id (`dispatch`)
   - await result with timeout (`awaitResult`)
   - MRU ranking (`bumpRank`) with deterministic ordering rules
   - persistence of registered targets to `state:/intents/targets.json` (if `/state` exists; otherwise host tests simulate persistence)
2. IDL contract:
   - `tools/nexus-idl/schemas/intent.capnp` describing Target/Payload/Result and Intents API
   - contract must be versioned and compatible with OS constraints (bounded allocations, stable errors)
3. `shared_policy` helper:
   - validates `Payload` budgets and allowed URI schemes (`content://` only)
   - enforces:
     - max URIs
     - max text/html bytes
     - max image bytes
   - HTML sanitization as a safe subset (deterministic stripping; scripts/styles/event handlers removed)
   - emits marker/audit line on policy rejection: `policy: share limit hit (kind=...)`
4. Markers:
   - `intentsd: ready`
   - `intent: target register app=...`
   - `intent: dispatch rid=...`
   - `intent: result rid=... status=...`
   - `policy: share limits enforce on`

## Non-Goals

- Kernel changes.
- SystemUI chooser UI (follow-up).
- Built-in targets/providers (follow-up).
- App sender wiring and OS selftests/postflight (follow-up).
- Ability lifecycle state machine (handled by `appmgrd` + `TASK-0234`/`TASK-0235`; `intentsd` only routes intents, not manages ability states).

## Constraints / invariants (hard requirements)

- Deterministic:
  - MRU ordering given a deterministic bump sequence
  - policy decisions and error reasons
  - HTML sanitization output for fixtures
- Bounded memory and bounded payload sizes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/share_v2_host/`:

- register/query returns deterministic ordering
- bumpRank deterministically reorders targets
- dispatch/awaitResult roundtrip with loopback receiver returns within timeout
- policy rejects oversize payloads with `error:policy`
- policy rejects non-`content://` URIs deterministically
- HTML sanitizer removes banned content deterministically

## Touched paths (allowlist)

- `source/services/intentsd/` (new)
- `source/services/shared_policy/` (new)
- `tools/nexus-idl/schemas/intent.capnp` (new)
- `tests/share_v2_host/` (new)
- `docs/share/overview.md` (added in later task or here if minimal)

## Plan (small PRs)

1. IDL schema + intentsd in-memory registry + markers
2. shared_policy budgets + sanitizer + markers
3. dispatch + awaitResult mechanism + host tests
