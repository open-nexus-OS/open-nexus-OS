# Cursor Current State (SSOT)

## Current architecture state
- **last_decision**: `TASK-0023B` Phase 1 **closed** (Cuts 0–22). Task scope **expanded** on 2026-04-17 to include Phases 4–6: Phase 4 (Marker-Manifest as SSOT + profile-aware harness + runtime selftest profiles), Phase 5 (signed evidence bundles per QEMU run), Phase 6 (replay + diff + bisect tooling). Title updated to `TASK-0023B Selftest-Client production-grade deterministic test architecture refactor + manifest/evidence/replay v1`. New track `TRACK-OS-PROOF-INFRASTRUCTURE.md` created for long-running B/C/D discipline workstreams (precondition: Phase 6 closure). `TASK-0024` re-sequenced: now blocked on `TASK-0023B` **Phase 4 closure** (was Phase 3) so TASK-0024's new markers land directly into the profile-aware manifest with `emit_when = { profile = "quic-required" }`. RFC-0014 phase list to be extended 8 → 12 (`bringup → ipc_kernel → mmio → routing → ota → policy → exec → logd → vfs → net → remote → end`) in Cut P2-00 — precondition for the Phase 4 manifest to make harness phases and code phases congruent.
- **active_constraints**:
  - keep `TASK-0021`, `TASK-0022`, `TASK-0023` frozen as done baselines,
  - keep marker honesty strict (`ok`/`ready` only after real behavior),
  - keep Phase 2/3 behavior-preserving (no marker rename, no reordering, same reject behavior, no new `unwrap`/`expect`, visibility ceiling `pub(crate)`, no new dependencies in selftest-client),
  - Phase 4 may *add* new markers via the manifest (e.g. `SELFTEST: smp ipi ok` under `profile=smp`) but must NOT rename existing markers,
  - keep `main.rs` at 122 lines through Phase 2; only Cut P3-02 modifies it (host-pfad extraction to `host_lite/`),
  - keep no_std-safe boundaries explicit (no hidden std/runtime coupling),
  - keep `TASK-0024` blocked on `TASK-0023B` Phase 4 closure; do not reopen TASK-0023 closure semantics,
  - keep `TASK-0044` as follow-up tuning scope (no silent scope absorption),
  - keep host tests (`cargo test --workspace`, `just test-host`, `just test-e2e`, `just test-dsoftbus-quic`) **outside** the proof manifest — different mental model (cargo-tested host logic vs. QEMU-attested OS behavior),
  - no kernel changes across all 6 phases (`SELFTEST_PROFILE` reading from kernel cmdline is a userspace read),
  - Phase 4+: no marker string literal outside `markers_generated.rs` + `markers.rs` (extended `arch-gate` rule),
  - Phase 4+: no `REQUIRE_*` env var read directly in `just test-*` recipes; CI must invoke `just test-os PROFILE=…`,
  - Phase 5+: successful run without sealed evidence bundle = CI failure,
  - Phase 6+: no unbounded replay or bisect runs (`--max-seconds` and `--max-commits` mandatory),
  - defer STATUS-BOARD / IMPLEMENTATION-ORDER updates until Phase 4 closure (per-cut updates create drift); Phase 4 closure also unblocks `TASK-0024` metadata.

