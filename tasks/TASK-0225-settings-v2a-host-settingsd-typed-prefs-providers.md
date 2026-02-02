---
title: TASK-0225 Settings v2a (host-first): settingsd typed preferences registry + deterministic storage + provider “apply” hooks + tests
status: Draft
owner: @ui
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - Config broker (2PC reload for selected keys): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy authority + audit direction: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Policy v1.1 grant semantics (modes/expiry): tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - SystemUI Settings pages (DSL): tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

The repo already has a `settingsd` service crate and a `userspace/settings` client crate. The missing piece is a
clear **typed** settings contract that is deterministic, QEMU-friendly, and does not introduce storage drift.

This task is host-first: define semantics, storage format, and provider “apply” hooks with deterministic tests.
OS wiring and UI live in v2b.

## Goal

Deliver:

1. `settingsd` typed registry:
   - `Key { ns, kind, scope, default_json, doc }` with stable ordering
   - `Value { ns, scope, json }` where `json` is canonical (sorted keys, stable floats policy)
   - stable errors:
     - type mismatch → deterministic `EINVAL`-class error
     - unknown key → deterministic `ENOENT`-class error
2. Deterministic storage (NO libSQL in v2):
   - **canonical on-disk snapshot** (Cap'n Proto): 
     - device scope file: `state:/prefs/device.nxs`
     - user scope file: `state:/prefs/user/<uid>.nxs`
   - **derived/debug view**: `nx settings export --json` emits deterministic JSON (not a storage contract)
   - atomic write: temp → fsync(best-effort) → rename
   - bounded size/depth and stable reject rules
3. Provider “apply” hooks (host-proof, side effects mocked):
   - after a successful `set()`, `settingsd` calls an adapter:
     - `display.scale` → windowing adapter (stub in host tests)
     - `ime.locales`, `ime.personalization` → IME adapter
     - `notifications.dnd.*` → notifications adapter
     - `privacy.kill.*` → privacy adapter
   - `settingsd` emits deterministic markers for apply:
     - `settingsd: set ns=<...> scope=<...> apply=<provider>`
     - `settingsd: apply err ns=<...> reason=<...>` (only when apply truly fails)
4. Subscriptions:
   - `subscribe(prefix)` is **marker-only** in v2 (no streaming IPC contract yet)
5. Deterministic host tests `tests/settings_v2_host/`:
   - type validation
   - canonical JSON stability
   - atomicity semantics
   - provider apply ordering and error mapping (mock adapters)

## Non-Goals

- Kernel changes.
- Introducing a new DB dependency (libSQL/sqlite) for v2.
- Creating a second preferences authority parallel to `configd`; bridge is explicit and limited.
- “Deep links” (UI/router concern; v2b).

## Constraints / invariants (hard requirements)

- Deterministic bytes for storage and export under fixed inputs.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: markers only emitted after real set/apply.
- `/state` gating: if `/state` is absent, persistence must be explicit `stub/placeholder` (host tests remain valid).

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p settings_v2_host -- --nocapture`

## Touched paths (allowlist)

- `source/services/settingsd/` (extend)
- `userspace/settings/` (client contract)
- `tools/nexus-idl/schemas/settings.capnp` (new; versioned)
- `tests/settings_v2_host/` (new)
- `docs/settings/overview.md` (or doc stubs in v2b)

## Plan (small PRs)

1. Freeze `settings.capnp` + implement typed registry + storage adapter
2. Add provider hook trait + mock adapters + markers
3. Add deterministic host tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove typed validation, canonicalization, atomic persistence, and provider apply hooks.
