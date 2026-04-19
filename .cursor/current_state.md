# Cursor Current State (SSOT)

## Current architecture state
- **last_decision**: `TASK-0023B` Phase 4 **CLOSED** under Cursor-internal plan `task-0023b_phase-4_57f4bce2.plan.md`. All 10 cuts (P4-01 → P4-10) executed; RFC-0038 Phase-4 checklist ticked (10 boxes). `source/apps/selftest-client/proof-manifest.toml` is now the SSOT for the marker ladder (433 entries), the harness profile catalog (`full / smp / dhcp / dhcp-strict / os2vm / quic-required`), and the runtime selftest profile catalog (`bringup / quick / ota / net / none`). New host-only crate `nexus-proof-manifest` (parser + CLI: `list-markers / list-env / list-forbidden / list-phases / verify / verify-uart`); `selftest-client/build.rs` generates `markers_generated.rs` from the manifest; 373 emit sites across 29 files migrated to `crate::markers::M_<KEY>` constants; `[marker_emission]` allowlist now empty. `arch-gate` is 6/6 mechanical rules — Rule 6 (added in P4-10) forbids `REQUIRE_*` env literals in `test-*` / `ci-*` justfile recipe bodies, with a `[justfile_require_env]` allowlist (currently empty). `scripts/qemu-test.sh` consumes the manifest via `pm_apply_profile_env` + `pm_mirror_check`, plus a deny-by-default `verify-uart` post-pass. `tools/os2vm.sh` consumes the manifest via `pm_apply_profile_env` + `pm_mirror_subset_check`. New `os_lite/profile.rs` (`Profile`, `PhaseId`, `from_kernel_cmdline_or_default(SELFTEST_PROFILE)`) + `run_or_skip!` macro in `os_lite/mod.rs` drive runtime phase skipping with `dbg: phase X skipped` breadcrumbs. `just test-os PROFILE=…` is canonical; `test-smp / test-os-dhcp / test-os-dhcp-strict / test-dsoftbus-2vm / test-network` deleted in P4-10 (replaced by `ci-os-smp / ci-os-dhcp / ci-os-dhcp-strict / ci-os-os2vm / ci-network`). QEMU `SELFTEST:` ladder for `PROFILE=full` byte-identical to the pre-Phase-4 baseline (mirror-check enforces this on every run). Phase 4 closure unblocks `TASK-0024`. Phase 5 = 6 cuts (signed evidence bundles).
- **prev_decision**: `TASK-0023B` Phase 3 CLOSED 2026-04-17 under `task_0023b_phase_3_ee96d119.plan.md`. All 4 cuts (P3-01 → P3-04) executed; RFC-0038 Phase-3 checklist ticked (4 boxes). `main.rs` shrunk 122 → **49 LoC** (dispatch-only); `host_lite.rs` (78 LoC) holds host-pfad `run()` symmetric to `os_lite::run()`; 13 single-file `name/mod.rs` modules flattened to `name.rs` via pure `git mv` (no parent edits, history preserved); `scripts/check-selftest-arch.sh` (167 LoC, 5 mechanical rules — Phase 4 added Rule 6) + `just arch-gate` recipe chained into `just dep-gate`. QEMU `SELFTEST:` ladder byte-identical (119 markers) across all 4 Phase-3 cuts versus pre-Phase-3 baseline.
- **post_closure_docs_cut (2026-04-17, after Phase 2)**: Two follow-up commits landed on `main` with no code-behavior change and both proof gates green:
  - `65d299d` — `docs(selftest-client): add ADR-0027, onboarding README, and CONTEXT headers`. Authored **`docs/adr/0027-selftest-client-two-axis-architecture.md`** (architectural contract: two-axis nouns+verbs, `PhaseCtx` minimality, phase isolation, aggregator-only `mod.rs`, rejected alternatives, consequences). Authored **`source/apps/selftest-client/README.md`** (onboarding: std vs. os-lite flavors, two invariants, folder map, how-to-run, marker-ladder contract, decision tree for adding new proofs, determinism rules, common pitfalls). Brought all 49 Rust source files under `source/apps/selftest-client/src/` in line with `docs/standards/DOCUMENTATION_STANDARDS.md`: 2026 copyright, `// SPDX-License-Identifier: Apache-2.0`, CONTEXT/OWNERS/STATUS/API_STABILITY/TEST_COVERAGE block, `ADR: docs/adr/0027-selftest-client-two-axis-architecture.md` reference. The 17 pre-existing headers that pointed at the old `ADR-0017-service-architecture.md` were repointed to ADR-0027. The previously-existing 2024 copyright dates were corrected to 2026.
  - `f52cf60` — `style(selftest-client): apply rustfmt to drifted files`. Pre-existing rustfmt drift in 6 files (`phases/{bringup,routing}.rs`, `probes/ipc_kernel/{plumbing,security,soak}.rs`, `updated/stage.rs`) was exposed by `just test-all` running `fmt-check` up front; corrected with `rustfmt --config-path config/rustfmt.toml`. Pure formatting (multi-line `.send/.recv` calls collapsed to single line, inline struct literals); no behavior change.
  - **Verification (2026-04-17)**: `just test-all` exit 0 (440 s; 119 SELFTEST markers; QEMU clean shutdown after `SELFTEST: end`), `just test-network` exit 0 (185 s; all 2-VM phases handshake → session → mux → remote → perf → soak → end with `status=ok`, `result=success`). Logs in `.cursor/test-all.output.log` and `.cursor/test-network.output.log`.
  - **Working tree at handoff**: clean except for `uart.log` (test artifact only — single-HART vs multi-HART log overwrite — explicitly **do not commit**).
