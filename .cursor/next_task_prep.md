# Next Task Preparation (Drift-Free)

## Candidate next execution
- **task**: begin `TASK-0023B` **Phase 3** under a new Cursor-internal plan `task-0023b_phase-3_<hash>.plan.md` (to be authored at the start of the Phase-3 session).
- **focus (immediate)**: Cut **P3-01** — survey `os_lite/` for Single-File-`name/mod.rs` modules that Phase 2 did NOT sub-split (`phases/`, `updated/`, `probes/ipc_kernel/` are out of scope), then flatten each to `name.rs`.
- **mode**: switch to plan mode first (author the Phase-3 plan), then agent mode to execute the 4 cuts (P3-01 → P3-04) sequentially with the Phase-2 Proof-Floor after each.
- **closed in current session**: all 18 Phase-2 cuts (P2-00 → P2-17). RFC-0038 §"Stop conditions / acceptance" Phase 2 checklist ticked (8 boxes). `os_lite/mod.rs` 1256 → 31 LoC; `pub fn run()` body ~1100 → 14 lines; QEMU `SELFTEST:` ladder byte-identical (119 markers) at every cut.

## Phase-2 closure record (chronological)

Cuts executed in **actual** `pub fn run()` order (top-to-bottom), not the plan's assumed sequence. Per user instruction the plan file itself stayed untouched; this is the deviation record:

| Phase order in `pub fn run()` | Cut ID | Closed |
|---|---|---|
| 1. bringup     | P2-02 | done |
| 2. routing     | P2-05 | done |
| 3. ota         | P2-06 | done |
| 4. policy      | P2-07 | done |
| 5. exec        | P2-08 | done |
| 6. logd        | P2-09 | done |
| 7. ipc_kernel  | P2-03 | done |
| 8. mmio        | P2-04 | done |
| 9. vfs         | P2-10 | done |
| 10. net        | P2-11 | done |
| 11. remote     | P2-12 | done |
| 12. end        | P2-13 | done |

Sub-splits (post-extraction): **P2-14** (updated/ → 6 files), **P2-15** (probes/ipc_kernel/ → 3 files), **P2-16** (DRY `ipc/reply_inbox.rs`), **P2-17** (services/mod.rs aggregator-only via `probes/core_service.rs`). Closure: this file + `handoff/current.md` + `current_state.md` + RFC-0038 checklist.

## Current structural state (post-Phase-2 closure, verified green)
- `source/apps/selftest-client/src/main.rs` = **122** lines (frozen — first move is Cut P3-02).
- `source/apps/selftest-client/src/os_lite/mod.rs` = **31** lines (12 `mod` decls + 14-line `pub fn run()` dispatch).
- `pub fn run()` body = **14 lines** (`PhaseCtx::bootstrap()?` + 12 phase calls).
- New in Phase 2:
  - `os_lite/context.rs` (52 LOC): `PhaseCtx { reply_send_slot, reply_recv_slot, updated_pending, local_ip, os2vm }` + silent `bootstrap()`.
  - `os_lite/phases/{mod, bringup, routing, ota, policy, exec, logd, ipc_kernel, mmio, vfs, net, remote, end}.rs` (12 phase files, LoC range 18→259).
  - `os_lite/ipc/reply_inbox.rs` (54 LOC) — single shared `ReplyInboxV1` `nexus_ipc::Client` adapter (replaces 3× duplicated local impls).
  - `os_lite/probes/core_service.rs` (64 LOC) — generic core-service probes moved out of `services/mod.rs`.
- Phase-2 sub-splits:
  - `os_lite/updated/{mod, types, reply_pump, stage, switch, status, health}.rs` — `mod.rs` 451 → 30 LoC (aggregator-only).
  - `os_lite/probes/ipc_kernel/{mod, plumbing, security, soak}.rs` — `mod.rs` 393 → 28 LoC (aggregator-only).
  - `os_lite/services/mod.rs` 51 → 23 LoC (aggregator-only).
