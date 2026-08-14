---
title: TASK-0318 Storage performance contract + benchmark gate: RFC-0071 perf amendment, bench harness, regression gate, client-side round-trip hygiene
status: Draft
owner: @runtime
created: 2026-08-14
depends-on:
  - TASK-0314
  - TASK-0316
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract to amend: docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md
  - Prior art (UI perf gates, pattern to mirror): tasks/TASK-0143..0145 (perfd/frame trace)
  - Ladder: tasks/TRACK-STASH-USER-DATA-FS.md
---

## Context

The storage stack has **no performance contract anywhere**: RFC-0071/0072, ADR-0043/0044 and the
track contain zero cache/readahead/write-back requirements, zero latency/throughput budgets, no
benchmark harness and no regression gate. The only perf sentence in the whole contract is an
unresolved "group-commit flush interval 5 ms vs 20 ms — measure" (owned by TASK-0316). "The
filesystem is very slow" (user, 2026-08-14) currently cannot even be measured, let alone gated.
On the client side, app-host makes it worse: `collect_recent` is an N+1 readdir fan-out and the
readdir cache is a single slot invalidated by every write
(`source/services/app-host/src/effect_files.rs:86-100,201-256`), against a 250 ms svc deadline.

## Goal

- **RFC-0071 "Performance" amendment** (approval-gated): the normative perf model — cache
  obligations (block cache/readahead existence + boundedness), durability-mode semantics
  (sync vs group-commit, interval from TASK-0316's measurement), and **budgets** for the core
  ops (open/stat/readdir page, small read/write, bulk splice throughput) on the QEMU reference
  profile — honest TCG-relative budgets with headroom, not marketing numbers.
- **Bench harness**: `cargo bench`-style host benches over the nxfs engine (SpyDevice op-count +
  timing) and a deterministic OS selftest lane emitting **real numbers as markers**
  (`SELFTEST: storage bench ok (readdir=<n>us, write4k=<m>us, splice=<x>MBps)` — values printed,
  thresholds asserted; no vague "faster").
- **Regression gate**: the op-count assertions (requests per 4 KiB block, blocks written per
  1-byte write, FLUSHes per txn group) run in `just test-host` so a perf regression is a test
  failure, not a vibe.
- **Client-side round-trip hygiene**: multi-entry readdir cache with targeted (per-path)
  invalidation in app-host; `collect_recent` batched (one query, not N+1); proof that normal FS
  ops stay far under the 250 ms deadline.

## Non-Goals

- Engine/driver optimizations themselves (TASK-0314/0316 own them; this task makes their gains
  contractual and irreversible).
- Wall-clock CI gates on host hardware variance — thresholds are op-count-based where exact,
  time-based only with generous deterministic margins on the pinned QEMU profile.
- UI perf (perfd/HUD tasks own that).

## Constraints / invariants (hard requirements)

- Marker honesty: bench markers print measured values; a threshold miss fails the lane loudly.
- Deterministic first: op-count gates are exact; timing gates are secondary and margin-padded.
- No new cfgs; bench code host-first, OS lane behind the existing selftest architecture
  (TASK-0023B patterns).

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Bench harness runs in CI-safe mode (op-count assertions) and dev mode (timings).
- Op-count regression tests green: requests/4 KiB block, blocks-written/small-write,
  FLUSH/txn-group.
- app-host tests: readdir cache multi-entry + targeted invalidation, batched recent query.

### Proof (OS / QEMU) — required

- `SELFTEST: storage bench ok (…)` with real numbers within budget on the headless profile.
- Existing storage ladder unchanged and green.

### Docs — required

- RFC-0071 perf section merged (approval-gated); `docs/testing/README.md` documents the bench
  lane; budgets recorded with their measurement provenance.

## Touched paths (allowlist)

- `userspace/nxfs/` (benches), `source/services/app-host/src/effect_files.rs`
- `source/apps/selftest-client/`, `scripts/qemu-test.sh`
- `docs/rfcs/RFC-0071-…` (perf amendment; approval-gated), `docs/testing/README.md`,
  `docs/storage/`
