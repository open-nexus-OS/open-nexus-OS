# Current Handoff: TASK-0023B Phase 2 — CLOSED

**Date**: 2026-04-17 (Phase 2 closure session, all 18 cuts landed)
**Status**: `TASK-0023B` Phase 2 **complete**. All 18 cuts (P2-00 → P2-17) executed under Cursor-internal plan `task-0023b_phase_2_plan_5e547ada.plan.md`. RFC-0038 §"Stop conditions / acceptance" Phase 2 checklist ticked (8 boxes). `pub fn run()` reduced from ~1100 LoC of inline orchestration to **14 lines** of phase dispatch; `os_lite/mod.rs` reduced from 1256 → **31 LoC**. QEMU `SELFTEST:` ladder is byte-identical (119 markers) across all 18 cuts versus the pre-Phase-2 baseline.
**Execution SSOT**: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
**Contract SSOT**: `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
**Long-running discipline track**: `tasks/TRACK-OS-PROOF-INFRASTRUCTURE.md`

## Phase 2 closure summary

### Final structural state (post-P2-17, verified 2026-04-17)

| File | Pre-Phase-2 LoC | Post-Phase-2 LoC | Δ |
|---|---|---|---|
| `source/apps/selftest-client/src/main.rs` | 122 | 122 | 0 (frozen — moves in Phase 3 / Cut P3-02) |
| `source/apps/selftest-client/src/os_lite/mod.rs` | 1256 | **31** | **−1225** |
| `pub fn run()` body inside the above | ~1100 | **14** | **−~1086** |
| `os_lite/updated/mod.rs` | 451 | **30** | **−421** (split into 6 files) |
| `os_lite/probes/ipc_kernel/mod.rs` | 393 | **28** | **−365** (split into 3 files) |
| `os_lite/services/mod.rs` | 51 | **23** | **−28** (now aggregator-only) |

### New top-level structure under `source/apps/selftest-client/src/os_lite/`

- **Capabilities (nouns, pre-existing)**: `dsoftbus/`, `ipc/`, `mmio/`, `net/`, `probes/`, `services/`, `timed/`, `updated/`, `vfs/`.
- **Orchestration (verbs, NEW in Phase 2)**: `phases/{bringup, routing, ota, policy, exec, logd, ipc_kernel, mmio, vfs, net, remote, end}.rs` (12 files, 21 LoC aggregator + ~1500 LoC of moved bodies).
- **Cross-phase state (NEW in Phase 2)**: `context.rs` hosts `PhaseCtx { reply_send_slot, reply_recv_slot, updated_pending, local_ip, os2vm }` (5 fields, locked minimal) + silent `bootstrap()`.
- **DRY consolidation (NEW in Phase 2)**: `ipc/reply_inbox.rs` hosts the single `ReplyInboxV1` `nexus_ipc::Client` adapter previously triplicated across 3 probes.
- **Sub-splits (NEW in Phase 2)**:
  - `updated/{types, reply_pump, stage, switch, status, health}.rs` (mod.rs is aggregator-only).
  - `probes/ipc_kernel/{plumbing, security, soak}.rs` (mod.rs is aggregator-only).
- **Aggregator-only cleanups**: `services/mod.rs` and `probes/ipc_kernel/mod.rs` and `updated/mod.rs` now hold zero `fn` bodies (refinement (5) of RFC-0038).
- **Moved out**: `services::core_service_probe*` → `probes/core_service.rs` (the only generic probes that lived under `services/` before P2-17).

### Cut-by-cut log (executed in actual `pub fn run()` order, not plan-assumed order)

| Cut | Scope | LoC delta in `os_lite/mod.rs` | Marker parity vs prior cut |
|---|---|---|---|
| P2-00 | RFC-0014 phase list 8 → 12 (doc-only) | 0 | n/a (no code change) |
| P2-01 | `phases/` skeleton + `os_lite/context.rs` + `PhaseCtx` | ~+30 | byte-identical |
| P2-02 | extract `phases/bringup.rs` | −210 | byte-identical |
| P2-05 | extract `phases/routing.rs` | −94 | byte-identical |
| P2-06 | extract `phases/ota.rs` | −188 | byte-identical |
| P2-07 | extract `phases/policy.rs` | −282 | byte-identical |
| P2-08 | extract `phases/exec.rs` | −197 | byte-identical |
| P2-09 | extract `phases/logd.rs` | −233 | byte-identical |
| P2-03 | extract `phases/ipc_kernel.rs` | −51 | byte-identical |
| P2-04 | extract `phases/mmio.rs` | −15 | byte-identical |
| P2-10 | extract `phases/vfs.rs` | −4 | byte-identical |
| P2-11 | extract `phases/net.rs` | −31 | byte-identical |
| P2-12 | extract `phases/remote.rs` | −90 | byte-identical |
| P2-13 | extract `phases/end.rs` | −10 | byte-identical |
| P2-14 | sub-split `updated/{types,reply_pump,stage,switch,status,health}.rs` | 0 (intra-domain) | byte-identical |
| P2-15 | sub-split `probes/ipc_kernel/{plumbing,security,soak}.rs` | 0 (intra-domain) | byte-identical |
| P2-16 | DRY consolidation `ipc/reply_inbox.rs` (drops 3 local impls) | 0 (intra-probe) | byte-identical |
| P2-17 | move `services::core_service_probe*` → `probes/core_service.rs`; `services/mod.rs` aggregator-only | 0 (cross-module rename + import path) | byte-identical |

### Behavioral parity gates (all green at every cut)

- `cargo check -p selftest-client --no-default-features --features os-lite --target riscv64gc-unknown-none-elf` (`RUSTFLAGS='--cfg nexus_env="os" -W unexpected_cfgs -W dead_code'`) → clean (only pre-existing `nexus_env` cfg warnings).
- `just test-os` → 119 `SELFTEST:` markers, BYTE-IDENTICAL to the P2-00 baseline at every cut (`diff /tmp/p2_<n-1>_selftest.txt /tmp/p2_<n>_selftest.txt` empty).
- Marker strings, marker order, reject behavior, retry/yield budgets, NONCE seeds, and IPC frame layouts all preserved verbatim.
- No new `unwrap`/`expect`. No new dependencies in `selftest-client`.

### Operational lessons captured this phase (apply forward)

- `cargo +stable fmt --all` reformats ~200 unrelated files due to long-standing rustfmt drift; never run it. Use `rustfmt +stable <touched-files>` and immediately revert any submodule churn pulled in via `mod` resolution.
- `git diff --name-only | grep -v -E '^(file1|file2)$'` requires exact-line match; prefer `git diff --name-only -- :^path1 :^path2` instead, or use `xargs -r git checkout --` for revert lists.
- `just diag-os` does not include `selftest-client`. Use the `RUSTFLAGS=...` `cargo check` command above for direct compile verification of selftest changes.
- The sandbox blocks reads under `.git/hooks` for some commands; re-run with `required_permissions: ["all"]` when sandbox-related path errors appear (no security implication — this is purely a sandbox-mount artifact).
- Phase isolation rule: `phases::*` modules deliberately do NOT import other `phases::*` modules. Service handles are re-resolved per-phase via the silent `route_with_retry`; this is cheaper than carrying clients across `PhaseCtx` and matches the production-grade isolation goal of refinement (3).
- `phases::end::run` returns `!` (the never type), which coerces cleanly to `Result<(), ()>`. This keeps `pub fn run()` totally `?`-free at the dispatch layer.

## Phase 3 — what's next

Phase-3 contract is locked in `RFC-0038` and is 4 cuts:

- **Cut P3-01** (refinement (4)) — flatten Single-File-`name/mod.rs` modules to `name.rs` for those Phase 2 did NOT sub-split. Concretely: review `services/{bootctl,bundlemgrd,execd,keystored,logd,metricsd,policyd,samgrd,statefs}/mod.rs` and `dsoftbus/remote/{resolve,statefs,pkgfs}.rs` parents; flatten any folder that has only one sibling. Modules already sub-split in Phase 2 (`updated/`, `probes/ipc_kernel/`) are explicitly out of scope.
- **Cut P3-02** (refinement (6)) — extract host-pfad `run()` from `main.rs` into `host_lite/mod.rs::run()`; `main.rs` becomes cfg + 2 dispatch lines. This is the ONLY Phase-2/3 cut that touches `main.rs` (122 LoC frozen until then).
- **Cut P3-03** (refinement (7)) — write `scripts/check-selftest-arch.sh` + `just arch-gate` recipe; chain into `just dep-gate`; produce allowlist file. Mechanically enforces phase isolation, aggregator-only `mod.rs`, and (when Phase 4 lands) marker-string SSOT.
- **Cut P3-04** — standards review: `#[must_use]` on decision-bearing results, `newtype` for safety-relevant IDs/state, `Send`/`Sync` audit. Apply where the diff is mechanical and risk-free.

