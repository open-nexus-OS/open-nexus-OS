---
title: TASK-0049C Reliability v1c: persistent evidence journal — bounded logd spill to statefs
status: Draft
owner: @reliability
created: 2026-08-18
depends-on:
  - TASK-0049 # produces the events worth persisting (crash/exhaustion reasons)
  - TASK-0026 # statefs journal v2 txn ops (Done)
follow-up-tasks:
  - TASK-0051B # crash dumps at rest build on surviving evidence
links:
  - Reliability contract: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§5 evidence retention)
  - logd v1: docs/rfcs/RFC-0011-logd-journal-crash-v1.md
  - Structured events: docs/rfcs/RFC-0068-structured-event-observability-subject-grouped-journal-renderer.md
  - statefs txn wire: userspace/statefs/src/protocol/txn.rs (TASK-0026)
  - Retention precedent: tasks/TASK-0014-observability-v2-metrics-tracing.md (WAL/rollup/TTL on /state)
  - Testing contract: scripts/qemu-test.sh
---

## Context

RFC-0087 Phase 2a. logd is a RAM ring — 128 records / 16 KiB, drop-oldest
(`source/services/logd/src/os_lite.rs:69-70`) — and never writes to statefs.
Consequence: the `crash.v1` record execd appends is gone after ~128 log lines
and never survives a reboot. Every honest post-mortem claim, and TASK-0051B's
"correlate dump with recent evidence", needs evidence that outlives the boot.

RFC-0011 explicitly deferred persistence; this task is that deferred slice, on
the substrate that now exists (statefs journal v2 + 2PC txn ops, TASK-0026).
metricsd already proved the retention pattern on `/state` (WAL + rotation +
TTL, TASK-0014) — reuse the shape, not a new retention language.

## Goal

1. **Selective spill, not full mirroring**: logd persists *evidence-class*
   records — `event=crash.v1`, `event=exhaust.v1`, audit records, and
   first-failure markers per RFC-0068 verdict folding — to statefs under
   `/state/logd/`, via the 2PC txn client (`statefs::client`). The bulk debug
   stream stays RAM-only (bounded, cheap).
2. **Bounded**: explicit byte budget + record cap per class, drop-oldest
   on-disk (bounded segments, TTL GC) — budgets in one config surface, no
   magic numbers.
3. **Query**: existing logd QUERY surface gains a `persisted` flag/scope so
   `nx diagnose` (TASK-0051) and selftests can read back post-reboot evidence
   through the same wire protocol — no second query API.
4. **Crash-safe by construction**: spill writes ride journal v2 txns; a torn
   spill is dropped whole on replay (both-or-neither, proven semantics from
   0026 — no new durability code).

## Non-Goals

- Full persistent journal / log streaming / subscriptions (RFC-0011 deferrals
  stand).
- Crash *dumps* at rest (TASK-0051B; this task persists *records*).
- Remote export (TASK-0040 lane).
- New retention vocabulary — budgets follow the metricsd precedent.

## Constraints / invariants (hard requirements)

- ADR-0043: `/state` is a service-KV — records stay small (bounded fields per
  RFC-0011), never blobs; blobs belong to 0051B's placement decision.
- Spill must be best-effort and non-blocking for emitters: statefsd
  unavailability degrades to RAM-only + ONE deterministic degrade marker
  (`logd: degrade evidence volatile (statefs unavailable)`) — RFC-0087
  exhaustion/degradation discipline applies to logd itself.
- Never persist secrets; scope/field bounds enforced before write (existing
  logd caps).
- Bump-allocator discipline (OS): reuse spill buffers; no per-record `format!`
  or fresh `Vec` per event (known OOM class).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (write amplification)**: evidence events are rare by design; if a
  runaway emitter floods evidence-class records, the class budget + rate
  limiter (existing `security.rs` limiter) caps it — flood becomes an
  `exhaust.v1` event about logd itself, not disk death.
- **YELLOW (ordering)**: spill happens on logd's thread after ring append;
  crash-of-logd loses at most the unspilled tail — acceptable and documented
  (the alternative, synchronous spill per record, violates the bounded-cost
  constraint).

## Contract sources (single source of truth)

- Evidence classes + budgets: RFC-0087 §5.
- Wire/txn: `userspace/statefs/src/protocol/txn.rs` (frozen framing).
- Query surface: RFC-0011 protocol (additive scope only).

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p logd`: spill selection (evidence-class only), budget
  enforcement + on-disk drop-oldest, torn-write replay drops whole txn
  (SpyDevice pattern from statefs crash-injection reused), query `persisted`
  scope roundtrip, `test_reject_*` for oversized/foreign-scope evidence.

### Proof (OS/QEMU) — required

- `logd: evidence persist on (budget=<n>KiB)`
- `SELFTEST: evidence persist ok` — induced crash (0049 fault child) →
  reboot via double-boot lane → crash record queryable post-boot:
  `NEXUS_KEEP_BLK=1 REQUIRE_STATEFS_COLD_BOOT=1`, two runs.
- `SELFTEST: evidence budget ok` — flood knob → budget cap + degrade event,
  no disk growth beyond cap.

## Touched paths (allowlist)

- `source/services/logd/src/` (spill module, query scope, markers)
- `userspace/statefs/` (client use only — no engine changes)
- `source/apps/selftest-client/` (persist + budget proofs)
- `source/apps/selftest-client/proof-manifest/markers/` + `scripts/qemu-test.sh`
- `docs/reliability/` (evidence retention section)

## Plan (small PRs)

1. Spill module host-first (selection, budgets, txn writes, replay tests).
2. Query scope + host roundtrip.
3. OS wiring + markers + double-boot proof; `just test-all`.