- Pre-Phase-2 extractions still in place (Phase 1):
  - `dsoftbus/{quic_os, remote/{mod, resolve, pkgfs, statefs}}`
  - `net/{icmp_ping, local_addr, smoltcp_probe (cfg-gated)}`
  - `ipc/{clients, routing, reply}` (P2-16 added `reply_inbox`)
  - `mmio/`, `vfs/`, `timed/`
  - `probes/{rng, device_key, ipc_kernel, elf}` (P2-15 sub-split, P2-17 added `core_service`)
  - `services/{samgrd, bundlemgrd, keystored, policyd, execd, logd, metricsd, statefs, bootctl}/mod.rs`

## Phase-2 plan (18 cuts) — CLOSED

Two-axis structure: **capability nouns** (existing) + **orchestration phases** (new). Marker order, marker strings, and reject behavior held frozen across all 18 cuts. Phase-1 Proof-Floor cadence applied after every cut.

| Cut | Scope | Status |
|---|---|---|
| P2-00 | Doc-only: extend `RFC-0014` phase list 8 → 12. | done |
| P2-01 | Skeleton: `phases/mod.rs`, `os_lite/context.rs` with empty `PhaseCtx::bootstrap()`. | done |
| P2-02 | Extract `phases/bringup.rs`. | done |
| P2-03 | Extract `phases/ipc_kernel.rs` (orchestration only). | done |
| P2-04 | Extract `phases/mmio.rs`. | done |
| P2-05 | Extract `phases/routing.rs`. | done |
| P2-06 | Extract `phases/ota.rs`. | done |
| P2-07 | Extract `phases/policy.rs`. | done |
| P2-08 | Extract `phases/exec.rs` (timing preserved). | done |
| P2-09 | Extract `phases/logd.rs` (logd-stat deltas preserved). | done |
| P2-10 | Extract `phases/vfs.rs`. | done |
| P2-11 | Extract `phases/net.rs`. | done |
| P2-12 | Extract `phases/remote.rs`. | done |
| P2-13 | Extract `phases/end.rs`. `pub fn run()` body now 14 lines. | done |
| P2-14 | Sub-split `updated/{types, reply_pump, stage, switch, status, health}.rs` via `updated/mod.rs` re-exports. `mod.rs` 451 → 30 LoC. | done |
| P2-15 | Sub-split `probes/ipc_kernel/{plumbing, security, soak}.rs`. `mod.rs` 393 → 28 LoC. | done |
| P2-16 | DRY `ipc/reply_inbox.rs` newtype + `impl Client`; removed 3× duplicated local impls. Net −21 LoC. | done |
| P2-17 | Aggregator-only cleanup: moved `services::core_service_probe*` to `probes/core_service.rs`; `services/mod.rs` 51 → 23 LoC (declarations only). | done |

### `PhaseCtx` minimality (locked at Cut P2-01, executed)
- **Promoted (5 fields)**: `reply_send_slot: u32`, `reply_recv_slot: u32`, `updated_pending: VecDeque<Vec<u8>>`, `local_ip: Option<[u8;4]>`, `os2vm: bool`.
- **Deliberately NOT promoted**: service handles (logd/policyd/bundlemgrd/samgrd/updated/execd/statefsd/keystored). The existing `pub fn run()` re-resolves them per-phase via `route_with_retry` 4–5 times; promoting them is higher-risk and would conflate Phase 2 (behavior-preserving extraction) with a separate refactor. Revisit at Phase 3 if real duplication shows up after extraction.
- **Forbidden**: phase-local timing, retry counters scoped to one phase, transient buffers — keep those in the phase file.

### Phase isolation invariant (mechanically enforced in Phase 3)
- `phases/*` MUST NOT import other `phases::*`.
- Allowed downstream imports for `phases/*`: `services::*`, `ipc::*`, `probes::*`, `dsoftbus::*`, `net::*`, `mmio::*`, `vfs::*`, `timed::*`, `updated::*`.

