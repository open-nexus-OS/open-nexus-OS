# Tracing (Observability v2, local)

This document describes the local tracing span contract from `TASK-0014`.

Primary references:

- `tasks/TASK-0014-observability-v2-metrics-tracing.md`
- `docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md`
- `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`
- `scripts/qemu-test.sh`

## Scope

Tracing v2 is local-only in this slice:

- span start/end handling in `metricsd`,
- deterministic IDs,
- span-end export to `logd`.

Cross-node propagation/correlation is out of scope here and belongs to follow-up tasks.

## Span model

- `span_start(sender_service_id, span_id, trace_id, start_ns, name)`
- `span_end(sender_service_id, span_id, end_ns, status)`

On successful end, `metricsd` emits a structured span-end record to `logd` with:

- sender identity,
- `span_id`,
- `trace_id`,
- span name,
- duration (`end_ns - start_ns`, saturating),
- status.

## Deterministic IDs

For OS builds in this slice:

- IDs are deterministic (no RNG requirement).
- `span_id` must match sender-binding constraints; mismatches reject as `invalid_args`.

This keeps proofs deterministic and avoids fake-green behavior caused by non-repeatable IDs.

## Bounds and rejects

- Live span table is bounded (`MAX_LIVE_SPANS`).
- Duplicate or sender-mismatched span starts reject deterministically.
- Unknown span end returns deterministic `not_found`.

Reject classes used by the service:

- `invalid_args`
- `over_limit`
- `rate_limited`
- `not_found`

## Correlation and shared inbox behavior

Shared inbox request/reply paths use nonce correlation rules from `RFC-0019`.
The selftest path now uses CAP_MOVE + nonce-matched receive for logd STATS/QUERY checks to avoid false-negative drift under mixed reply traffic.

## Proof markers (QEMU)

Required tracing markers:

- `SELFTEST: tracing spans ok`
- plus metrics security baseline marker:
  - `SELFTEST: metrics security rejects ok`

Markers must only be emitted after exported span-end evidence is queryable through `logd`.
