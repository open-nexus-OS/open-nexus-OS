---
title: TASK-0080 DSL v0.3b: perf benchmarks (AOT vs interp) + AOT demo integration + OS selftests/postflight + docs
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v0.3a codegen foundation: tasks/TASK-0079-dsl-v0_3a-aot-codegen-incremental-assets.md
  - DSL v0.2 demo baseline: tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
  - UI v6b launcher integration (tiles): tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With an AOT codegen path available (v0.3a), we need:

- measurable performance deltas (first frame, steady-state CPU),
- an OS-visible AOT demo side-by-side with interpreter,
- parity checks between AOT and interpreter (lightweight checksum),
- postflight scripts that prove “real behavior” (host tests + perf + QEMU markers).

## Goal

Deliver:

1. Perf benchmarks (host):
   - new bench tool `dsl_aot_vs_interp` comparing:
     - first frame ms
     - steady-state CPU ms/frame
   - scenes:
     - simple controls
     - list scene
     - SVG + shaped text scene
   - output JSON report under `target/bench/dsl_perf.json`
   - marker: `bench: dsl aot vs interp done`
2. AOT demo integration:
   - build an AOT variant of the v0.2 master-detail app (generated crate + runner shim)
   - SystemUI shows two launcher tiles:
     - “DSL MasterDetail (Interp)”
     - “DSL MasterDetail (AOT)”
   - markers:
     - `dsl: aot demo launched`
     - `dsl: aot first frame ok`
3. OS selftests:
   - launch AOT demo and verify:
     - boot ok marker
     - route nav parity vs interpreter via a scene checksum helper
     - locale switch changes visible strings
4. Postflight:
   - host tests + perf run + QEMU marker checks
5. Docs:
   - how to interpret perf JSON
   - when to use AOT vs interpreter

## Non-Goals

- Kernel changes.
- Guarantee that AOT is always faster (bench is for regression tracking; deltas may vary).

## Constraints / invariants (hard requirements)

- Bench determinism:
  - fixed scene seeds
  - stable iteration counts
  - stable output JSON schema
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Postflight must delegate to canonical proof mechanisms (cargo test, qemu-test).

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p dsl_v0_3_host` green (or equivalent host suite)
- `cargo run -p dsl_aot_vs_interp -- --scenes all --iters N` produces `dsl_perf.json`

### Proof (OS/QEMU) — gated

UART markers:

- `dsl: aot codegen on`
- `dsl: aot demo launched`
- `dsl: aot first frame ok`
- `SELFTEST: dsl v0.3 aot boot ok`
- `SELFTEST: dsl v0.3 aot parity ok`
- `SELFTEST: dsl v0.3 aot i18n ok`

## Touched paths (allowlist)

- `tools/bench/dsl_aot_vs_interp/` (new)
- `userspace/apps/examples/dsl_masterdetail_aot/` or generated app wiring
- SystemUI launcher tiles
- `source/apps/selftest-client/` (markers)
- `tools/postflight-dsl-v0-3.sh` (delegates)
- `docs/dsl/perf.md` (new)
- `docs/dsl/cli.md` (extend: --aot perf/watch)

## Plan (small PRs)

1. bench tool + JSON output
2. AOT demo app integration + SystemUI tiles + markers
3. OS selftest parity + i18n markers
4. postflight + docs

