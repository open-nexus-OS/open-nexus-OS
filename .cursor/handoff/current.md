# Current Handoff: TASK-0023B Phase 1 closed; Phase 2–6 architecture + manifest/evidence/replay documented

**Date**: 2026-04-17 (re-scoped session)
**Status**: `TASK-0023B` Phase 1 **complete** (Cuts 0–22 merged; `main.rs` frozen at 122 LOC; `os_lite/mod.rs` shrunk from ~6771 → 1226 LOC). Phase 2/3 **architecture refinements** locked in `RFC-0038`. Task scope **expanded** to include Phase 4 (manifest as SSOT + profile-aware harness + runtime selftest profiles), Phase 5 (signed evidence bundles), Phase 6 (replay capability). New `TRACK-OS-PROOF-INFRASTRUCTURE.md` covers long-running discipline workstreams (B/C/D) that consume the Phase 4–6 foundations.
**Execution SSOT**: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md` (now titled `… refactor + manifest/evidence/replay v1`)
**Contract SSOT**: `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
**Long-running discipline track**: `tasks/TRACK-OS-PROOF-INFRASTRUCTURE.md`

## What changed in the latest session
- Cuts 19–22 executed under plan `task-0023b_cuts_19-22_914398f4.plan.md`:
  - Cut 19 → `os_lite/updated/mod.rs` (`SYSTEM_TEST_NXS` const + `SlotId` enum + 9 `updated_*`/`init_health_ok` fns)
  - Cut 20 → `os_lite/probes/ipc_kernel/mod.rs` (8 IPC-kernel/security probes)
  - Cut 21 → `os_lite/probes/elf.rs` (`log_hello_elf_header`; `read_u64_le` reduced to file-private)
  - Cut 22 → `emit_line` shim removed in `os_lite/mod.rs`; replaced with direct `use crate::markers::emit_line`
- Phase-1 Proof-Floor rerun after every cut; full marker ladder unchanged; no fallback markers.
- Hygiene at session end: `just fmt-check` and `just lint` green.
- No edits to `main.rs`, `markers.rs`, `Cargo.toml`, or `build.rs` this session.
- **Architecture review (Phase 2/3)**: 9 normative refinements captured in `RFC-0038 → Phase-2/3 architectural refinements (post-Phase-1 review, 2026-04-17)`.
- **Scope expansion (Phase 4–6)**: A1 Marker-Manifest as SSOT, A2 Signed Evidence Bundle, A3 Replay Capability promoted from "future Apple-grade goals" to formal Phase 4–6 of `TASK-0023B` with hard gates + per-cut proof floors. Documented in `RFC-0038` ("Phase 4 — Marker-Manifest + profile dimension", "Phase 5 — Signed evidence bundles", "Phase 6 — Replay capability") and in the task's Execution phases section.
- **TRACK created**: `TRACK-OS-PROOF-INFRASTRUCTURE.md` for long-running B/C/D workstreams (Observability/Performance, Coverage as measured property, Discipline/Process). Each workstream has 4 candidate tasks; precondition is `TASK-0023B` Phase 6 closure.
- **TASK-0024 sequencing**: now blocked on `TASK-0023B` **Phase 4 closure** (was Phase 3). Reason: TASK-0024's new markers must land directly into the profile-aware manifest with `emit_when = { profile = "quic-required" }`; landing them before Phase 4 would create another two-truth surface.
- **RFC-0014 phase list**: extended 8 → 12 in Phase 2 (Cut P2-00) so harness phases and code phases are congruent — precondition for Phase 4 manifest.

## Phase-2/3 architecture (locked in RFC-0038, unchanged)
Captured as 9 normative refinements:

1. **Two-axis structure (Capabilities × Phases)**: capability nouns kept; new orchestration verbs under `os_lite/phases/{bringup, ipc_kernel, mmio, routing, ota, policy, exec, logd, vfs, net, remote, end}.rs`. `pub fn run()` collapses to ~13 lines.
2. **`PhaseCtx` minimality**: only state read by ≥ 2 phases or determining the marker ladder.
3. **Phase isolation**: `phases/*` MUST NOT import other `phases::*`. Mechanically enforced in Phase 3.
4. **Folder-form heuristic**: `name.rs` is default; `name/mod.rs` only when ≥ 1 sibling exists.
5. **Aggregator-only `mod.rs`**: declarations + re-exports only; `services::core_service_probe*` → `probes/core_service.rs` (Cut P2-17).
6. **Host-pfad symmetry**: extract host `run()` from `main.rs` into `host_lite/` (Cut P3-02).
7. **Mechanical architecture gate**: `scripts/check-selftest-arch.sh` + `just arch-gate` chained into `just dep-gate` (Cut P3-03). Phase 4 extends arch-gate to enforce no marker-string literals outside `markers_generated.rs` + `markers.rs`.
8. **Explicitly rejected**: Rust marker-string SSOT (was rejected pre-Phase-4 because the harness was the SSOT; **now superseded by Phase 4** which makes `proof-manifest.toml` the SSOT and generates Rust constants from it). `trait Phase`, generic `Probe` trait, renaming `os_lite/` — still rejected.
9. **Forward-compatibility check**: TASK-0024 → 1 line in `phases/net.rs` + N marker entries in `proof-manifest.toml`. TRACK-PODCASTS-APP / mediasessd → `services/mediasessd.rs` + `phases/media.rs`.