## Refined Phase-3 plan (4 cuts)

| Cut | Scope | Risk |
|---|---|---|
| P3-01 | Flatten Single-File-`name/mod.rs` → `name.rs` for modules Phase 2 did NOT sub-split. Candidates today: `services/{keystored, execd, metricsd, statefs, bootctl}/mod.rs`, `mmio/mod.rs`, `vfs/mod.rs`, `timed/mod.rs`. (Final list depends on Phase-2 sub-split outcomes.) | low (mechanical) |
| P3-02 | Extract host-pfad `run()` from `main.rs` into `host_lite/mod.rs::run()`. `main.rs` becomes cfg + `os_entry()` + `main()` only. | medium (host build path) |
| P3-03 | Write `scripts/check-selftest-arch.sh` + `just arch-gate` recipe; chain into `just dep-gate`; produce allowlist file. Mechanical anti-re-monolithization gate. | low |
| P3-04 | Standards review (`#[must_use]` on decision-bearing results, `newtype` for safety-relevant IDs, `Send`/`Sync` audit). Apply only where mechanical and risk-free. | low |

### Architecture-gate rules (Cut P3-03)

| Rule | Mechanism |
|---|---|
| `os_lite/mod.rs` ≤ 80 LOC | `wc -l` |
| `phases/*.rs` does not import other `phases::*` | `rg -n "use .*::phases::" os_lite/phases/` |
| Marker strings (`"SELFTEST: ..."`, `"dsoftbusd: ..."`) only in `phases/*` and `markers.rs` (Phase 4 tightens to: only in `markers_generated.rs` + `markers.rs`) | `rg -n '"SELFTEST: ' os_lite/{services,ipc,probes,dsoftbus,net,mmio,vfs,timed,updated}/` |
| `mod.rs` files contain no `fn` definitions outside re-exports | `rg -n "^\s*(pub(\(crate\))? )?fn " **/mod.rs` |
| No file ≥ 500 LOC outside the explicit allowlist | `wc -l` + allowlist file |

## Refined Phase-4 plan (10 cuts) — Marker-Manifest as SSOT + profile-aware harness

Goal: `proof-manifest.toml` becomes the single source of truth. `scripts/qemu-test.sh` and `tools/os2vm.sh` consume it. Runtime selftest profiles via `SELFTEST_PROFILE` env / kernel cmdline.

| Cut | Scope | Risk |
|---|---|---|
| P4-01 | Write manifest schema doc (`docs/testing/proof-manifest.md`) + `proof-manifest.toml` skeleton (meta + 12 phase declarations). New host-only crate `nexus-proof-manifest` with parser + reject tests. | low |
| P4-02 | Cross-reference RFC-0014 phase list (already extended in P2-00); document manifest ↔ RFC-0014 binding. | trivial |
| P4-03 | Populate manifest with all current markers from `scripts/qemu-test.sh` + `selftest-client` source (1:1, no behavior change). `build.rs` generates `markers_generated.rs`. | medium (1:1 fidelity) |
| P4-04 | Replace marker emission in `phases/*` with generated constants. Arch-gate enforces no marker string literal outside `markers_generated.rs` + `markers.rs`. | low |
| P4-05 | Harness profiles `full`, `smp`, `dhcp`, `os2vm`, `quic-required` defined in manifest; `scripts/qemu-test.sh` consumes manifest via `nexus-proof-manifest` host CLI (`list-markers --profile=…`, `list-env --profile=…`). | medium (harness rewrite) |
| P4-06 | Migrate `test-os`, `test-smp`, `test-os-dhcp`, `test-dsoftbus-2vm`, `test-network` `just` recipes to `just test-os PROFILE=…`. Old recipes alias for ≥ 1 cycle, then deleted. | low |
| P4-07 | `tools/os2vm.sh` consumes manifest (`profile.os2vm`). | medium (2-VM harness) |
| P4-08 | Runtime-only profiles `bringup`, `quick`, `ota`, `net`, `none` defined in manifest + `os_lite/profile.rs::Profile::from_kernel_cmdline_or_default(Profile::Full)`. `pub fn run()` iterates `profile.enabled_phases()`. Per-profile QEMU smoke tests added. | low |
| P4-09 | Deny-by-default analyzer: any unexpected runtime marker for active profile = hard failure (host-side). | low |
| P4-10 | Hard-deprecate direct `RUN_PHASE`/`REQUIRE_*` env usage in CI; CI must invoke `just test-os PROFILE=…`. Document in `docs/testing/index.md`. | trivial |