- **active_constraints**:
  - keep `TASK-0021`, `TASK-0022`, `TASK-0023` frozen as done baselines,
  - keep marker honesty strict (`ok`/`ready` only after real behavior),
  - keep Phase 2/3/4 behavior-preserving (no marker rename, no reordering, same reject behavior, no new `unwrap`/`expect`, visibility ceiling `pub(crate)`, no new dependencies in selftest-client),
  - Phase 4 may *add* new markers via the manifest (e.g. `SELFTEST: smp ipi ok` under `profile=smp`) but must NOT rename existing markers,
  - `main.rs` is now dispatch-only at 49 LoC; Phase 4 cuts should not touch it (changes belong in manifest + `os_lite/`),
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
- **active_task**: `TASK-0023B` Phase 5 (6 cuts: P5-01 → P5-06). Phase 4 closed 2026-04-17.
- **active_plan**: TBD — author `task-0023b_phase-5_<hash>.plan.md` (Cursor-internal) at the start of the Phase-5 session as a separate plan file; scope: 6 cuts only (signed evidence bundles).
- **resume cut**: P5-01 — `nexus-evidence` crate skeleton + canonicalization spec + unit tests for deterministic hashing.
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

## Structural snapshot (post-Phase-3 closure, 2026-04-17)
- `source/apps/selftest-client/src/main.rs` = **49** lines (CONTEXT + cfgs + 2 dispatch fns + 3 mod decls — zero logic; rustfmt-canonical floor).
- `source/apps/selftest-client/src/host_lite.rs` = **78** lines (host slice — std + no-std-host `pub(crate) fn run()`; sibling-flattened from `host_lite/mod.rs` per Phase-3 single-file rule).
- `source/apps/selftest-client/src/os_lite/mod.rs` = **50** lines (12 `mod` decls + 14-line `pub fn run()` that dispatches to `phases::*::run(&mut ctx)`; within the 80-LoC arch-gate ceiling; CONTEXT clarification expanded slightly from 31 LoC, structurally unchanged).
- 13 single-file `name/mod.rs` candidates flattened to `name.rs` (P3-01): `os_lite/services/{bootctl,bundlemgrd,execd,keystored,logd,metricsd,policyd,samgrd,statefs}/mod.rs`, `os_lite/{mmio,vfs,timed}/mod.rs`, `os_lite/dsoftbus/quic_os/mod.rs`. Pure `git mv`, zero content drift, history preserved.
- `scripts/check-selftest-arch.sh` = **167** lines, executable, `bash -n` clean (P3-03). Enforces 5 mechanical rules: `os_lite/mod.rs` ≤ 80 LoC; no `phases::*` cross-imports; marker strings only in `phases/*`+`markers.rs` (with allowlist); no `fn` in `mod.rs` outside re-exports (with allowlist); no file ≥ 500 LoC (with allowlist).
- `source/apps/selftest-client/.arch-allowlist.txt` = **50** lines (P3-03), 3 sections (`marker_emission` 17 capability files, `mod_rs_fn` `os_lite/mod.rs`, `size_500` `smoltcp_probe.rs`); each entry annotated with intent + Phase-4 tightening plan.
- `justfile` adds `arch-gate` recipe; `dep-gate: arch-gate` chain in place.
- `pub fn run()` body = **14 lines** (`PhaseCtx::bootstrap()?` + 12 phase calls).
- New in Phase 2:
  - `os_lite/context.rs` (52 LOC): `PhaseCtx { reply_send_slot, reply_recv_slot, updated_pending, local_ip, os2vm }` + silent `bootstrap()`.
  - `os_lite/phases/{mod, bringup, routing, ota, policy, exec, logd, ipc_kernel, mmio, vfs, net, remote, end}.rs` (12 phase files + 21-LoC aggregator). LoC ranges 18 (vfs) to 259 (logd); together hold ~1500 LoC of orchestration moved out of `os_lite/mod.rs`.
  - `os_lite/ipc/reply_inbox.rs` (54 LOC): single `ReplyInboxV1` `nexus_ipc::Client` adapter (replaces 3× duplicated local impls).
  - `os_lite/probes/core_service.rs` (64 LOC): the two generic core-service probes moved out of `services/mod.rs`.
