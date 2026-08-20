---
title: TASK-0049C Reliability v1c: persistent evidence journal — bounded logd spill to statefs
status: Done (2026-08-20 — PR-C1 host + PR-C2 OS QEMU-proven incl. keep-blk double boot; test-all green incl. smp1; see the systemic-bugs section)
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

1. ✅ **PR-C1 (2026-08-20) — pure halves, host-first**: `evidence.rs`
   (classifier: `event=crash.v1|exhaust.v1` scanned in fields AND message —
   execd and statefsd carry the tag in different places; `.audit` scope
   suffix; exact-match only, lookalikes rejected; **allow-audits stay
   RAM-only** — every spill PUT triggers a policyd check whose ALLOW audit
   lands back in logd, persisting it would self-sustain). `spill.rs` (slot
   ring `/state/logd/evidence/slot_NN` + `head` sequence; 32 × ≤512 B ⇒ the
   16 KiB budget holds BY CONSTRUCTION, drop-oldest IS the rotation — no GC
   walker per ADR-0043). Additive QUERY source byte (v1 len 15 / v2 len 23;
   absent = RAM, old frames stay valid); the persisted mirror is a second
   `Journal` so ONE renderer serves both worlds. 14 host tests
   (`tests/evidence_spill.rs`; the grandfathered suite may not grow).
2. ✅ **PR-C2 (2026-08-20) — OS wiring + double-boot proof**: txn helpers on
   the shared `statefs::client::StatefsClient` (begin/put/commit/abort —
   reusable, no hand-rolled frames); `spill_os.rs` (lazy attach = ONE head
   GET; **incremental mirror load**, one slot GET per quiet tick;
   **pending-queue decoupling** — the request handler only classifies and
   enqueues, spill I/O runs BETWEEN requests like statefsd's
   compaction_tick; **arming**: the apparatus arms on the first
   crash/exhaust event, audit records alone never arm it — a preserved
   -image boot replays statefsd audits seconds after ready, and eager
   attach in that window deterministically parked the boot; terminal
   degrade after 4 consecutive txn failures — each failure mints a deny
   audit which is itself evidence, retrying forever would echo). logd
   ServiceSpec: `reply_inbox` + declarative statefsd route (CAP_MOVE
   inbox — never the shared response queue); `policies/base.toml` grant;
   `route_os.rs` split keeps the os_lite ratchet (791→802 < 818). Proofs:
   `SELFTEST: evidence query ok` (persisted scope finds crash.v1),
   `SELFTEST: evidence budget ok` (40-record flood, LIST count ≤ 33),
   `logd: evidence persist on (loaded=0x16)` on the keep-blk boot (the
   honest cross-boot count), all gated incl. the cold-boot lane.

## Systemic bugs found and fixed on the way (all end-state fixes)

1. **Cross-service wait TRIANGLE**: statefsd blocks on policyd (cap check),
   policyd blocked on logd (bounded audit-ACK wait), logd blocked on
   statefsd (spill txn) — bounded timeouts composed into fail-closed DENIES
   for uninvolved services (settingsd/rngd/keystored/selftest). Broken at
   two edges: policyd `emit_audit` is now fire-and-forget + inbox drain
   (the ACK wait only served inbox hygiene); logd spill decoupled (above).
2. **metricsd retention bulk path had NO reply consumer**
   (`new_with_slots(send, 0)`) — every PUT/DEL reply rotted on statefsd's
   shared response queue (the 0049B wedge class). Now CAP_MOVE onto
   metricsd's OWN inbox + drain-before-send.
3. **statefsd txn gate checked `statefs.boot` FIRST** — every ordinary txn
   writer earned a policyd DENY audit per op; for the spill that deny audit
   was itself evidence (1:1 loop). Order flipped (write first, OR-semantics
   identical).
4. **statefsd bump-heap death**: 384 KiB exhausted on preserved-image boots
   (journal replay + ~40 compaction generations + evidence churn on the
   never-freeing bump) — alloc-fail killed the store mid-proof. Now
   `heap-1m`; the honest fix for the per-request Vec churn is buffer reuse
   (follow-up below).
5. **Query pagination LOST records (wire bug since RFC-0011)**: the bounded
   encoder SKIPPED records that did not fit the 512 B page while paging
   resumes past max_ts — a skipped record was never served again (crash.v1
   vanished in the denser keep-blk ring → `SELFTEST: crash report FAIL`).
   Now `break` (rest goes to the next page); the store caps guarantee any
   single record fits an empty page, so nothing is ever lost.
   Also: the boot-loaded mirror is renumbered onto synthetic monotonic
   timestamps (last boot's REAL timestamps are higher than this boot's
   early ones — keeping them made ts-paging skip everything new); original
   timestamps stay ON DISK in the slot layout for TASK-0051's diag surface.

## DoD reconciliation (documented recuts)

- "`SELFTEST: evidence persist ok`" → the cross-boot truth is the LOADER's
  count (`logd: evidence persist on (loaded=0x<n>)`, gated > 0 on the
  cold-boot lane): record ids/timestamps are boot-local, so a client-side
  cross-boot assert would have required wire surface with no other user.
  The selftest instead proves the query path + same-boot spill E2E
  (`SELFTEST: evidence query ok`).
- "budgets in one config surface" → the budget is two named constants
  (`MAX_SLOTS`, `SLOT_VALUE_CAP`) enforced by construction; a config file
  for a ring size nobody tunes would be dead surface (metricsd's TOML
  budget precedent covers runtime-tunable retention, this is not that).
- "torn-write replay drops whole txn (SpyDevice reuse)" → both-or-neither
  is a JOURNAL-ENGINE property proven by TASK-0026's crash-injection suite;
  logd's host tests pin the txn framing order (slot before head, one txn),
  not a re-proof of statefsd's replay.
- "`logd: evidence persist on (budget=<n>KiB)`" → the marker carries
  `loaded=0x<n>` instead: the budget is a compile-time constant (nothing to
  announce), the loaded count is the cross-boot evidence the lane gates on.

Follow-ups recorded: (1) statefsd per-request Vec churn → buffer reuse
(bump-allocator class; heap watermark markers keep the ceiling visible);
(2) `nx diagnose` (TASK-0051) reads slots directly for original timestamps;
(3) metricsd's own inbox depth vs. burst size is tight (8 vs ~6) — revisit
with the retention rework.