## Current focus (execution)
- **active_task**: `TASK-0023B` selftest-client production-grade deterministic test architecture refactor + manifest/evidence/replay — Phase 1 **closed**; Phases 2–6 sequenced (~44 cuts total: P2-00..P2-17, P3-01..P3-04, P4-01..P4-10, P5-01..P5-06, P6-01..P6-06). Phase 2 is next; opens with Cut P2-00 (RFC-0014 phase list extension; doc-only) followed by Cut P2-01 (skeleton).
- **seed_contract**:
  - `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
  - `tasks/TRACK-OS-PROOF-INFRASTRUCTURE.md`
  - `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
  - `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (extended 8 → 12 in Cut P2-00)
  - `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`
  - `docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md`
  - `tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md` (blocked until TASK-0023B Phase 4 closure)
  - `docs/testing/index.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/IMPLEMENTATION-ORDER.md`

## TASK-0023B Phase-1 closure snapshot (2026-04-17)
- `source/apps/selftest-client/src/main.rs` = **122** lines (frozen).
- `source/apps/selftest-client/src/os_lite/mod.rs` = **1226** lines (only top-level imports + `mod`-decls + `pub fn run()` body).
- Extracted modules under `source/apps/selftest-client/src/os_lite/`:
  - `dsoftbus/{quic_os, remote/{mod, resolve, pkgfs, statefs}}`
  - `net/{icmp_ping, local_addr, smoltcp_probe (cfg-gated)}`
  - `ipc/{clients, routing, reply}`
  - `mmio/`, `vfs/`, `timed/`
  - `probes/{rng, device_key, ipc_kernel, elf}`
  - `services/{samgrd, bundlemgrd, keystored, policyd, execd, logd, metricsd, statefs, bootctl}/mod.rs`
  - `services/mod.rs` still hosts shared `core_service_probe*` (to be moved to `probes/core_service.rs` in Cut P2-17)
  - `updated/mod.rs` (`SYSTEM_TEST_NXS` + `SlotId` + 9 fns including `updated_send_with_reply` and `init_health_ok`)
- Behavior-parity gates honored each cut: `pub fn run()` call-order unchanged; marker strings byte-identical; visibility kept at `pub(crate)`.

## Phase-2/3 architecture (locked in RFC-0038, normative)
1. **Two-axis structure**: capability nouns (`services/`, `ipc/`, `probes/`, `dsoftbus/`, `net/`, `mmio/`, `vfs/`, `timed/`, `updated/`) + new orchestration verbs (`os_lite/phases/{bringup, ipc_kernel, mmio, routing, ota, policy, exec, logd, vfs, net, remote, end}.rs`). `pub fn run()` collapses to ~13 lines.
2. **`PhaseCtx` minimality**: only state read by ≥ 2 phases or determining the marker ladder.
3. **Phase isolation**: `phases/*` MUST NOT import other `phases::*`. Mechanically enforced in Phase 3.
4. **Folder-form heuristic**: `name.rs` is default; `name/mod.rs` only when ≥ 1 sibling exists.
5. **Aggregator-only `mod.rs`**: declarations + re-exports only; no `fn` bodies.
6. **Host-pfad symmetry**: extract host `run()` from `main.rs` to `host_lite/mod.rs::run()` (Cut P3-02).
7. **Mechanical architecture gate**: `scripts/check-selftest-arch.sh` + `just arch-gate` chained into `just dep-gate` (Cut P3-03). Phase 4 extends to enforce no marker-string literals outside `markers_generated.rs` + `markers.rs`.
8. **Explicitly rejected**: hand-written marker-string Rust constants (superseded by Phase 4 generation), `trait Phase`, generic `Probe` trait, renaming `os_lite/`.
9. **Forward-compatibility**: TASK-0024, TRACK-PODCASTS-APP, mediasessd, runtime-profile bisects all land cleanly without `run()` re-touch.

## Phase 4–6 architecture (locked in RFC-0038, normative; new 2026-04-17)
- **Phase 4 — Marker-Manifest + profile dimension**: `source/apps/selftest-client/proof-manifest.toml` is the single source of truth for phase list, marker ladder, profile membership, run config. Profiles unified:
  - **Harness profiles** (drive `scripts/qemu-test.sh` / `tools/os2vm.sh`): `full`, `smp`, `dhcp`, `os2vm`, `quic-required`.
  - **Runtime profiles** (drive `selftest-client` via `SELFTEST_PROFILE` env / kernel cmdline): `full`, `bringup`, `quick`, `ota`, `net`, `none`.
  - Markers gain `emit_when` / `emit_when_not` / `forbidden_when` fields.
  - `build.rs` generates `markers_generated.rs` from the manifest; `arch-gate` enforces no string literal outside generated file + `markers.rs`.
  - `scripts/qemu-test.sh` and `tools/os2vm.sh` consume the manifest via a host CLI (`nexus-proof-manifest list-markers --profile=… / list-env --profile=…`).
  - All `just test-*` recipes route through `just test-os PROFILE=…`. New host-only crate `nexus-proof-manifest`.
  - 10 cuts (P4-01 … P4-10).
- **Phase 5 — Signed evidence bundles**: `target/evidence/<utc>-<profile>-<git-sha>.tar.gz` per run (manifest + uart + trace + config + Ed25519 signature). New host-only crate `nexus-evidence`. `tools/verify-evidence.sh` validates fail-closed. `ci` vs `bringup` key separation. 6 cuts (P5-01 … P5-06).
- **Phase 6 — Replay capability**: `tools/replay-evidence.sh` + `tools/diff-traces.sh` + `tools/bisect-evidence.sh` (all bounded by mandatory `--max-*` budgets). Cross-host determinism floor (CI runner + ≥ 1 dev box) with reviewable allowlist. 6 cuts (P6-01 … P6-06).

Detail: see `docs/rfcs/RFC-0038-...` (Phase 4–6 sections) and `.cursor/next_task_prep.md` (cut tables + hard-gate tables per phase).

## TRACK-OS-PROOF-INFRASTRUCTURE (umbrella, precondition: Phase 6 closure)
- **B — Observability & Performance contracts**: per-phase `icount`/wallclock budgets, structured `TraceEvent` enum, failure-mode catalog, perf regression gate. 4 candidates (CAND-OBS-010..040).
- **C — Coverage as Measured Property**: capability-coverage analyzer (≥ 80% floor on `profile=full`), parser fuzz corpus (manifest + IPC + DSoftBus), ABI snapshot file. 4 candidates (CAND-COV-010..040).
- **D — Discipline & Process**: `nexus-discipline` lint crate, flake-tracking + SLO + stop-the-line, marker-string drift detector, PR template + merge-gate for verified bundle. 4 candidates (CAND-DSC-010..040).
- Track-level stop condition: ≥ 1 candidate from each of B/C/D extracted into a real `TASK-XXXX` and closed; hard gates mechanically enforced.

## Frozen baseline (must stay green per cut)
- Host proofs (Phase 1–3):
  - `cargo test -p dsoftbusd -- --nocapture`
  - `just test-dsoftbus-quic`
  - `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture`
- Host proofs (Phase 4+ adds): `cargo test -p nexus-proof-manifest -- --nocapture`
- Host proofs (Phase 5+ adds): `cargo test -p nexus-evidence -- --nocapture`
- OS proof (Phase 1–3):
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
- OS proof (Phase 4+ replaces with):
  - `just test-os PROFILE=full`
  - `just test-os PROFILE=quic-required`
  - `just test-os PROFILE=smp`
  - `just test-os PROFILE=os2vm`
  - `just test-os PROFILE=bringup` (runtime profile)
  - `just test-os PROFILE=none` (runtime profile)
- Required QUIC subset markers (`profile=quic-required`):
  - `dsoftbusd: transport selected quic`
  - `dsoftbusd: auth ok`
  - `dsoftbusd: os session ok`
  - `SELFTEST: quic session ok`
- Forbidden fallback markers under `profile=quic-required`:
  - `dsoftbusd: transport selected tcp`
  - `dsoftbus: quic os disabled (fallback tcp)`
  - `SELFTEST: quic fallback ok`
- Hygiene gates:
  - `just dep-gate && just diag-os`
  - `just fmt-check && just lint`
  - Phase 3+: `just arch-gate` (chained into `just dep-gate`)
  - Phase 5+: `tools/verify-evidence.sh target/evidence/<latest>` returns 0
- Build sanity (`smoltcp-probe`-gate invariant):
  - `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +nightly check -p selftest-client --target riscv64imac-unknown-none-elf --no-default-features --features os-lite,smoltcp-probe`

## Scope boundaries reaffirmed
- `TASK-0023`: closed as real OS session path (production-floor scope).
- `TASK-0023B`: Phase 1 closed; Phases 2–6 plan locked (~44 cuts total).
- `TASK-0024`: blocked on `TASK-0023B` Phase 4 closure; lands as `dsoftbus/recovery_probe.rs` (capability) + 1 line in `phases/net.rs` (orchestration) + N marker entries in `proof-manifest.toml` (`emit_when = { profile = "quic-required" }`).
- `TASK-0044`: explicit tuning/performance breadth follow-up.
- `TRACK-PODCASTS-APP / TRACK-MEDIA-APPS / TRACK-NEXUSMEDIA-SDK`: future media-side extensions land as `services/mediasessd.rs` + `phases/media.rs` (cfg-gated or new manifest profile `media`).
- `TRACK-OS-PROOF-INFRASTRUCTURE`: precondition is `TASK-0023B` Phase 6 closure. First high-leverage candidates likely CAND-DSC-010 (lint crate) + CAND-OBS-010 (per-phase budgets).

## Next handoff target
- **Step 1 — plan-first**: produce `task-0023b_phase-2..6_*.plan.md` mirroring the Phase-1 plan style, encoding the full ~44-cut sequence (P2-00 … P6-06) with Proof-Floor cadence per cut and hard-gate tables per phase boundary.
- **Step 2 — open Phase 2**: execute Cut P2-00 (RFC-0014 phase list 8 → 12; doc-only) then Cut P2-01 (`phases/` skeleton + `PhaseCtx`); mark RFC-0038 Implementation Checklist as cuts close.
- **Step 3 — sequential execution**: continue through Phase 2 (18 cuts) then Phase 3 (4 cuts).
- **Phase 4 closure trigger**: unblock `TASK-0024` (update its `depends-on`); refresh STATUS-BOARD / IMPLEMENTATION-ORDER; refresh `docs/testing/index.md`.
- **Phase 6 closure trigger**: extract first `TRACK-OS-PROOF-INFRASTRUCTURE` candidate into a real `TASK-XXXX`.
