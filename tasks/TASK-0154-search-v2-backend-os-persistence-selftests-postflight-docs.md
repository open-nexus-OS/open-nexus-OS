---
title: TASK-0154 Search v2 backend (OS/QEMU): service wiring + OS sources + optional persistence + selftests/postflight + docs
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Search v2 backend host slice: tasks/TASK-0153-search-v2-backend-host-index-ranking-analyzers-sources.md
  - Search v2 UI execution: tasks/TASK-0152-search-v2-ui-os-deeplinks-selftests-postflight-docs.md
  - Search backend baseline: tasks/TASK-0071-ui-v9a-searchd-command-palette.md
  - Recents substrate: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - MIME/content substrate (files): tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Packages/apps list substrate: tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Store v1 feed/service (optional source): tasks/TASK-0180-store-v1a-host-storefeedd-storemgrd-ratings.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Quotas v1: tasks/TASK-0133-statefs-quotas-v1-accounting-enforcement.md
  - Policy caps: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Config broker: tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Search v2 backend is host-proven in `TASK-0153`. This task wires it into OS/QEMU:

- `searchd` service readiness and IPC,
- OS source adapters (apps/settings/files/recents) using real services where available,
- OS selftests and marker contract,
- optional index persistence under `/state` when `TASK-0009` exists (otherwise explicitly no persistence).

## Goal

Deliver:

1. OS `searchd` wiring:
   - prints `searchd: ready` only after it can answer `query/suggest`
   - marker examples (throttled):
     - `searchd: ready`
     - `search: reindex done docs=<n>`
2. OS source adapters (deterministic, offline):
   - Apps: from bundle manager (or fixture fallback)
   - Settings: from a deterministic in-package registry (localized)
   - Docs: from a small allowlisted `pkg://docs/...` fixture set (no crawling)
   - Store (optional): from `storefeedd` when present (offline), otherwise fixture fallback
   - Files: limited allowlist only (e.g., Documents/Downloads or fixtures) and no network
   - Recents: from `recentsd` (or fixture fallback)
3. Policy + quotas:
   - caps:
     - `search.query`, `search.index.write`, `search.debug`
   - if `/state` exists: enforce a quota for search index artifacts (soft/hard)
4. OS selftests (bounded):
   - `reindex` works and prints markers
   - query returns expected top hit for `settings dark` (DE)
   - usage boost path (recents) affects ranking deterministically
   - suggest returns at least one term deterministically
   - multilingual query path:
     - one JA/KR/ZH query fixture returns at least one expected hit deterministically
   - markers:
     - `SELFTEST: search v2 settings ok`
     - `SELFTEST: search v2 usage ok`
     - `SELFTEST: search v2 suggest ok`
     - `SELFTEST: search v2 multi ok`
5. Postflight + docs:
   - postflight delegates to canonical proofs:
     - host tests (`search_v2_host`)
     - QEMU marker run (`scripts/qemu-test.sh`)
   - docs: overview, sources, testing, tuning knobs

## Non-Goals

- Kernel changes.
- Network indexing.
- Unbounded file crawling (strict allowlist).

## Constraints / invariants (hard requirements)

- Deterministic behavior (stable tie-breaks, stable fixture ordering).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success:
  - if `/state` is not present, do not claim “index persisted”; emit explicit `stub/placeholder` markers if referenced.

## Red flags / decision points (track explicitly)

- **RED (persistence gated on `/state`)**:
  - the prompt’s “index path: state:/search/index” is only valid once `TASK-0009` exists.
  - before that, search must run in-memory only.

- **YELLOW (source availability drift)**:
  - `bundlemgrd`/`recentsd`/`mimed` may be staged later. Adapters must either:
    - integrate when services exist, or
    - fall back to deterministic fixtures with explicit `stub/placeholder` markers.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p search_v2_host -- --nocapture`

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh`
  - Required markers (to be added to `scripts/qemu-test.sh` expected list):
    - `searchd: ready`
    - `search: reindex done docs=`
    - `SELFTEST: search v2 settings ok`
    - `SELFTEST: search v2 usage ok`
    - `SELFTEST: search v2 suggest ok`

## Touched paths (allowlist)

- `source/services/searchd/`
- `userspace/search/`
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh` (marker contract update)
- `tools/postflight-search-v2.sh` (delegates)
- `docs/search/overview.md` + `docs/search/sources.md` + `docs/search/testing.md`
- `docs/ui/testing.md` (link)

## Plan (small PRs)

1. OS wiring + readiness markers + IPC
2. OS adapters + allowlists + policy caps
3. Selftests + marker contract + docs + postflight

## Acceptance criteria (behavioral)

- In QEMU, `searchd` answers query/suggest and selftests prove settings/usage/suggest markers.
- If `/state` is unavailable, behavior is still correct and explicitly non-persistent.