A Cursor-internal Phase-3 plan file (`task-0023b_phase-3_<hash>.plan.md`) will be authored at the start of the Phase-3 session, scoped to these 4 cuts only, mirroring the Phase-2 plan format.

## Frozen baseline that must stay green (verified end-of-Phase-2; carries into Phase 3)

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
- Hygiene: `just dep-gate && just diag-os && just fmt-check && just lint`
- Phase 3+: `just arch-gate` (chained into `just dep-gate`)
- Phase 5+: `tools/verify-evidence.sh target/evidence/<latest>` returns 0

## Boundaries reaffirmed

- Phase 2 is closed and behavior-preserving: same marker order, same proof meanings, same reject behavior across all 18 cuts.
- Phase 4 may *add* new markers gated by new profiles (e.g. `SELFTEST: smp ipi ok`) but must not rename existing markers.
- `main.rs` stays at 122 LoC throughout Phase 2 (verified) and into Phase 3 until Cut P3-02.
- Visibility ceiling: `pub(crate)`. No new `unwrap`/`expect`. No new dependencies in `Cargo.toml` for selftest-client itself; new host-only crates (`nexus-proof-manifest`, `nexus-evidence`) are separate.
- Do not absorb `TASK-0024` transport features. Do not regress `TASK-0023` to fallback-only marker semantics.
- Single-File-`name/mod.rs` flattening (Cut P3-01) only applies to modules Phase 2 did NOT sub-split.
- No kernel changes across all 6 phases. `SELFTEST_PROFILE` reading from kernel cmdline is a userspace read, not a kernel API change.
- Host-only tests stay outside the proof manifest by design.