- Phase-2 sub-splits:
  - `os_lite/updated/{mod, types, reply_pump, stage, switch, status, health}.rs` — `mod.rs` reduced 451 → 30 LoC (aggregator-only; `pub(crate) use` re-exports preserve all call-sites).
  - `os_lite/probes/ipc_kernel/{mod, plumbing, security, soak}.rs` — `mod.rs` reduced 393 → 28 LoC (aggregator-only).
  - `os_lite/services/mod.rs` reduced 51 → 23 LoC (aggregator-only; `core_service_probe*` moved to `probes/core_service.rs` in Cut P2-17).
- Pre-Phase-2 extractions still in place (from Phase 1):
  - `dsoftbus/{quic_os, remote/{mod, resolve, pkgfs, statefs}}`
  - `net/{icmp_ping, local_addr, smoltcp_probe (cfg-gated)}`
  - `ipc/{clients, routing, reply}` (P2-16 added `reply_inbox`)
  - `mmio/`, `vfs/`, `timed/`
  - `probes/{rng, device_key, ipc_kernel, elf}` (P2-17 added `core_service`; P2-15 sub-split `ipc_kernel/`)
  - `services/{samgrd, bundlemgrd, keystored, policyd, execd, logd, metricsd, statefs, bootctl}/mod.rs`
  - `updated/mod.rs` (P2-14 sub-split into 6 files)
- Behavior-parity gates (verified at every cut):
  - `pub fn run()` call-order unchanged; marker strings byte-identical (119 `SELFTEST:` markers); visibility kept at `pub(crate)`; QEMU marker ladder byte-identical pre- and post-each cut (`diff`-empty across all 18).
  - `cargo check -p selftest-client --no-default-features --features os-lite --target riscv64gc-unknown-none-elf` (with `RUSTFLAGS='--cfg nexus_env="os" -W unexpected_cfgs -W dead_code'`) clean at every cut.

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
- **Resume command (when user says "go")**: switch to plan mode and author `task-0023b_phase-5_<hash>.plan.md` (Cursor-internal) as a separate plan file. Scope: 6 cuts (P5-01 → P5-06), signed evidence bundles. Mirror Phase-3/Phase-4 plan format. Then switch to agent mode and execute P5-01 first.
- **Cut order (Phase 5, locked in RFC-0038)**: P5-01 (`nexus-evidence` crate skeleton + canonicalization spec + Ed25519 hash unit tests) → P5-02 (sealing pipeline: collect manifest + uart + trace + config into `target/evidence/<utc>-<profile>-<sha>.tar.gz`) → P5-03 (`tools/verify-evidence.sh` fail-closed validator + reject tests) → P5-04 (wire `scripts/qemu-test.sh` + `tools/os2vm.sh` to seal on success; `just ci-*` recipes invoke `verify-evidence.sh`) → P5-05 (`ci` vs `bringup` key separation; `nexus-evidence` CLI rejects unknown signers) → P5-06 (Phase-5 closure: RFC tick + handoff sync + `tasks/TASK-0024` unblock confirmation + Phase-6 plan seed).
- **Per-cut verification floor (Phase 5)**: extends Phase 4 floor (which requires `verify-uart` clean) and adds `cargo test -p nexus-evidence -- --nocapture` (from P5-01); from P5-04 onward `tools/verify-evidence.sh target/evidence/<latest>` returns 0 on every successful QEMU run. Marker ladder for `PROFILE=full` must remain byte-identical to the pre-Phase-5 baseline.
- **Phase 5 closure**: tick RFC-0038 Phase-5 checklist (6 boxes); sync `.cursor/{handoff/current.md, next_task_prep.md, current_state.md}`; open `task-0023b_phase-6_<hash>.plan.md` (6 cuts, replay capability); refresh `tasks/STATUS-BOARD.md`, `tasks/IMPLEMENTATION-ORDER.md`, `docs/testing/index.md`.
- **Phase 6 closure trigger**: extract first `TRACK-OS-PROOF-INFRASTRUCTURE` candidate into a real `TASK-XXXX`.
