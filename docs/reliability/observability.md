---
schema: 1
title: Observability
role: guide
subsystem: observability
sources: [source/services/logd, source/libs/nexus-log, source/libs/nexus-event, source/services/metricsd, userspace/nexus-metrics, recipes/observability/metrics.toml]
contracts: [RFC-0003, RFC-0011, RFC-0024, RFC-0068, RFC-0087, RFC-0019]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Observability

> **Purpose** — how logs, persistent evidence, metrics and trace spans flow through logd and metricsd, with their bounds and proof markers.
> **Owns** — `source/services/logd/`, `source/libs/nexus-log/`, `source/libs/nexus-event/`, `source/services/metricsd/`, `userspace/nexus-metrics/`
> **Contracts** — [RFC-0003](../rfcs/RFC-0003-unified-logging.md) (logging facade, marker discipline), [RFC-0011](../rfcs/RFC-0011-logd-journal-crash-v1.md) (logd wire protocol, crash report v1), [RFC-0024](../rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md) (metricsd + span export via logd), [RFC-0068](../rfcs/RFC-0068-structured-event-observability-subject-grouped-journal-renderer.md) (subject-grouped verdicts), [RFC-0087](../rfcs/RFC-0087-reliability-failure-model-v1.md) §5 (persistent evidence journal), [RFC-0019](../rfcs/RFC-0019-ipc-request-reply-correlation-v1.md) (nonce correlation)
> **Invariants** — identity is the kernel-attributed `sender_service_id`, never a payload field. Every input is bounded on the wire before it is stored. A marker is emitted only after the behaviour it names happened.
> **Read next** — [crashdump-v2.md](crashdump-v2.md), [recovery-operations.md](recovery-operations.md), [../testing/README.md](../testing/README.md)

## Components

| piece | role |
|---|---|
| `logd` | central sink: bounded RAM journal, persistent evidence ring, per-subject verdict rendering |
| `nexus-log` | facade services log through; `configure_sink_logd(slot)` binds the declared logd leg |
| `nexus-event` | alloc-free verdict math and event vocabulary shared by kernel, `nexus-abi` and logd |
| `metricsd` | bounded counter/gauge/histogram registry, live span table, retention to statefs |
| `nexus-metrics` | producer-side client for metricsd |
| `execd` | appends crash records (`event=crash.v1`) to logd; writes `.nxcd` artifacts ([crashdump-v2.md](crashdump-v2.md)) |

Services that bind the logd sink: bundlemgrd, dsoftbusd, execd, metricsd, updated. policyd appends decision audits on scope `policyd.audit`.

## logd

### Wire

Byte frames `L O <ver> <op>`: `OP_APPEND` (1), `OP_QUERY` (2), `OP_STATS` (3); v2 frames add a nonce (RFC-0019). Status codes: `OK` 0, `MALFORMED` 1, `UNSUPPORTED` 2, `TOO_LARGE` 3, `INVALID_ARGS` 4, `OVER_LIMIT` 5, `RATE_LIMITED` 6 (`source/services/logd/src/protocol.rs`).

A cap-less APPEND is fire-and-forget: logd journals it and produces no reply. Only a sender that moved a reply cap gets an ack.

### Bounds

| bound | value | where |
|---|---|---|
| wire scope / message / fields | 64 / 256 / 512 B | `protocol.rs` |
| stored scope / message / fields | 32 / 128 / 128 B (truncated) | `journal.rs` |
| RAM journal | 128 records, 16 KiB, drop-oldest | `os_lite.rs` |
| QUERY response | fixed 512 B; stops at the first record that does not fit | `lite_handler.rs` |
| APPEND rate | 24 per sender per second, 64 tracked senders | `security.rs` |

### Persistent evidence

Only evidence-class records persist (`source/services/logd/src/evidence.rs`); the debug stream stays RAM-only.

| class | recognized by |
|---|---|
| crash | `event=crash.v1` |
| exhaustion | `event=exhaust.v1` |
| audit | scope ending `.audit`, except `decision=allow` |

The spill engine (`spill.rs`) writes a fixed ring under `/state/logd/evidence/`: 32 slots (`slot_<n>`) of at most 512 B plus a `head` sequence key. Drop-oldest is the ring rotation, so the byte budget holds by construction. Each spill is one statefs journal-v2 transaction (slot PUT + head PUT, both or neither).

