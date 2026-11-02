---
schema: 1
id: TASK-0225
title: "Settings v2a (host-first): settingsd typed preferences registry + deterministic storage + provider “apply” hooks + tests"
status: draft
owner: ["@ui"]
created: 2025-12-29
subsystem: storage
tracks: [TRACK-ADS-SAFETY-FAMILYMODE]
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "Config broker (2PC reload for selected keys)"
    target: tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - label: "Policy authority + audit direction"
    target: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - label: "Policy v1.1 grant semantics (modes/expiry)"
    target: tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - label: "SystemUI Settings pages (DSL)"
    target: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - label: "Data formats rubric (JSON vs Cap'n Proto)"
    target: docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

> **Reconciliation note (2026-07-21):** the core of this plan already shipped —
> `source/services/settingsd/src/registry.rs` has the typed SPECS registry with
> validators, and persistence lands via the prefs blob at `state:/prefs/device.nxs`
> (TASK-0072 line). The remaining live scope (new region/keymap/time keys +
> `OP_WATCH` change propagation) moved to **TASK-0298** (RFC-0078). This ledger
> stays Draft only for the provider-"apply"-hook generalization; do not re-plan
> the registry from here.

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
