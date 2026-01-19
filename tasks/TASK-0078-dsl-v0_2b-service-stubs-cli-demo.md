---
title: TASK-0078 DSL v0.2b: IDL client stubs + nx dsl run/i18n extract + master-detail demo + host/OS proofs
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Zero-Copy App Platform (svc.* consumers): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Office Suite (reference apps): tasks/TRACK-OFFICE-SUITE.md
  - NexusMedia SDK track (audio/video/image): tasks/TRACK-NEXUSMEDIA-SDK.md
  - NexusGame SDK track (games): tasks/TRACK-NEXUSGAME-SDK.md
  - NexusNet SDK track (cloud + DSoftBus): tasks/TRACK-NEXUSNET-SDK.md
  - DSL v0.2a core: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - DSL v0.1 CLI baseline: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - DSL v0.1 interpreter baseline: tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
  - Search service (example stub target): tasks/TASK-0071-ui-v9a-searchd-command-palette.md
  - App lifecycle launch (demo integration): tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Virtualized list (demo uses it if present): tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
  - DSL query objects (optional data ergonomics): tasks/TASK-0274-dsl-v0_2c-db-query-objects-builder-defaults-paging-deterministic.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

DSL v0.2a introduces stores/effects/navigation/i18n. v0.2b makes effects useful by allowing service calls
via typed stubs, and provides end-to-end tooling and a demo app.

## Goal

Deliver:

1. IDL client stub registry:
   - new crate `userspace/dsl/nx_stubs`
   - typed, minimal clients (Cap’n Proto) for a small set of services (e.g., `users`, `search`)
   - mock mode for host tests
   - effect runner integration: `Call(ServiceFn)` with timeouts and error mapping
2. `nx dsl` CLI upgrades:
   - `nx dsl run <appdir> --route ... --locale ... --profile ...` (headless run; OS mount optional)
   - `nx dsl i18n extract <appdir> -o i18n/en.json`
   - stronger lint rules (reducers pure, routes unique, i18n coverage)
3. Example app: `dsl_masterdetail`
   - routes `/` and `/detail/:id`
   - store loads data via stub service call
   - i18n packs `en` and `de`
4. SystemUI launcher entry and OS markers:
   - `dsl: example masterdetail launched`
   - `dsl: nav to /detail/... ok`
   - selftest markers for load/nav/i18n
5. Host tests + OS postflight.

## Non-Goals

- Kernel changes.
- Full codegen and schema-driven stub generation (manual stubs only in v0.2b).
- Full service coverage.
- DB query objects / paging tokens / lazy loading contract (tracked in `TASK-0274`/`TASK-0275`).

## Constraints / invariants (hard requirements)

- Deterministic effect scheduling:
  - effects run after reducer commit,
  - bounded concurrency and timeouts.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_2_host/`:

- mock service call returns deterministic data; `LoadRequested` results in `Loaded` and state update
- navigation to `/detail/7` mounts correct route subtree and snapshot matches golden
- locale switch changes rendered strings deterministically
- `nx dsl run` prints expected markers and exits 0
- profile fixtures: `--profile desktop` vs `--profile tv` produces deterministically different (but stable) snapshots for the demo app

### Proof (OS/QEMU) — gated

UART markers:

- `dsl: store runtime on`
- `dsl: nav runtime on`
- `dsl: i18n on`
- `SELFTEST: dsl v0.2 load ok`
- `SELFTEST: dsl v0.2 nav ok`
- `SELFTEST: dsl v0.2 i18n ok`

## Touched paths (allowlist)

- `userspace/dsl/nx_stubs/` (new)
- `userspace/dsl/nx_interp/` (extend: effect runner calls stub registry)
- `tools/nx-dsl/` (extend: run + i18n extract)
- `userspace/apps/examples/dsl_masterdetail/` (new)
- SystemUI launcher entries (demo)
- `tests/dsl_v0_2_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-dsl-v0-2.sh` (delegates)
- `docs/dsl/services.md` + `docs/dsl/cli.md` (extend)

## Plan (small PRs)

1. stub registry + mock mode + effect runner integration
2. nx dsl run + i18n extract + improved lint
3. master-detail example + i18n packs + markers
4. host tests + OS selftest + postflight + docs
