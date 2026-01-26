---
title: TASK-0079 DSL v0.3a: IR stabilization + AOT Rust codegen + incremental rebuilds/tree-shaking + asset embedding + nx dsl --aot
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v0.2 core: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - DSL v0.2 stubs/CLI/demo: tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
  - UI kit baseline: tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
  - UI svg safe subset baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
  - DSL v1 DevX track: tasks/TRACK-DSL-V1-DEVX.md
---

## Context

Interpreter mode is great for iteration, but AOT codegen can reduce startup cost and improve steady-state CPU.
DSL v0.3 introduces an optional AOT path:

- stabilize IR (canonical ordering + stable IDs),
- generate plain Rust that constructs the same runtime/layout/kit graph,
- incremental rebuilds with tree-shaking from routes,
- embed assets (SVG/text/i18n) deterministically,
- add `nx dsl --aot` commands.

Perf benchmarking and OS demo integration is handled in v0.3b (`TASK-0080`).

## Goal

Deliver:

1. IR stabilization in `nx_ir`:
   - canonical ordering for props/children
   - stable `ir_id` values across runs
   - component/page defs (`IrComponentDef`, `IrPageDef`)
   - reachability graph from Routes for tree-shaking
   - marker string: `dsl: ir stable` (host-visible; OS marker only in v0.3b)
2. `nx_codegen` crate:
   - input: canonical `.nxir` (Cap'n Proto) (+ assets)
     - optional derived view input for tooling/debug: `.nxir.json` (must be derived from the canonical IR)
   - output: generated Rust crate under `userspace/apps/generated/<app>_dsl/`
   - generated API: `mount_<page>(...) -> ViewRootHandle`
   - router generation for routes
   - marker string: `dsl: aot codegen on`
   - profile semantics:
     - AOT must preserve the same `device.*` environment contract as interpreter mode (`TASK-0077`)
     - if IR contains `@when device.*` branches, codegen must emit deterministic branch evaluation and must not
       change behavior between interpreter and AOT (goldens prove parity)
3. Incremental rebuilds & tree-shaking:
   - content hashes per module
   - stable file paths per component/page
   - regenerate only changed modules
   - shake unreachable components from routes
   - summary marker: `dsl: aot gen (modules=<n> reused=<m> shaken=<k>)`
4. Asset embedding:
   - SVG: pre-parse safe subset to compact binary and embed via `include_bytes!`
   - locale packs: embed selected packs as bytes; runtime fallback to external packs
   - generated crate `build.rs` verifies asset hashes
   - marker: `dsl: assets embedded (svg=<n> locales=<m>)`
5. `nx dsl` CLI upgrades (host-first):
   - `nx dsl build <appdir> --aot [--release]`
   - `nx dsl run <appdir> --aot ... --profile ...` (headless run)
   - `nx dsl watch <appdir> --aot` (fs notify loop)
   - structured outputs under `target/nxir/`
6. Host tests for determinism and incremental behavior (no QEMU):
   - codegen determinism (byte-for-byte)
   - compile+run generated crate headless
   - incremental rebuild touches only the expected modules
   - tree-shaking removes unreachable components
   - build.rs asset verification fails on corruption

## Non-Goals

- Kernel changes.
- OS/QEMU integration markers (v0.3b).
- Full SDK/codegen stability guarantees beyond v0.3 scope (documented as “experimental AOT”).

## Constraints / invariants (hard requirements)

- Deterministic generation:
  - stable ordering and stable formatting in generated Rust
  - stable file paths and module names
- Bounded tool behavior:
  - cap app size (files/assets) to avoid runaway generation
  - guard watch mode from infinite loops (debounce)
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## v1 readiness gates (DevX, directional)

AOT is optional, but it must never change “the feel”:

- Interpreter and AOT must be behavior-identical for the same `{locale, profile, env}` inputs (goldens prove parity).
- Asset embedding (SVG/i18n) must remain deterministic so “first-party polish” doesn’t become flaky.
- Codegen must preserve boundedness (no unbounded compile-time explosion, stable module counts).

Track reference: `tasks/TRACK-DSL-V1-DEVX.md`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_3a_host/`:

- determinism: N runs generate identical Rust output
- compile+run: generated crate builds and renders headless successfully
- incremental: single-leaf change regenerates only expected module(s)
- tree-shaking: removing a route increases “shaken” count and drops generated modules
- assets: hash mismatch makes `build.rs` fail deterministically
- parity: for a fixture app/page, interpreter snapshot and AOT snapshot match for the same `{locale, profile}` inputs

## Touched paths (allowlist)

- `userspace/dsl/nx_ir/` (stabilization)
- `userspace/dsl/nx_codegen/` (new)
- `tools/nx-dsl/` (extend: --aot build/run/watch)
- `tests/dsl_v0_3a_host/` (new)
- `docs/dev/dsl/codegen.md` + `docs/dev/dsl/incremental.md` (new)

## Plan (small PRs)

1. IR stabilization + reachability graph
2. codegen crate + generated crate template + deterministic output rules
3. incremental rebuild graph + tree-shaking
4. asset embedding + build.rs verification
5. nx dsl CLI upgrades
6. host tests + docs
