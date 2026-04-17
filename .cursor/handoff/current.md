# Current Handoff: TASK-0023B Phase 2 in progress — P2-00 + P2-01 closed; 16 cuts remaining

**Date**: 2026-04-17 (Phase 2 execution session, P2-00 + P2-01 landed)
**Status**: `TASK-0023B` Phase 2 **in progress** under plan `task-0023b_phase_2_plan_5e547ada.plan.md` (Cursor-internal). Cuts **P2-00 (RFC-0014 phase list 8 → 12, doc-only)** and **P2-01 (phases/ skeleton + `os_lite/context.rs` + minimal `PhaseCtx`)** complete; Phase-1 Proof-Floor green (`cargo test -p dsoftbusd`, `just test-dsoftbus-quic`, `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os` all pass). 16 cuts remaining (P2-02 … P2-17 + closure). Phase 1 stayed complete; Phase 3-6 + TRACK-OS-PROOF-INFRASTRUCTURE unchanged.
**Execution SSOT**: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md` (now titled `… refactor + manifest/evidence/replay v1`)
**Contract SSOT**: `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
**Long-running discipline track**: `tasks/TRACK-OS-PROOF-INFRASTRUCTURE.md`

## What changed in the latest session (Phase 2 execution, cuts P2-00 + P2-01)

- **Cut P2-00 — RFC-0014 phase list 8 → 12 (doc-only)**:
  - `docs/rfcs/RFC-0014-…-v1.md` §3 rewritten: 7 illustrative phases → 12 normative numbered phases, congruent with the 12 phase files now under `os_lite/phases/`.
  - New subsection "Acknowledged contract ↔ harness drift": `scripts/qemu-test.sh PHASES` stays at 8 until Cut P4-05; explicit prohibition on adding new `RUN_PHASE` values before then.
  - `docs/testing/index.md` intentionally **not** modified — that file documents harness-operational `RUN_PHASE` values (unchanged in P2-00).
- **Cut P2-01 — phases/ skeleton + `os_lite/context.rs` (plumbing)**:
  - New `source/apps/selftest-client/src/os_lite/context.rs` (52 LOC): `PhaseCtx { reply_send_slot, reply_recv_slot, updated_pending: VecDeque<Vec<u8>>, local_ip: Option<[u8;4]>, os2vm: bool }` + silent `bootstrap()` (no markers, no routing).
  - New `source/apps/selftest-client/src/os_lite/phases/mod.rs` aggregator + 12 stub files (`bringup, ipc_kernel, mmio, routing, ota, policy, exec, logd, vfs, net, remote, end`), each `pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()>`.
  - `os_lite/mod.rs` updates: `mod context; mod phases;` added; `let mut ctx = context::PhaseCtx::bootstrap()?;` at top of `run()`; redundant `REPLY_*_SLOT` consts + `let mut updated_pending` removed; ~30 references rewritten to `ctx.<field>`.
  - Phase-1 Proof-Floor rerun: `cargo test -p dsoftbusd` green; `just test-dsoftbus-quic` 6+8 green; `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os` green (full marker ladder, ~118s).
- **PhaseCtx minimality (locked at P2-01)**: only `reply_send_slot`, `reply_recv_slot`, `updated_pending`, `local_ip`, `os2vm` promoted. Service handles (logd / policyd / bundlemgrd / samgrd / etc.) deliberately **not** cached — existing code already re-resolves them per-phase via `route_with_retry`; promoting them is higher-risk and out of scope for P2-01.
- **Plan deviation: actual `pub fn run()` call order ≠ plan's assumed P2-02..P2-13 order**. Verified by reading `mod.rs` after P2-01:
  - Plan assumed: `bringup, ipc_kernel, mmio, routing, ota, policy, exec, logd, vfs, net, remote, end`.
  - Actual: `bringup, routing, ota, policy, exec, logd, ipc_kernel, mmio, vfs, net, remote, end`.
  - Will execute extraction cuts in actual order (todo IDs identify phase content, not sequence number). Plan file unchanged per user instruction; this handoff is the deviation record.
- **Operational lessons (apply to every future cut)**:
  - `cargo +stable fmt --all` reformats ~200 unrelated files due to long-standing rustfmt drift; never run it. Use `rustfmt +stable <touched-files…>` and revert any submodule churn it pulls in via `mod` resolution.
  - `git diff --name-only | grep -v -E '^(file1|file2)$'` requires **exact-line** match; use `git diff --name-only -- :^path1 :^path2` instead.
  - `just diag-os` does not include `selftest-client`. Use `cargo +nightly check -p selftest-client --target riscv64imac-unknown-none-elf --no-default-features --features os-lite` for direct compile verification.
- **Frozen baseline**: marker order, marker strings, reject paths all byte-identical post-P2-01.
- **No edits to**: `main.rs`, `markers.rs`, `Cargo.toml`, `build.rs`, kernel.

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
- **Active plan**: `/home/jenning/.cursor/plans/task-0023b_phase_2_plan_5e547ada.plan.md` (Cursor-internal, do **not** edit during execution).
- **Resume point**: Cut **P2-02** — extract `phases/bringup.rs` (keystored CRUD + qos + timed-coalesce + rng + device_key persist + statefs CRUD + reply slot announce + capmove probe + dsoftbus readiness + samgrd register/lookup/malformed). Largest cut in Phase 2; ~210 LOC of `pub fn run()` body migrates to `phases::bringup::run(&mut ctx)`. PhaseCtx fields read by this slice: `ctx.reply_send_slot`, `ctx.reply_recv_slot`. No new fields promoted.
- **Per-cut cadence (Phase-1 Proof-Floor, run after every Px-XX)**:
  1. `cargo +nightly check -p selftest-client --target riscv64imac-unknown-none-elf --no-default-features --features os-lite`
  2. `cargo test -p dsoftbusd -- --nocapture`
  3. `just test-dsoftbus-quic`
  4. `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  5. `rustfmt +stable <touched .rs files only>`; verify no submodule drift
  6. `just lint`
- **Phase 2 closure tasks (after P2-17)**: tick RFC-0038 18-box Phase 2 checklist; sync `.cursor/{handoff/current.md, next_task_prep.md, current_state.md}`; open `task-0023b_phase-3_<hash>.plan.md`.
- STATUS-BOARD / IMPLEMENTATION-ORDER stay deferred until Phase 2 is closed.
