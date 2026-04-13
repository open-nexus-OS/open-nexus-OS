---
title: TASK-0172 Perf v2a (host-first): perfd sessions API + deterministic stats + export format (.nptr) + tests
status: Draft
owner: @reliability
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Perf v1 plan (superseded): tasks/TASK-0143-perf-v1a-perfd-frame-trace-metrics.md
  - Perf gates v1 plan (superseded): tasks/TASK-0145-perf-v1c-deterministic-gates-scenes.md
  - SDK IDL freeze (perf schema later): tasks/TASK-0163-sdk-v1-part1a-idl-freeze-codegen-wire-gates.md
  - Persistence (/state export, OS-gated): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want performance work to be provable and regression-resistant. v1 (`TASK-0143/0145`) outlines perfd + gates,
but we now need a clearer v2 model:

- explicit **sessions** with warmup/sample windows,
- deterministic stats computation (mean/p95/long frames),
- deterministic export artifact format suitable for CI and local debugging.

This task is **host-first** and focuses on perfd’s core data model and math.
Scenario orchestration and CI gates are in `TASK-0173`.

## Goal

Deliver:

1. `perfd` v2 API (service + schema):
   - begin(spec) / frameTick(dtUs,cpuMs,rssKiB) / mark(tag) / end() / last() / export(path)
   - stable session naming
   - warmup frames are discarded before computing summary
2. Deterministic stats:
   - nearest-rank percentile (documented)
   - stable rounding policy (avoid FP nondeterminism; if floats are used, define rounding)
   - long-frame classification derived from configured budget (or from dt threshold rule; decide and document)
3. Export format `.nptr` (Nexus Perf Trace):
   - deterministic archive (zip/tar; choose one and define rules)
   - includes:
     - `manifest.json` (session spec + budgets used + summary + build id)
     - `frames.bin` (fixed binary encoding) or `frames.jsonl` (if bounded and deterministic)
     - `marks.jsonl` (optional)
   - deterministic file ordering and normalized mtimes in export
4. Host tests:
   - stats math exactness (mean/p95/long counts)
   - budget gate logic (pass/fail)
   - export determinism (stable hash under fixed input)
5. Markers (rate-limited):
   - `perfd: ready v2`
   - `perf: begin <name>`
   - `perf: end <name> mean=... p95=... long=...`

## Non-Goals

- Kernel changes.
- Wiring producers (windowd/search/ime/etc.) in this task.
- QEMU timing-based hard gates (handled via deterministic scenarios in v2b).

## Constraints / invariants (hard requirements)

- Determinism: given deterministic inputs, summaries and exports are byte-stable.
- Bounded memory: ring buffers have fixed size; export size is bounded.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **YELLOW (rssKiB stability)**:
  - RSS is not stable under QEMU. v2 should treat it as optional/diagnostic:
    - either stub to a deterministic value in OS builds, or
    - exclude from gate decisions and only report it.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p perf_v2_host -- --nocapture`
  - Required tests:
    - deterministic stats + percentiles
    - deterministic export hash for fixture input stream

## Touched paths (allowlist)

- `source/services/perfd/` (new)
- `tools/nexus-idl/schemas/perf.capnp` (new or extended; schema doc)
- `tests/perf_v2_host/` (new)
- `docs/perf/overview.md` (added in `TASK-0173` or here if minimal)

## Plan (small PRs)

1. perfd v2 schema + ring buffer + stats math
2. export format `.nptr` + deterministic archive rules
3. host tests + docs stubs

## Acceptance criteria (behavioral)

- Host tests prove stats/export determinism.