### Manifest schema (normative target shape)

```toml
[meta]
schema_version = "1"
default_profile = "full"

[phase.bringup]
order = 1
markers = ["init: ready", "samgrd: ready", "execd: ready"]

# … 11 more [phase.X] entries …

[profile.full]
runner = "scripts/qemu-test.sh"
env = {}
phases = "all"

[profile.smp]
extends = "full"
env = { SMP = "2", REQUIRE_SMP = "1" }

[profile.os2vm]
runner = "tools/os2vm.sh"
env = { REQUIRE_DSOFTBUS = "1" }

[profile.bringup]   # runtime-only sub-profile
runtime_only = true
phases = ["bringup", "ipc_kernel", "end"]

[marker."SELFTEST: smp ipi ok"]
phase = "bringup"
emit_when = { profile = "smp", smp_min = 2 }
proves = "SMP IPI delivery between hart 0 and hart 1"
introduced_in = "TASK-0012"
```

### Phase-4 hard gates (mechanically enforced)

| Rule | Mechanism |
|---|---|
| No marker string literal outside `markers_generated.rs` + `markers.rs` | `arch-gate` (extension of P3-03) |
| No `REQUIRE_*` env var read directly in `just test-*` recipes | `rg` over `justfile` |
| Manifest parser rejects unknown keys | `cargo test -p nexus-proof-manifest` |
| Profile with no declared markers rejected at build time | parser reject test |
| Unexpected runtime marker for active profile = hard failure | host analyzer over `uart.log` |
| Skipped runtime phases emit no `*: ready` / `SELFTEST: * ok` | host analyzer + grep gate |

### Out of scope for Phase 4 (intentional)
- Host tests (`cargo test --workspace`, `just test-host`, `just test-e2e`, `just test-dsoftbus-quic`) stay outside the manifest by design (different mental model: cargo-tested host logic vs. QEMU-attested OS behavior).
- Marker authority for *services* (e.g. `dsoftbusd: ready`) stays with each service's owning task; manifest is the cross-cutting orchestration contract.

## Refined Phase-5 plan (6 cuts) — Signed evidence bundles

Goal: every QEMU run writes `target/evidence/<utc>-<profile>-<git-sha>.tar.gz` with manifest + uart + trace + config + Ed25519 signature. New host-only crate `nexus-evidence` owns canonicalization / sign / verify.

| Cut | Scope | Risk |
|---|---|---|
| P5-01 | `nexus-evidence` crate skeleton + canonicalization spec + unit tests for deterministic hashing. | low |
| P5-02 | `tools/extract-trace.sh` produces `trace.jsonl` from `uart.log` using manifest phase tags. Reject test for out-of-order ladder. | low |
| P5-03 | `tools/seal-evidence.sh` builds and signs `evidence-bundle.tar.gz`; `EVIDENCE_KEY` selects `ci` vs `bringup`. | medium (signing path) |
| P5-04 | Hook seal step into `scripts/qemu-test.sh` after pass/fail decision. Failed runs produce bundle without signature (replay-only). | low |
| P5-05 | `tools/verify-evidence.sh` + bring-up vs CI key separation; reject tests for tamper classes (manifest/uart/trace/key swap). | medium (key model) |
| P5-06 | `docs/testing/evidence-bundle.md` documents bundle layout, key model, verify workflow. | trivial |