## Phase 4 — Marker-Manifest as SSOT + profile dimension (NEW)
- `source/apps/selftest-client/proof-manifest.toml` is the single source of truth for: phase list, marker ladder, profile membership, run config (env, runner, extends).
- Profiles unified into the manifest:
  - **Harness profiles** (drive `scripts/qemu-test.sh` / `tools/os2vm.sh`): `full`, `smp`, `dhcp`, `os2vm`, `quic-required`.
  - **Runtime profiles** (drive `selftest-client` via `SELFTEST_PROFILE` env / kernel cmdline): `full`, `bringup`, `quick`, `ota`, `net`, `none`. Selects subset of the 12 code phases at runtime; no recompile.
- Markers gain `emit_when` / `emit_when_not` / `forbidden_when` fields for profile-conditional behavior.
- `build.rs` generates `markers_generated.rs` from the manifest; `arch-gate` enforces no marker string literal outside `markers_generated.rs` + `markers.rs`.
- `scripts/qemu-test.sh` and `tools/os2vm.sh` rewritten to consume the manifest via a small host CLI (`nexus-proof-manifest list-markers --profile=… / list-env --profile=…`).
- All `just test-*` recipes route through `just test-os PROFILE=…`. Old recipes alias for ≥ 1 cycle, then deleted.
- Hard gates: no marker string literal outside generated file; no `REQUIRE_*` env in `just test-*`; manifest parser rejects unknown keys; deny-by-default analyzer (any unexpected runtime marker for active profile = hard failure).
- 10 cuts (P4-01 … P4-10).

### Host tests stay outside the manifest
- `cargo test --workspace`, `just test-host`, `just test-e2e`, `just test-dsoftbus-quic` are intentionally not part of the manifest. They prove host-resident logic, not OS-attested behavior. Collapsing both into one model would force a wrong abstraction onto either side.

## Phase 5 — Signed evidence bundle (NEW)
- Each `just test-os PROFILE=…` writes `target/evidence/<utc>-<profile>-<git-sha>.tar.gz`: manifest + uart.log + trace.jsonl + config.json + signature.bin (Ed25519).
- New host-only crate `nexus-evidence` owns canonicalization / sign / verify; reuses `nexus-noise-xk` Ed25519 primitives.
- Two key labels: `ci` and `bringup`. Bringup-key bundles **must not validate** against CI policy.
- `tools/verify-evidence.sh` validates fail-closed; reject tests cover tampered manifest / uart / trace / key swap.
- Successful run without sealed bundle = CI failure. Failed run still produces a bundle (replay-only artifact, no signature).
- 6 cuts (P5-01 … P5-06).

## Phase 6 — Replay capability (NEW)
- `tools/replay-evidence.sh <bundle>` deterministically re-runs a captured profile from a stored bundle.
- `tools/diff-traces.sh` produces a stable diff (classifies: `exact_match | extra_marker | missing_marker | reorder | phase_mismatch`).
- `tools/bisect-evidence.sh` walks git-SHA range with mandatory `--max-commits` + `--max-seconds` bounds.
- Cross-host determinism floor: replay must be empty-diff on CI runner + ≥ 1 dev box for the same bundle. Documented allowlist (wall-clock, qemu version banner, hostname) is append-only with reviewer signoff.
- 6 cuts (P6-01 … P6-06).

## TRACK-OS-PROOF-INFRASTRUCTURE (NEW; precondition: TASK-0023B Phase 6 closure)
Three independent workstreams. Each candidate extracts into a real `TASK-XXXX` after Phase 6 closure.

- **B — Observability & Performance contracts**:
  - CAND-OBS-010 per-phase `icount`/wallclock budgets in manifest.
  - CAND-OBS-020 structured `TraceEvent` enum + stable error classes.
  - CAND-OBS-030 failure-mode catalog; `Unknown` class rejection in CI.
  - CAND-OBS-040 perf regression gate on CI profile.
- **C — Coverage as Measured Property**:
  - CAND-COV-010 capability-coverage analyzer; ≥ 80% floor on `profile=full`.
  - CAND-COV-020 fuzz corpus + harness for `nexus-proof-manifest`.
  - CAND-COV-030 fuzz corpus + harness for IPC frames + DSoftBus.
  - CAND-COV-040 ABI snapshot file format + CI gate.
- **D — Discipline & Process**:
  - CAND-DSC-010 `nexus-discipline` lint crate.
  - CAND-DSC-020 flake-tracking dashboard + SLO + stop-the-line.
  - CAND-DSC-030 marker-string drift detector (daily CI).
  - CAND-DSC-040 PR template + merge-gate for verified evidence bundle.

