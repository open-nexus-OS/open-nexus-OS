# Metrics (Observability v2, local)

This document describes the local metrics contract from `TASK-0014`.

Primary references:

- `tasks/TASK-0014-observability-v2-metrics-tracing.md` (execution truth)
- `docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md` (contract seed)
- `recipes/observability/metrics.toml` (limits/retention defaults)
- `scripts/qemu-test.sh` (authoritative marker contract)

## Components

- **`metricsd` (OS service)**: bounded registry for counter/gauge/histogram + span export trigger path.
- **`nexus-metrics` (client lib)**: producer-facing API for services/apps.
- **`logd` (sink)**: authoritative local export sink via structured records.

## Metric model

- **Counter(u64)**: monotonic increment.
- **Gauge(i64)**: last-value set semantics.
- **Histogram(u64 ns)**: fixed buckets with overflow bucket.

Current in-service histogram bucket boundaries (ns):

- `<= 1_000_000`
- `<= 5_000_000`
- `<= 20_000_000`
- `<= 100_000_000`
- overflow

## Bounded limits (current floor)

From `source/services/metricsd/src/lib.rs`:

- `MAX_SERIES_TOTAL = 64`
- `MAX_SERIES_PER_METRIC = 16`
- `MAX_LIVE_SPANS = 64`
- `RATE_WINDOW_NS = 1_000_000_000`
- `RATE_MAX_EVENTS_PER_WINDOW = 64`
- `RATE_MAX_SUBJECTS = 64`

Reject categories:

- `invalid_args`
- `over_limit`
- `rate_limited`

## Identity and security

- Identity for decisions is kernel-authenticated `sender_service_id`.
- Payload identity spoofing is rejected.
- Export path is bounded; no unbounded allocation on wire decode path.

## Deterministic export contract

`metricsd` exports snapshots to `logd` through `nexus-log` sink wiring. Deterministic service startup may configure explicit slots through:

- `configure_sink_logd_slots(logd_send, reply_send, reply_recv)`

Fallback remains routed `logd` + `@reply` discovery when explicit slots are not configured.

## Proof markers (QEMU)

Required v2 metrics markers:

- `metricsd: ready`
- `metricsd: reject invalid_args`
- `metricsd: reject over_limit`
- `metricsd: reject rate_limited`
- `SELFTEST: metrics security rejects ok`
- `SELFTEST: metrics counters ok`
- `SELFTEST: metrics histograms ok`

## Current status

- Phase 0a/0 are green in mmio proofs.
- Phase 1 reject matrix is complete, including oversized metric-field rejection coverage.