### Phase-5 hard gates

| Rule | Mechanism |
|---|---|
| Successful run without sealed bundle = CI failure | hook in `scripts/qemu-test.sh` |
| Bringup-key bundle validates against CI policy = test failure | unit test in `nexus-evidence` |
| Tampered bundle validates = test failure (per tamper class) | unit tests |
| Secret material in `uart.log` / `trace.jsonl` / `config.json` | reject test (known patterns + entropy heuristic) |
| Bundle missing any required artifact | `verify-evidence.sh` fails closed |

## Refined Phase-6 plan (6 cuts) — Replay capability

Goal: failures reproducible from stored bundles; CI bisects become trace-diff-driven.

| Cut | Scope | Risk |
|---|---|---|
| P6-01 | `tools/replay-evidence.sh` skeleton — extract bundle, validate signature, pin git-SHA, set env, invoke `just test-os PROFILE=<recorded>`. | medium (env replay) |
| P6-02 | Trace diff format spec (`docs/testing/trace-diff-format.md`) + `tools/diff-traces.sh`; unit fixtures for `exact_match`, `extra`, `missing`, `reorder`, `phase_mismatch`. | low |
| P6-03 | `tools/bisect-evidence.sh` with mandatory `--max-commits` + `--max-seconds` budgets; fail-closed on exhaust. | low |
| P6-04 | `scripts/regression-bisect.sh` wrapper for typical CI-failure flow. | low |
| P6-05 | Cross-host determinism floor: replay must be empty-diff on CI runner + ≥ 1 dev box for the same bundle. Documented allowlist (wall-clock, qemu version banner, hostname); append-only with reviewer signoff. | medium (real-world drift) |
| P6-06 | `docs/testing/replay-and-bisect.md` documents workflow + allowlist + extension procedure. | trivial |

### Phase-6 hard gates

| Rule | Mechanism |
|---|---|
| Unbounded replay run | `--max-seconds` mandatory; CLI rejects missing arg |
| Replay requires environment beyond what bundle records | `replay-evidence.sh` fails closed pre-QEMU |
| Cross-host allowlist accepts arbitrary fields | code-review + structured allowlist file |
| Bisect without `--max-commits` | CLI rejects missing arg |

## Current proven baseline (must stay green per cut)
- Host (Phase 1–3):
  - `just test-dsoftbus-quic`
  - `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture`
  - `cargo test -p dsoftbusd -- --nocapture`
- Host (Phase 4+ adds):
  - `cargo test -p nexus-proof-manifest -- --nocapture`
- Host (Phase 5+ adds):
  - `cargo test -p nexus-evidence -- --nocapture`
- OS (Phase 1–3):
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
- OS (Phase 4+ replaces with):
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
- Hygiene:
  - `just dep-gate && just diag-os`
  - `just fmt-check && just lint`
  - Phase 3+: `just arch-gate` (chained into `just dep-gate`)
  - Phase 5+: `tools/verify-evidence.sh target/evidence/<latest>` returns 0

## Boundaries for Phase 2–6
- Keep `TASK-0021`, `TASK-0022`, `TASK-0023` closed/done.
- Do not regress `TASK-0023` to fallback-only marker semantics.
- Do not absorb `TASK-0024` transport features into `TASK-0023B`. `TASK-0024` is unblocked at **Phase 4 closure**, not earlier.
- Phase-2/3 slicing is behavior-preserving: same marker order, same proof meanings, same reject behavior.
- Phase 4 may add new markers via manifest (e.g. `SELFTEST: smp ipi ok` under `profile=smp`) but must not rename existing markers.
- `main.rs` remains 122 LOC through Phase 2; only Cut P3-02 modifies it.
- Visibility ceiling: `pub(crate)` (binary crate boundary).
- No new `unwrap`/`expect`. No new dependencies in selftest-client `Cargo.toml`. New host-only crates (`nexus-proof-manifest`, `nexus-evidence`) are separate.
- Single-File-`name/mod.rs` flattening (Cut P3-01) only applies where Phase 2 did NOT introduce siblings.
- No kernel changes across all 6 phases. `SELFTEST_PROFILE` reading from kernel cmdline is a userspace read.
- Host-only tests (`cargo test --workspace`, `just test-host`, `just test-e2e`, `just test-dsoftbus-quic`) stay outside the proof manifest by design.