## Next handoff target

- **Active plan**: TBD — author `task-0023b_phase-3_<hash>.plan.md` (Cursor-internal) at the start of the Phase-3 session; scope: 4 cuts P3-01 → P3-04 only.
- **Resume point**: Cut **P3-01** — flatten Single-File-`name/mod.rs` modules. Survey-first cut: enumerate folders under `os_lite/` whose `mod.rs` is the only file in the folder (excluding `phases/`, `updated/`, `probes/ipc_kernel/` which are already sub-split). Then flatten each.
- **Per-cut cadence (carry from Phase 2)**:
  1. `RUSTFLAGS='--cfg nexus_env="os" -W unexpected_cfgs -W dead_code' cargo check -p selftest-client --no-default-features --features os-lite --target riscv64gc-unknown-none-elf`
  2. `cargo test -p dsoftbusd -- --nocapture`
  3. `just test-dsoftbus-quic`
  4. `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  5. `rustfmt +stable <touched .rs files only>`; verify and revert any submodule drift via `git checkout -- <unintended>`
  6. `just lint`
- **Phase 3 closure tasks (after P3-04)**: tick RFC-0038 Phase 3 checklist (4 boxes); sync `.cursor/{handoff/current.md, next_task_prep.md, current_state.md}`; open `task-0023b_phase-4_<hash>.plan.md` (10 cuts).
- STATUS-BOARD / IMPLEMENTATION-ORDER stay deferred until Phase 4 closure (which also unblocks `TASK-0024`).
