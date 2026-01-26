---
title: TASK-0180 Store v1a (host-first): offline feed (pkg://store) + storefeedd + storemgrd over bundlemgrd + ratings stub + deterministic tests
status: Draft
owner: @platform
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Packages install authority (bundlemgrd): tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Bundle authoring/signing (pkgr): tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md
  - Trust store unification (Ed25519): tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - SDK OS install proof (catalog decision): tasks/TASK-0166-sdk-v1-part2b-os-local-catalog-install-launch-proofs.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

We want a first “Store” slice that is **offline and deterministic**:

- a local feed of apps (metadata + `pkg://...` bundle URIs),
- an orchestrator that can install/update/remove apps using the existing packages authority,
- and a ratings/comments stub purely for UX (local-only).

Repo reality:

- `bundlemgrd` is the install authority (`TASK-0130`).
- We should avoid introducing a new `catalogd` unless forced (`TASK-0166` explicitly calls this out).

This task is host-first: we prove feed parsing, planning, orchestration logic, and ratings deterministically.
OS UI/selftests are in `TASK-0181`.

## Goal

Deliver:

1. `storefeedd` service (host-testable):
   - reads `pkg://store/feed.nxf` (Cap'n Proto; canonical, deterministic, signable)
     - authoring/fixture input may be `pkg://store/feed.json` (human-readable), compiled to `.nxf` at build time
     - optional derived/debug view: `nx store feed export --json` emits deterministic JSON
   - exposes RPC:
     - `list()`, `get(appId)`, `search(q)` (substring on title/summary; deterministic ordering)
   - markers (throttled):
     - `storefeedd: ready`
2. `storemgrd` service (host-testable core behavior):
   - orchestration over the install authority:
     - `plan(appId)` compares installed vs feed version (SemVer) and returns `install|update|noop`
     - `install/update/remove` drive **bundlemgrd** operations (verify/install/uninstall)
   - offline-only:
     - all artifacts are `pkg://store/nxb/...` (no network)
   - deterministic progress model:
     - verify → install → finalize (stable 0/30/90/100 steps)
   - markers (throttled):
     - `store: plan app=<id> action=<...>`
3. Ratings/comments stub library (host-first) and service surface decision:
   - store local ratings at `state:/store/ratings/<appId>.nxs` (Cap'n Proto snapshot; canonical) when `/state` exists
     - optional derived/debug view: `nx store ratings export --json` emits deterministic JSON
   - stable schema: `stars/title/text/ts/device_id_hash`
   - rules:
     - one rating per device per app (overwrite allowed)
     - compute average/count deterministically
   - quotas (soft/hard) are enforced when `/state` exists; otherwise ratings are explicitly `stub/placeholder`

## Non-Goals

- Kernel changes.
- Online store, payments, accounts.
- Remote ratings/comments.
- Introducing `catalogd` as a new authority unless forced (must be a separate explicit decision).

Follow-up:

- Store v2.2 adds offline purchases/licensing (NLT tokens), a sandbox wallet, ledger/revocations, and parental controls. Tracked as `TASK-0221`/`TASK-0222`.

## Constraints / invariants (hard requirements)

- Offline & deterministic: `pkg://` only; stable ordering in all lists.
- No fake success: install/update/remove only “ok” after bundlemgrd confirms success.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (catalogd vs bundlemgrd DB)**:
  - Store v1 must reuse `bundlemgrd` list/query as the installed catalog view.
  - If a separate `catalogd` is required later, it must be justified (no duplicated authority/format).

- **RED (/state gating)**:
  - ratings persistence requires `TASK-0009`. Without `/state`, ratings must be explicitly non-persistent and tests must not claim persistence.

- **YELLOW (trust store dependency)**:
  - signature verification must use the unified trust store contract (`TASK-0160`). Until that exists, storemgrd may only do placeholder verification with explicit markers containing `stub/placeholder`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p store_v1_host -- --nocapture`
  - Required tests:
    - feed parse stable order + search determinism
    - plan logic (installed vs feed SemVer)
    - install/update/remove orchestration calls (mocked bundlemgrd interface)
    - ratings write/overwrite + average/count + quota deny behavior (mocked statefs)

## Touched paths (allowlist)

- `source/services/storefeedd/` (new)
- `source/services/storemgrd/` (new)
- `userspace/libs/store-ratings/` (or similar; new)
- `tests/store_v1_host/` (new)
- `docs/store/` (minimal doc stub here or in v1b)

## Plan (small PRs)

1. Define feed schema + storefeedd + deterministic host tests
2. storemgrd planning + mocked bundlemgrd orchestration + tests
3. ratings stub + quotas + tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove feed/search, install planning/orchestration, and ratings stub behavior.
