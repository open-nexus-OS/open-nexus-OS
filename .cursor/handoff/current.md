# Current Handoff: TASK-0014 Observability v2 (metrics/tracing) — FULL SLICES BUILT, PROOFS GREEN

**Date**: 2026-02-13  
**Status**: `TASK-0014` remains active by policy (no explicit closure command yet). `phase-0a`, `phase-0`, `phase-1`, and `phase-2` are green, full planned closure slices are implemented, and final proof chain passed (`just dep-gate && just diag-os && cargo test --workspace && RUN_PHASE=mmio RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`).
**Delta report**: `.cursor/handoff/task-0014-delta-closure-report.md`

---

## What is stable now

- `metricsd` + `nexus-metrics` baseline is live and selftests pass in active mmio runs.
- `metricsd -> nexus-log -> logd` sink-path remains deterministic and green.
- Kernel heap OOM diagnostics now emit explicit budget and request details (`heap_budget_used/free/total`, request size/alignment).
- Delegated-policy sender/subject normalization is now evidence-driven for mmio bring-up aliases (selftest/rngd/keystored/statefsd paths).
- `policyd` identity binding now normalizes known bring-up aliases for sender-bound checks (`OP_CHECK/ROUTE/EXEC`) and delegated subjects (including `updated` alt SID observed in mmio runs).
- `selftest-client` logd STATS probe now uses CAP_MOVE + nonce correlation on the shared reply inbox, eliminating false zero-count/delta failures.
- Kernel stabilization for this slice is intentional (no rollback planned): heap budget increase + alloc diagnostics are treated as approved implementation reality for deterministic bring-up.

## Runtime progress proven this slice

- `rngd` delegated policy now allows entropy path deterministically (`rngd: policy allow`, `rngd: mmio window mapped ok`).
- Metrics/tracing semantics now pass in mmio (`SELFTEST: metrics security rejects ok`, `SELFTEST: metrics counters ok`, `SELFTEST: metrics gauges ok`, `SELFTEST: metrics histograms ok`, `SELFTEST: tracing spans ok`).
- Retention proof is now active in mmio (`[INFO metricsd] retention wal active`, `SELFTEST: metrics retention ok`).
- Device-key and statefs selftest proofs now pass (`SELFTEST: device key pubkey ok`, `SELFTEST: device key persist ok`, `SELFTEST: statefs put ok`, `SELFTEST: statefs persist ok`).
- `ipc sender service_id` selftest is green in the current ladder.
- OTA/update path now passes in mmio (`SELFTEST: ota stage ok`, `SELFTEST: ota switch ok`, `SELFTEST: ota health ok`, `SELFTEST: ota rollback ok`, `SELFTEST: bootctl persist ok`).
- Latest proof snapshot: `RUN_PHASE=mmio RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` with `exit_code=0`, `first_failed_phase=""`, `missing_marker=""`.
- Final build/proof chain snapshot:
  - `just dep-gate`: pass (`no forbidden crates in OS graph`)
  - `just diag-os`: pass
  - `cargo test --workspace`: pass
  - `RUN_PHASE=mmio RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`: pass (`exit_code=0`)

## New architectural decision (this slice)

- `nexus-log` now exposes explicit sink-slot configuration:
  - `configure_sink_logd_slots(logd_send, reply_send, reply_recv)`.
- Deterministic wiring is now **opt-in per service/process** (init-lite assigned slots), not a hidden global fallback policy in `nexus-log`.
- If slots are not configured (or invalid), `nexus-log` still uses routed discovery (`logd` + `@reply`) as bounded fallback.

## Active focus (next)

- Keep TASK/RFC/state artifacts in lockstep with implementation reality and proof outputs.
- Keep reject-matrix and retention claims strictly evidence-bound (host tests + marker ladder).
- Keep sender-alias normalization constrained to observed/verified identities only; avoid speculative broadening.
- Prepare explicit closure handoff package for `TASK-0014` (without changing status unless requested).

## Why still in progress

- Full-scope slices are implemented and proven, but user requested to keep `TASK-0014` in `In Progress` until explicit closure command.

## Guardrails

- No fake success markers (`ready`/`ok` only after proven behavior).
- No unbounded cardinality, payload, span-table, or rate growth.
- Identity/policy from kernel-authenticated `sender_service_id` only.
- Proof floor remains modern virtio-mmio with bounded deterministic timeouts.