## Refined cut sequences (locked)
- **Phase 2** (18 cuts): P2-00 (RFC-0014 phase list 8 → 12), P2-01 (skeleton + `PhaseCtx`), P2-02 … P2-13 (12 phases extracted, one per cut), P2-14 (`updated/` sub-split), P2-15 (`probes/ipc_kernel/` sub-split), P2-16 (`ipc/reply_inbox.rs` DRY), P2-17 (aggregator-only `services/mod.rs`).
- **Phase 3** (4 cuts): P3-01 (flatten single-file `name/mod.rs`), P3-02 (`host_lite/` extract), P3-03 (`arch-gate`), P3-04 (standards review).
- **Phase 4** (10 cuts): P4-01 … P4-10 (manifest crate, schema, populate, generate constants, harness consumers, recipe migration, os2vm, runtime profiles, deny-by-default analyzer, deprecate direct `REQUIRE_*`/`RUN_PHASE`).
- **Phase 5** (6 cuts): P5-01 … P5-06 (`nexus-evidence` crate, extract-trace, seal, hook into qemu-test, verify, docs).
- **Phase 6** (6 cuts): P6-01 … P6-06 (replay, diff, bisect, regression-bisect wrapper, cross-host floor, docs).

**Total**: ~44 cuts after Phase 1.

## Frozen baseline that must stay green (verified after every cut)
- Host:
  - `cargo test -p dsoftbusd -- --nocapture`
  - `just test-dsoftbus-quic`
  - Phase 4+: `cargo test -p nexus-proof-manifest -- --nocapture`
  - Phase 5+: `cargo test -p nexus-evidence -- --nocapture`
- OS (Phase 1–3):
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
- OS (Phase 4+):
  - `just test-os PROFILE=full` (replaces the above)
  - `just test-os PROFILE=smp` (replaces `just test-smp`)
  - `just test-os PROFILE=quic-required` (replaces `REQUIRE_DSOFTBUS=1 …`)
  - `just test-os PROFILE=os2vm` (replaces `just test-dsoftbus-2vm`)
  - `just test-os PROFILE=bringup` (runtime profile; short-circuits before `routing`)
  - `just test-os PROFILE=none` (runtime profile; only `SELFTEST: end`)
- Required QUIC subset markers (`profile=quic-required`):
  - `dsoftbusd: transport selected quic`
  - `dsoftbusd: auth ok`
  - `dsoftbusd: os session ok`
  - `SELFTEST: quic session ok`
- Forbidden fallback markers under `profile=quic-required`:
  - `dsoftbusd: transport selected tcp`
  - `dsoftbus: quic os disabled (fallback tcp)`
  - `SELFTEST: quic fallback ok`
- Hygiene: `just dep-gate && just diag-os && just fmt-check && just lint`
- Phase 3+: `just arch-gate` (chained into `just dep-gate`)
- Phase 5+: `tools/verify-evidence.sh target/evidence/<latest>` returns 0

## Boundaries reaffirmed
- Phase 2 is behavior-preserving: same marker order, same proof meanings, same reject behavior.
- Phase 4 may *add* new markers gated by new profiles (e.g. `SELFTEST: smp ipi ok`) but must not rename existing markers.
- `main.rs` stays at 122 LOC throughout Phase 2; only Phase 3 / Cut P3-02 modifies it.
- Visibility ceiling: `pub(crate)`. No new `unwrap`/`expect`. No new dependencies in `Cargo.toml` for selftest-client itself; new host-only crates (`nexus-proof-manifest`, `nexus-evidence`) are separate.
- Do not absorb `TASK-0024` transport features. Do not regress `TASK-0023` to fallback-only marker semantics.
- Single-File-`name/mod.rs` flattening (Cut P3-01) only applies to modules Phase 2 did NOT sub-split.
- No kernel changes across all 6 phases. `SELFTEST_PROFILE` reading from kernel cmdline is a userspace read, not a kernel API change.
- Host-only tests stay outside the proof manifest by design.

## Next handoff target
- **Step 1 (next session, plan-first)**: write `task-0023b_phase-2..6_<hash>.plan.md` encoding all ~44 cuts (P2-00 … P6-06) with per-cut Proof-Floor cadence, hard-gate tables per phase boundary, and explicit dependency between cuts.
- **Step 2 (after plan review)**: open Phase 2 by executing **Cut P2-00** (RFC-0014 phase list 8 → 12; doc-only) followed by **Cut P2-01** (`phases/` skeleton + `os_lite/context.rs` with empty `PhaseCtx::bootstrap()`; plumbing only).
- **Step 3 (per cut)**: marker order frozen, marker strings byte-identical (Phase 2/3); phase-proof-floor green per cut.
- STATUS-BOARD / IMPLEMENTATION-ORDER updates remain deferred until Phase 2 is closed (avoids per-cut drift). At Phase 4 closure, also update `tasks/TASK-0024-…md` to mark it unblocked.