`spill_os.rs` attaches to statefsd lazily on the first evidence append (logd starts first) and loads the on-disk ring into a RAM mirror. A QUERY with source `Persisted` answers from that mirror; logd never does IPC while it owes a response.

| marker | means |
|---|---|
| `logd: evidence persist on (loaded=0x<n>)` | ring attached, `n` records loaded from the previous boot |
| `logd: degrade evidence volatile (statefs unavailable)` | attach retries exhausted; evidence stays RAM-only |

`nx diagnose` decodes the ring host-side into `diagnose/evidence.jsonl` ([recovery-operations.md](recovery-operations.md)).

### Subject verdicts (RFC-0068)

logd groups journal records by `scope` (the subject a record is about, not its emitter) and, once init appends `stage: shell-visible`, renders one verdict line per subject with `nexus_event::render_verdict_line`. Interactive boots only; proof boots keep the raw records.

## Crash records

When a child exits non-zero, execd emits `execd: crash report pid=<pid> code=<code> name=<name>` and appends a logd record (scope `execd`) with fields `event=crash.v1`, `pid`, `code`, `name`, `build_id`, `dump_path` (when a dump exists), `recent_count`, `recent_window_nsec`. Selftests verify crash reporting through logd QUERY, not UART scraping.

## Metrics (metricsd)

| type | semantics |
|---|---|
| counter (u64) | monotonic increment |
| gauge (i64) | last value |
| histogram (u64 ns) | buckets ≤ 1 ms, ≤ 5 ms, ≤ 20 ms, ≤ 100 ms, overflow |

Limits come from `recipes/observability/metrics.toml` (constants in `source/services/metricsd/src/lib.rs`):

| limit | value |
|---|---|
| series total / per metric | 64 / 16 |
| live spans | 64 |
| events per sender per 1 s window | 64, 64 tracked senders |
| metric / span name | 48 B |
| labels / attrs | 192 B |

Rejects: `invalid_args`, `over_limit`, `rate_limited` (spans add `not_found`).

- **Export**: snapshots go to logd through the `nexus-log` sink.
- **Retention**: every accepted metric and span is written to a statefs WAL (`/state/observability/metricsd/wal/seg_<n>`) with 10 s and 60 s rollups (`…/rollup/10s/`, `…/rollup/60s/`) and GC (`source/services/metricsd/src/os_lite/retention.rs`). These writes are fire-and-forget and move no reply cap.
- **Kernel memory**: `nexus_abi::mm_stats` (syscall 61) is exported as gauges (`os_lite/kernel_memory.rs`).

## Tracing (spans)

- `span_start(sender, span_id, trace_id, start_ns, name)` / `span_end(sender, span_id, end_ns, status)` in metricsd.
- IDs are deterministic (no RNG); a `span_id` must match its sender binding, otherwise `invalid_args`. Duplicate starts reject; an unknown end returns `not_found`.
- On a successful end metricsd appends a span-end record to logd: sender, `span_id`, `trace_id`, name, saturating duration, status.
- Spans are local only; no cross-node propagation.

## Proof markers

| area | markers |
|---|---|
| logd | `logd: ready`, `logd: reject invalid_args` / `over_limit` / `rate_limited`, `SELFTEST: logd hardening rejects ok`, `SELFTEST: log query ok`, `SELFTEST: nexus-log sink-logd ok`, `SELFTEST: core services log ok` |
| evidence | `SELFTEST: evidence query ok`, `SELFTEST: evidence budget ok` |
| crash | `execd: crash report pid=…`, `execd: minidump written …`, `SELFTEST: crash report ok`, `SELFTEST: minidump ok` |
| metrics | `metricsd: ready`, `metricsd: reject invalid_args` / `over_limit` / `rate_limited`, `SELFTEST: metrics security rejects ok`, `SELFTEST: metrics counters ok`, `SELFTEST: metrics gauges ok`, `SELFTEST: metrics histograms ok`, `SELFTEST: metrics retention ok`, `metricsd: mm snapshot ok (…)` |
| tracing | `SELFTEST: tracing spans ok` (only after the span-end record is queryable in logd) |

The marker catalog is `source/apps/selftest-client/proof-manifest/markers/logd.toml`.

## Tests

`cargo test -p logd`, `cargo test -p nexus-log`, `cargo test -p metricsd`, `tests/logd_e2e/`; QEMU: `just test-os headless`.
