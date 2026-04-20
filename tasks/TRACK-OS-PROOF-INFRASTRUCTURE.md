---
title: TRACK OS proof infrastructure — observability + coverage + discipline (post-Phase-6 capabilities)
status: Draft
owner: @runtime
created: 2026-04-17
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Foundational task (delivers manifest + evidence + replay): tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md
  - Foundational RFC: docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md
  - Phase contract: docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md
  - Testing index: docs/testing/index.md
---

## Goal (track-level)

Once `TASK-0023B` Phases 4–6 deliver:

- a manifest-driven proof contract,
- signed evidence bundles per QEMU run,
- a replay-and-bisect toolchain,

this track turns those foundations into **long-running discipline capabilities** that keep the deterministic-proof story honest as the OS grows.

The track exists because the foundation alone (Phases 4–6) is necessary but not sufficient. Without the workstreams below, the proof system stays *internally usable* and *externally provable* but does not actively *prevent* drift, regressions, or hidden cost growth. This track is the active-prevention layer.

## Why this track exists (no other OS does this)

Existing OS testing models we benchmarked:

- **Linux**: scripted boot tests, KASAN/KCSAN, no marker contract, no portable evidence per run.
- **Fuchsia**: structured component test framework, but proof artifacts are not signed-replayable across machines.
- **seL4**: formal proof of kernel core, but userspace test discipline is project-specific.
- **iOS / macOS**: signed test runs internal to Apple; not externally provable.

None of them combines: (a) a single manifest as marker SSOT, (b) signed per-run evidence, (c) deterministic replay, (d) per-phase observability budgets, (e) measured capability coverage. This track is the part beyond Phase 6 that makes the combination self-policing.

## Scope boundaries (anti-drift)

- This track does **not** modify the manifest, evidence-bundle layout, replay tooling, or selftest-client architecture. Those are owned by `TASK-0023B`.
- This track does **not** introduce new transport features, kernel APIs, or service semantics. It consumes the proof infrastructure and adds discipline gates around it.
- Each candidate below extracts into a real `TASK-XXXX` only after `TASK-0023B` Phase 6 closure.
- Candidates are intentionally not yet planned in cut detail; their scope and proof shape will be locked when extracted.

## Architectural stance

Three independent workstreams (B, C, D), each with its own quality bar and proof shape. They share one input (the evidence-bundle stream from Phases 5/6) but do not depend on each other.

- **B — Observability & Performance contracts**: makes runs not just "pass/fail" but quantified.
- **C — Coverage as Measured Property**: makes "we tested it" measurable, not anecdotal.
- **D — Discipline & Process**: makes drift visible the moment it happens, not at the next quarterly review.

## Workstream B — Observability & Performance contracts

### Problem

Today a run is binary: green or red. We have no per-phase budget, no structured trace, no failure classification. A run that takes 200s vs 220s vs 350s is "pass" in all three cases until something hits the wallclock cap. There is no way to claim "this build regresses bring-up by 18%".

### Capability

- **Per-phase instruction-count and wallclock budgets**: each phase entry in `proof-manifest.toml` gains optional `[phase.X.budget] icount_max = ...` and `wallclock_ms_max = ...`. The `nexus-evidence` `trace.jsonl` records observed values. Verifier flags overruns.
- **Structured trace with stable error classes**: replace ad-hoc UART log scraping with a typed `TraceEvent` enum (Phase = Start | End | Skip | Marker { name } | Error { class, code }) where `class` is a stable enum (`TimedOut`, `RouteNotFound`, `DeniedByPolicy`, `InvalidFrame`, `OversizedInput`, `BadSignature`, `SlotMismatch`, `UnexpectedReplyOpcode`, etc., aligned with RFC-0014 §4).
- **Failure-mode catalog**: `docs/testing/failure-modes.md` enumerates every classified failure with: stable code, root-cause class, expected probe, recovery procedure. CI rejects runs whose `Error.class` is `Unknown`.
- **Performance regression gate**: a budget overrun on the CI profile fails the run; on developer profiles it warns but does not fail (configurable per profile).

### Candidate tasks

- **CAND-OBS-010**: per-phase budget schema + manifest extension + `trace.jsonl` field.
- **CAND-OBS-020**: `TraceEvent` enum + structured emission + UART-side parser.
- **CAND-OBS-030**: failure-mode catalog (initial enumeration) + `Unknown` class rejection in CI.
- **CAND-OBS-040**: performance regression gate (CI profile) + per-profile budget overrides.

### Hard gates (when extracted)

- `Unknown` failure class is a hard CI failure.
- Budget overrun on `profile=full` (CI) is a hard CI failure; on dev profiles, warning only.
- New marker without a budget entry on `profile=full` is rejected at manifest parse time (after a one-cycle grace).

## Workstream C — Coverage as Measured Property

### Problem

"We test the OS" is currently true but not measurable. We have no answer to: "what fraction of the kernel's syscall surface is exercised by the deterministic ladder?", "what fraction of policyd's deny paths are proven?", "which capability operations have never been called in a passing CI run?".

### Capability