## Explicitly rejected ideas (with reason — do not reintroduce)
- ~~Marker-string Rust constants written by hand~~ — superseded by Phase 4: constants are *generated* from the manifest, removing the two-truth surface entirely.
- `trait Phase` (boilerplate without composition gain; free fns are simpler).
- Generic `Probe` trait hierarchy (mismatch with linear deterministic ladder).
- Renaming `os_lite/` to `os_suite/` etc. (36-file churn for cosmetic gain).
- Cfg-time runtime-profile selection (forces recompile per profile; superseded by `SELFTEST_PROFILE` env).
- Collapsing host tests into the proof manifest (different mental model; weakens both).

## Forward-compatibility notes
- **TASK-0024 (DSoftBus QUIC recovery / UDP-sec)**: blocked until `TASK-0023B` Phase 4 closure. Lands as `dsoftbus/recovery_probe.rs` (capability) + 1 line in `phases/net.rs` + N marker entries in `proof-manifest.toml` with `emit_when = { profile = "quic-required" }`. No `run()` touch.
- **TRACK-PODCASTS-APP / TRACK-MEDIA-APPS / TRACK-NEXUSMEDIA-SDK**: lands as `services/mediasessd.rs` + `phases/media.rs` (cfg-gated profile or new manifest profile `media`).
- **TRACK-OS-PROOF-INFRASTRUCTURE** (B/C/D candidates): precondition is `TASK-0023B` Phase 6 closure. First candidates likely CAND-DSC-010 (lint crate) and CAND-OBS-010 (per-phase budgets) for highest immediate leverage.

## Linked contracts
- `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
- `tasks/TRACK-OS-PROOF-INFRASTRUCTURE.md`
- `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
- `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`
- `docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md`
- `tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md`
- `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (extended 8 → 12 in Cut P2-00)
- `docs/testing/index.md`
- `docs/distributed/dsoftbus-lite.md`
- `tasks/STATUS-BOARD.md`
- `tasks/IMPLEMENTATION-ORDER.md`

## Ready condition
- **Active plan (Cursor-internal, do not edit during execution)**: TBD — `/home/jenning/.cursor/plans/task-0023b_phase-3_<hash>.plan.md` to be authored at the start of the Phase-3 session, scoped to 4 cuts only (P3-01 → P3-04), mirroring the Phase-2 plan format.
- **Resume command (when user says "go")**: switch to **plan mode** to author the Phase-3 plan; then switch to agent mode, mark P3-01 todo `in_progress`, and execute the survey-first flatten cut. After each cut: `cargo +nightly check -p selftest-client --target riscv64imac-unknown-none-elf --no-default-features --features os-lite` → `cargo test -p dsoftbusd` → `just test-dsoftbus-quic` → `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os` (`grep -E '^SELFTEST: '` `diff`-empty vs pre-cut baseline) → `rustfmt +stable <touched .rs>` → `just lint`. After P3-03 lands, also `just arch-gate`.
- **Phase 3 closure trigger**: tick RFC-0038 Phase-3 checklist; sync `.cursor/{handoff/current.md, next_task_prep.md, current_state.md}`; open `task-0023b_phase-4_<hash>.plan.md` (10 cuts, manifest-driven).
- **Phase 4 closure trigger**: unblock `TASK-0024` (update its `depends-on`), update STATUS-BOARD / IMPLEMENTATION-ORDER, refresh `docs/testing/index.md`.
- **Phase 6 closure trigger**: extract first `TRACK-OS-PROOF-INFRASTRUCTURE` candidate into a real `TASK-XXXX`.
