---
title: TASK-0080 DSL v0.3b (partly OS-gated): perf benches (AOT vs interp) + cold-start budget breakdown + AOT demo launch + CI perf gates
status: Draft
owner: @ui @runtime
created: 2025-12-23
updated: 2026-07-06
depends-on:
  - tasks/TASK-0079-dsl-v0_3a-aot-codegen-incremental-assets.md
  - tasks/TASK-0080C-systemui-dsl-bootstrap-shell-os-wiring.md
follow-up-tasks: []
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Perf contract this task fills with numbers: docs/dev/dsl/perf.md
  - Launch pipeline being measured: tasks/TASK-0080D (app-host), docs/adr/0042-cross-process-surface-transport.md
  - Metrics substrate: source/services/metricsd (RFC-0024); markers via scripts/qemu-test.sh
  - Bench home: tools/bench/ (exists)
---

## Context (updated 2026-07-06)

Both execution tiers exist (interpreter app-host from 0080D, AOT ELF from 0079) and
the launch pipeline is live (0080C). This task makes performance a **measured,
CI-gated property**: the masterplan's promise is app cold-start in the class of the
fastest mobile platforms — that only holds if we measure per stage and gate
regressions.

## Goal

1. **Host bench tool** `tools/bench/dsl_aot_vs_interp/`:
   - scenes: simple controls, windowed list (1k rows via QuerySpec transcript),
     vector/text-heavy page;
   - metrics: mount time, first-frame time, steady-state dispatch→scene time,
     allocations per steady-state dispatch (must be 0);
   - fixed seeds/iteration counts; JSON report `target/bench/dsl_perf.json` with a
     stable schema; comparison mode fails on regression beyond threshold
     (**CI perf gate**).
2. **OS cold-start budget breakdown** (markers, timestamped):
   - `APPHOST: spawn→payload <ms>`, `payload→validated <ms>`, `validated→mounted
     <ms>`, `mounted→first-present <ms>`, and the total
     `PERF: cold_start_ms=<n> tier=interp|aot app=<id>`;
   - measured for the master-detail demo in both tiers from a launcher click;
   - budgets recorded in `docs/dev/dsl/perf.md` after first real measurements
     (living numbers, then regression-gated in postflight).
3. **AOT demo integration**: master-detail AOT variant installed as a second bundle;
   launcher shows both tiles (interp/AOT); parity spot-check on OS via scene checksum
   marker (`SELFTEST: dsl v0.3 aot parity ok`).
4. **Steady-state OS proof**: live interaction on the AOT app; allocation-audit debug
   assert stays silent (the app-host zero-alloc rule, on hardware).
5. Postflight `tools/postflight-dsl-v0-3.sh`: host suites + bench run + QEMU marker
   chain, delegating to canonical mechanisms.

## Non-Goals

- Guaranteeing AOT is always faster (bench tracks reality; deltas may vary and are
  documented). Kernel changes. New scenes beyond the three (grow with consumer apps).

## Constraints / invariants (hard requirements)

- Bench determinism: fixed seeds, stable iteration counts, stable JSON schema.
- OS timing from existing marker/timestamp infrastructure — no new ad-hoc clocks;
  headless-icount caveat documented (timing claims need the real virgl lane).
- No fake proof: perf markers carry measured numbers, never unconditional "ok".
- No `unwrap/expect`; no godfiles.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `tests/dsl_v0_3_host/` green;
- `cargo run -p dsl_aot_vs_interp -- --scenes all --iters N` → `dsl_perf.json`;
  re-run stable within tolerance; regression mode exits non-zero on injected slowdown
  (fixture);
- zero-alloc metric = 0 for both tiers on all scenes.

### Proof (OS/QEMU) — gated (user boot-verify)

- cold-start marker chain with plausible stage numbers for interp AND aot tiles;
- `SELFTEST: dsl v0.3 aot {boot,parity,i18n} ok`;
- live-pointer interaction on the AOT app visible; 0 faults.

### Docs — required

- `docs/dev/dsl/perf.md` becomes the living budget page: measured cold-start stage
  table (interp vs AOT), steady-state numbers, gate thresholds, when-to-AOT guidance.

## Touched paths (allowlist)

- `tools/bench/dsl_aot_vs_interp/` (new), `tools/postflight-dsl-v0-3.sh` (new)
- AOT demo bundle wiring (`bundles/`, generated crate from 0079)
- `source/services/app-host/` (stage timestamps), `source/apps/selftest-client/`
- `tests/dsl_v0_3_host/` (new)
- `docs/dev/dsl/{perf,cli}.md`

## Plan (small PRs)

1. bench tool + JSON + regression mode (host)
2. app-host stage markers + cold-start chain [boot-verify]
3. AOT bundle + launcher tile + parity/i18n selftests [boot-verify]
4. perf.md budgets + postflight + CI gate wiring