- **Capability coverage**: a build-time analysis of which kernel syscalls and capability operations are called from any phase of the manifest. Output: `target/coverage/capability-coverage.json`. Floor: ≥ 80% of public capability ops touched by `profile=full` (initial floor, raise over time).
- **Parser fuzz corpus**: every parser exposed to untrusted input (manifest, RSS, IPC frames, ABI types) gets a corpus + a structured fuzz harness. Corpus lives in `tests/fuzz-corpus/<parser>/`. CI runs ≥ N iterations per parser per run on `profile=full`.
- **Backward-compatibility ABI matrix**: `nexus-abi` types get a versioned snapshot file per minor release. CI fails when current types deviate from the latest snapshot without an explicit ADR-marked breaking-change record.

### Candidate tasks

- **CAND-COV-010**: capability-coverage analyzer (host-only crate); initial 80% floor on `profile=full`.
- **CAND-COV-020**: structured fuzz corpus + harness for `nexus-proof-manifest` parser (Phase-4 deliverable seeds the first corpus).
- **CAND-COV-030**: structured fuzz corpus + harness for IPC frame parsers + DSoftBus parsers.
- **CAND-COV-040**: ABI snapshot file format + CI gate; backfill snapshot for current `nexus-abi`.

### Hard gates (when extracted)

- Coverage floor regression = hard CI failure.
- Fuzz corpus reduction (file deletion) = hard CI failure unless ADR-recorded.
- ABI snapshot drift without ADR = hard CI failure.

## Workstream D — Discipline & Process

### Problem

Drift accumulates between deliberate decisions. A `unwrap` slips into a daemon. A second source of truth for a marker re-emerges. A non-deterministic helper lands in a phase file. A flaky test gets retried instead of fixed. By the time review notices, the drift is normalized.

### Capability

- **Determinism audit lint**: a `cargo clippy`-style lint pass (custom `nexus-discipline` lint crate) that flags: unbounded loops without explicit budget, `Instant::now()` outside an allowed allowlist, `rand::random()` outside test/bringup contexts, `unwrap`/`expect` inside daemon production paths, marker string literals outside the generated file.
- **Flake response runbook + SLO**: any test (host or QEMU) that fails non-deterministically must be either (a) fixed within 7 days or (b) explicitly marked `#[ignore = "tracked: <issue>"]`. CI tracks the flake count per week as a quality SLO; SLO breach triggers a stop-the-line review.
- **Marker-string drift detector**: a daily CI job that scans for new occurrences of `"SELFTEST: "` / `"dsoftbusd: "` / `"<svc>: ready"` outside the allowed locations and opens an issue.
- **Evidence-bundle review hook**: PR template requires linking the evidence bundle hash for the merge candidate. PR cannot merge without a verified bundle from the relevant profile (`full` for trunk merges, `quick` for docs-only).

### Candidate tasks

- **CAND-DSC-010**: `nexus-discipline` lint crate skeleton + first 5 lints (unbounded loop, `Instant::now`, `rand::random`, `unwrap` in daemon, marker literal location).
- **CAND-DSC-020**: flake-tracking dashboard + SLO definition + stop-the-line trigger.
- **CAND-DSC-030**: marker-string drift detector + daily CI job + auto-issue.
- **CAND-DSC-040**: PR template change + merge-gate hook for verified evidence bundle.

### Hard gates (when extracted)

- New `unwrap`/`expect` in `source/services/**` daemon path = lint failure (unless `// SAFETY: …` justification).
- New marker string literal outside the allowed location = lint failure.
- PR merge without linked verified evidence bundle = merge-gate failure.
- Flake SLO breach = stop-the-line (no merges) until the runbook step is executed.

## Sequencing

- **Precondition for all candidates**: `TASK-0023B` Phase 6 closure. **Status (2026-04-20)**: Phase 6 is functionally closed; `TASK-0023B` is `In Review`; `RFC-0038` is `Done`. The single remaining environmental closure step (external CI-runner replay artifact for P6-05, see `docs/testing/replay-and-bisect.md` §7-§11) is not blocking for *track-level extraction planning*, but candidate `TASK-XXXX` extraction must wait until that artifact lands and the documented status flip is applied across task / RFC / status docs.
- **B vs C vs D ordering**: independent. B and D are higher-leverage in the short term (they actively prevent regression). C is higher-leverage in the medium term (it makes the proof system measurable, which feeds external claims).
- **No cross-dependencies between candidates within a workstream** unless explicitly noted; e.g. CAND-COV-020 and CAND-COV-030 can land in either order.

## Out of scope (for this track)

- Marker manifest changes — owned by `TASK-0023B` Phase 4.
- Evidence-bundle layout changes — owned by `TASK-0023B` Phase 5.
- Replay/bisect tool changes — owned by `TASK-0023B` Phase 6.
- Service-internal correctness — owned by each service's task.
- Kernel changes — out of scope by design.

## Stop conditions (track-level)

This track is "done" when:

1. ≥ 1 candidate from each workstream (B, C, D) has been extracted into a real `TASK-XXXX` and closed.
2. The hard gates listed for each closed candidate are mechanically enforced in CI.
3. A new contributor can read `docs/testing/index.md` and understand: what the manifest is, how to verify a bundle, how to replay a failure, what the budget/coverage/discipline floors are, and how to extend each.

The track itself does not need to fully complete — it remains a backlog umbrella as long as the discipline categories remain relevant. Closure of individual candidates is the meaningful unit.
