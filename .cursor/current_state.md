# Cursor Current State (SSOT)

## Current architecture state
- **last_decision**: `TASK-0023B` Phase 2 **CLOSED** under Cursor-internal plan `task-0023b_phase_2_plan_5e547ada.plan.md`. All 18 cuts (P2-00 → P2-17) executed; RFC-0038 Phase-2 checklist ticked (8 boxes). `os_lite/mod.rs` reduced 1256 → **31 LoC**; `pub fn run()` body reduced ~1100 → **14 lines**; QEMU `SELFTEST:` ladder byte-identical (119 markers) at every cut. PhaseCtx minimality locked at 5 fields (`reply_send_slot`, `reply_recv_slot`, `updated_pending`, `local_ip`, `os2vm`); service handles deliberately not promoted (re-resolved per-phase via silent `route_with_retry`). Plan deviation: cuts executed in actual `pub fn run()` order (P2-02 → P2-05 → P2-06 → P2-07 → P2-08 → P2-09 → P2-03 → P2-04 → P2-10 → P2-11 → P2-12 → P2-13 → P2-14 → P2-15 → P2-16 → P2-17). New top-level structure under `os_lite/`: capability nouns (services/, ipc/, probes/, etc.) + orchestration verbs (`phases/`) + `context.rs` (PhaseCtx) + `ipc/reply_inbox.rs` (DRY shared adapter). Sub-splits landed for `updated/` (6 files), `probes/ipc_kernel/` (3 files); `services/mod.rs` and `probes/ipc_kernel/mod.rs` and `updated/mod.rs` are now aggregator-only (no fn bodies). Phase 1 closed; Phase 3 = 4 cuts (P3-01 flatten Single-File-`name/mod.rs` / P3-02 host-pfad extraction / P3-03 `arch-gate` / P3-04 standards review); Phase 4-6 + TRACK-OS-PROOF-INFRASTRUCTURE plans unchanged. `TASK-0024` still blocked on Phase 4 closure.
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
- **active_task**: `TASK-0023B` Phase 3 (4 cuts: P3-01 → P3-04). Phase 2 closed.
- **active_plan**: TBD — author `task-0023b_phase-3_<hash>.plan.md` (Cursor-internal) at the start of the Phase-3 session; scope: 4 cuts only.
- **resume cut**: P3-01 (flatten Single-File-`name/mod.rs` modules under `os_lite/` for those Phase 2 did NOT sub-split; survey-first cut). The `phases/`, `updated/`, `probes/ipc_kernel/` sub-splits are explicitly out of scope for P3-01.
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

## Structural snapshot (post-Phase-2 closure, 2026-04-17)
- `source/apps/selftest-client/src/main.rs` = **122** lines (frozen — no Phase-2 cut touched it; first move is Cut P3-02 host-pfad extraction).
- `source/apps/selftest-client/src/os_lite/mod.rs` = **31** lines (12 `mod` decls + 14-line `pub fn run()` that dispatches to `phases::*::run(&mut ctx)`).
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
- **Resume command (when user says "go")**: switch to plan mode and author `task-0023b_phase-3_<hash>.plan.md` (Cursor-internal). Scope: 4 cuts (P3-01 → P3-04). Mirror Phase-2 plan format. Then switch to agent mode and execute P3-01 first.
- **Cut order (Phase 3, locked in RFC-0038)**: P3-01 (flatten Single-File-`name/mod.rs`; survey-first) → P3-02 (`host_lite/` extraction; first `main.rs` move) → P3-03 (`scripts/check-selftest-arch.sh` + `just arch-gate` chained into `just dep-gate`) → P3-04 (standards review + closure).
- **Per-cut verification floor (Phase 3)**: `cargo +nightly check -p selftest-client --target riscv64imac-unknown-none-elf --no-default-features --features os-lite` → `cargo test -p dsoftbusd` → `just test-dsoftbus-quic` → `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os` (compare `grep -E '^SELFTEST: '` against pre-cut baseline; must be byte-identical) → `rustfmt +stable <touched .rs>` → `just lint`. After P3-03 lands, also `just arch-gate`.
- **Phase 3 closure**: tick RFC-0038 Phase-3 checklist; sync `.cursor/{handoff/current.md, next_task_prep.md, current_state.md}`; open `task-0023b_phase-4_<hash>.plan.md` (10 cuts, manifest-driven).
- **Phase 4 closure trigger**: unblock `TASK-0024` (update its `depends-on`); refresh STATUS-BOARD / IMPLEMENTATION-ORDER; refresh `docs/testing/index.md`.
- **Phase 6 closure trigger**: extract first `TRACK-OS-PROOF-INFRASTRUCTURE` candidate into a real `TASK-XXXX`.
