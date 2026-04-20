# Next Task Preparation (Drift-Free)

## Candidate next execution
- **task**: begin `TASK-0023B` **Phase 6** under a new Cursor-internal plan `task-0023b_phase-6_<hash>.plan.md` (to be authored at the start of the Phase-6 session as a separate plan file).
- **focus (immediate)**: Cut **P6-01** — `tools/replay-evidence.sh` skeleton: extract bundle, validate signature via `nexus-evidence verify`, pin git-SHA, set recorded env + kernel cmdline + QEMU args, invoke `just test-os PROFILE=<recorded>`, capture fresh trace.
- **mode**: switch to plan mode first (author the Phase-6 plan), then agent mode to execute the 6 cuts (P6-01 → P6-06) sequentially with the Phase-5 Proof-Floor (which now requires `verify-uart` clean, `arch-gate` 6/6 rules clean, `PROFILE=full` byte-identical marker ladder, and `tools/verify-evidence.sh` returns 0 on every successful QEMU run) after each.
- **closed in current session**: all **7** Phase-5 cuts (P5-00 → P5-06; P5-00 was prepended at session start to split the proof-manifest into per-phase files before any new code touched it). RFC-0038 §"Stop conditions / acceptance" Phase-5 checklist ticked (7 boxes). New host-only crate `source/libs/nexus-evidence/` owns canonicalization + Ed25519 sign/verify + secret scan; 102-byte signature wire format (`magic="NXSE" || version=0x01 || label || hash[32] || sig[64]`); `KeyLabel::{Ci, Bringup}` baked into the signature so `verify --policy=ci` rejects bringup-signed bundles. `Bundle::seal` returns `Result<Bundle, EvidenceError>` (callers must handle `EvidenceError::SecretLeak`). Post-pass evidence pipeline wired into `scripts/qemu-test.sh` (single bundle) and `tools/os2vm.sh` (per-node A/B bundles). CI gate: `CI=1` ⇒ seal mandatory + rejects `NEXUS_EVIDENCE_DISABLE=1`; failure to assemble or seal is fatal. CI key resolved from env (`NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64`); bringup key from `~/.config/nexus/bringup-key/private.ed25519` with mandatory mode `0600` check. Deny-by-default secret scanner refuses to seal bundles containing PEM private keys, bringup-key paths, `*PRIVATE_KEY*=…` env-style assignments, or ≥64-char base64 high-entropy blobs. `tools/{seal,verify}-evidence.sh` shell wrappers; `tools/{gen-bringup-key.sh, gen-ci-key.sh}` for key generation. `keys/evidence-ci.pub.ed25519` placeholder + rotation procedure documented in `keys/README.md`. 40 tests across 6 integration files in `nexus-evidence` (5 assemble + 6 canonical_hash + 4 key_separation + 5 qemu_seal_gate + 7 scan + 13 sign_verify); `cargo clippy -p nexus-evidence --all-targets -- -D warnings` clean; `just dep-gate` clean (zero new forbidden deps; `ed25519-dalek` was already in OS graph via `userspace/updates`). QEMU `SELFTEST:` ladder for `PROFILE=full` byte-identical to pre-Phase-5 baseline.

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

Post-closure docs supplement (commits `65d299d` + `f52cf60`, 2026-04-17, no code-behavior change):
- `docs/adr/0027-selftest-client-two-axis-architecture.md` — architectural contract anchoring the Phase-2 decision (nouns + verbs, `PhaseCtx` minimality, phase isolation, aggregator-only `mod.rs`, rejected alternatives, consequences). Phase-3 PRs should reference ADR-0027 directly.
- `source/apps/selftest-client/README.md` — onboarding guide (std vs. os-lite flavors, folder map, marker-ladder contract, decision tree for adding new proofs, determinism rules, common pitfalls).
- CONTEXT headers on all 49 Rust source files in `source/apps/selftest-client/src/` (2026 copyright, SPDX, OWNERS/STATUS/API_STABILITY/TEST_COVERAGE, ADR-0027 reference). 17 pre-existing headers repointed from ADR-0017 to ADR-0027.
- `f52cf60` — pure rustfmt cleanup of 6 files (`phases/{bringup,routing}.rs`, `probes/ipc_kernel/{plumbing,security,soak}.rs`, `updated/stage.rs`) for pre-existing drift exposed by `just test-all` running `fmt-check` first.
- **Verification**: `just test-all` exit 0 (440 s), `just test-network` exit 0 (185 s); logs in `.cursor/test-all.output.log` and `.cursor/test-network.output.log`. Working tree clean except for `uart.log` (test artifact — do not commit).

## Current structural state (post-Phase-3 closure, verified green)
- `source/apps/selftest-client/src/main.rs` = **49** lines (CONTEXT + cfgs + 2 dispatch fns + 3 mod decls — zero logic; rustfmt-canonical floor).
- `source/apps/selftest-client/src/host_lite.rs` = **78** lines (host slice — std + no-std-host `pub(crate) fn run()`; sibling-flattened from `host_lite/mod.rs` per the P3-01 single-file rule).
- `source/apps/selftest-client/src/os_lite/mod.rs` = **50** lines (12 `mod` decls + 14-line `pub fn run()` dispatch; within the 80-LoC arch-gate ceiling; minor expansion from 31 LoC for CONTEXT clarification — structurally unchanged).
- `pub fn run()` body = **14 lines** (`PhaseCtx::bootstrap()?` + 12 phase calls).
- 13 single-file `name/mod.rs` flattened to `name.rs` (P3-01): `os_lite/services/{bootctl,bundlemgrd,execd,keystored,logd,metricsd,policyd,samgrd,statefs}/mod.rs`, `os_lite/{mmio,vfs,timed}/mod.rs`, `os_lite/dsoftbus/quic_os/mod.rs`. Pure `git mv`, history preserved.
- `scripts/check-selftest-arch.sh` (167 LoC, executable) + `source/apps/selftest-client/.arch-allowlist.txt` (50 LoC, 3 sections) + `justfile` `arch-gate` recipe chained into `dep-gate` (P3-03).
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

## Phase-3 plan (4 cuts) — CLOSED 2026-04-17

Behavior-preserving structural completion executed under Cursor-internal plan `task_0023b_phase_3_ee96d119.plan.md`. Marker order, marker strings, reject behavior held frozen across all 4 cuts (119 markers byte-identical vs pre-Phase-3 baseline at every cut). Phase-2 Proof-Floor cadence applied after every cut.

| Cut | Scope | Status |
|---|---|---|
| P3-01 | Flatten 13 single-file `name/mod.rs` to `name.rs`: `services/{bootctl,bundlemgrd,execd,keystored,logd,metricsd,policyd,samgrd,statefs}/mod.rs`, `{mmio,vfs,timed}/mod.rs`, `dsoftbus/quic_os/mod.rs`. Pure `git mv`, no parent edits, history preserved. | done |
| P3-02 | Extract host-pfad `run()` from `main.rs` to `host_lite.rs::run()` (then sibling-flattened from `host_lite/mod.rs` per the P3-01 rule). `main.rs` shrunk 122 → 49 LoC (rustfmt-canonical floor; zero logic). Both std + no-std-host cfg branches preserved. | done |
| P3-03 | `scripts/check-selftest-arch.sh` (167 LoC) enforces 5 mechanical rules; `just arch-gate` recipe chained into `just dep-gate`; `source/apps/selftest-client/.arch-allowlist.txt` (3 sections) baselines current escapes. Synthetic-violation tests confirmed rules 2/3/4 fire with `file:line`. | done |
| P3-04 | Standards review: `#[must_use]` redundant on `Result` fns (`core::result::Result` already `#[must_use]`); `Slot(u32)` newtype deferred to Phase 4 with explicit `TODO(TASK-0023B Phase 4)` note in `os_lite/context.rs` (~16 call sites across 8 files; non-mechanical); Send/Sync intent comment added to `os_lite/context.rs` documenting single-HART/single-task runtime invariant (no marker traits introduced). | done |

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

## Phase-5 closure record (chronological, 7 cuts; P5-00 prepended at session start)

Cuts executed in plan order (P5-00 prepended to split the proof-manifest into per-phase files before any new code touched it):

| Cut | Scope | Closed |
|---|---|---|
| P5-00 | proof-manifest layout split v1 → v2: `source/apps/selftest-client/proof-manifest.toml` (1433 LoC) split into a directory tree (`manifest.toml` + `phases.toml` + `markers/*.toml` + `profiles/*.toml`); `[meta] schema_version = "2"`; `nexus-proof-manifest` parser extended with `[include]` glob expansion (lex-sorted, conflict-checked) keeping v1 single-file back-compat. `scripts/qemu-test.sh`, `tools/os2vm.sh`, `selftest-client/build.rs`, and the CLI repointed to `proof-manifest/manifest.toml`. | done |
| P5-01 | `nexus-evidence` skeleton + canonical hash spec: new host-only crate `source/libs/nexus-evidence/` with `Bundle` + per-artifact subtypes, `canonical_hash` (`H(meta) \|\| H(manifest_bytes) \|\| H(uart_normalized) \|\| H(sorted(trace)) \|\| H(sorted(config))`), 6 integration tests in `tests/canonical_hash.rs`. Spec authored in `docs/testing/evidence-bundle.md`. | done |
| P5-02 | Bundle assembly + trace.jsonl extractor + config.json builder + `nexus-evidence` CLI: `Bundle::assemble`, `extract_trace` (substring-against-all-manifest-literals; `[ts=…ms]` timestamp prefix; deny-by-default for orphan `SELFTEST:` / `dsoftbusd:` lines), `gather_config`, reproducible `tar.gz` packing in `bundle_io.rs` (`mtime=0`, `uid=0`, `gid=0`, mode `0o644`, lex-sorted entries, gzip OS byte fixed). CLI ships `assemble / inspect / canonical-hash`. 5 integration tests in `tests/assemble.rs`. | done |
| P5-03 | Ed25519 sign/verify + `tools/seal-evidence.sh` + `tools/verify-evidence.sh` + 5 tamper classes: `ed25519-dalek` (already in OS graph via `userspace/updates`). 102-byte signature wire format (`magic="NXSE" \|\| version=0x01 \|\| label \|\| hash[32] \|\| sig[64]`); `KeyLabel::{Ci, Bringup}` baked into the signature so `verify --policy=ci` rejects bringup-signed bundles. CLI extended with `seal / verify / keygen`. 13 integration tests in `tests/sign_verify.rs`. Placeholder `keys/evidence-ci.pub.ed25519` checked in. | done |
| P5-04 | Key separation (CI env vs bringup file) + `tools/{gen-bringup-key.sh, gen-ci-key.sh}` + secret-scan reject: `nexus_evidence::key::from_env_or_dir` (CI: `NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64`; bringup: `~/.config/nexus/bringup-key/private.ed25519` with mandatory `0600` perm check). Deny-by-default secret scanner in `src/scan.rs` (PEM blocks, `bringup-key/private` paths, `*PRIVATE_KEY*=…` env-style assignments, ≥64-char base64 high-entropy blobs) wired into `Bundle::seal` (now returns `Result<Bundle, EvidenceError>`); `Bundle::seal_with(&allowlist)` escape hatch for tests. `.gitignore` rejects `**/private.ed25519` belt-and-braces. 11 integration tests (7 scan + 4 key_separation). | done |
| P5-05 | `scripts/qemu-test.sh` + `tools/os2vm.sh` post-pass seal integration + CI gate: post-pass evidence pipeline wired into `qemu-test.sh` (single bundle) and `os2vm.sh` (per-node A/B bundles `…-a.tar.gz` / `…-b.tar.gz`). Env knobs: `NEXUS_EVIDENCE_SEAL=1`, `CI=1` (implies seal + rejects `NEXUS_EVIDENCE_DISABLE=1`), `NEXUS_EVIDENCE_DISABLE=1`. Label resolution: CI key when env set, bringup otherwise. Failure to assemble or seal is fatal. 5 integration tests in `tests/qemu_seal_gate.rs`. | done |
| P5-06 | Phase-5 closure: `docs/testing/evidence-bundle.md` final pass (§3a Assembly, §3b Signing & verification, §3c Key separation, §3d Secret scanner, §5 Operational gates with the env-knob matrix and CI hard gates). RFC-0038 §"Stop conditions / acceptance" Phase 5 ticked (7 boxes). `.cursor/{handoff,current_state,next_task_prep}` synced. Phase-6 plan to be authored at the start of the Phase-6 session as a separate plan file. | done |

### Phase-5 hard gates (verified at closure)

| Rule | Mechanism | Status |
|---|---|---|
| Successful run without sealed bundle = CI failure when `CI=1` (or `NEXUS_EVIDENCE_SEAL=1`) | post-pass block in `scripts/qemu-test.sh` + `tools/os2vm.sh` | enforced |
| `NEXUS_EVIDENCE_DISABLE=1` rejected when seal is mandatory | post-pass block + `tests/qemu_seal_gate.rs` | enforced |
| Bringup-signed bundle validated under `--policy=ci` = rejected | `KeyLabel` byte in signature wire format + `Bundle::verify` | enforced |
| Tampered bundle validates = test failure (5 tamper classes) | `tests/sign_verify.rs` | enforced |
| Secret material in any artifact refuses to seal | `src/scan.rs` deny-by-default + `Bundle::seal` returns `Err(SecretLeak)` | enforced |
| Bundle missing any required artifact | `bundle_io::read_unsigned` + `Bundle::verify` | enforced |
| Reproducible `tar.gz` (byte-identical for same inputs) | `mtime=0` + `uid=0` + `gid=0` + mode `0o644` + lex-sorted entries + fixed gzip OS byte | enforced |
| Bringup key file with mode ≠ `0600` rejected | `key::check_perm_0600` → `EvidenceError::KeyMaterialPermissions` | enforced |
| `nexus-evidence` stays host-only | `cargo tree -i ed25519-dalek` (dep was already in OS graph via `userspace/updates`); `nexus-evidence` itself host-only | enforced |
| `PROFILE=full` marker ladder byte-identical to pre-Phase-5 baseline | `pm_mirror_check` on every `qemu-test.sh` run | enforced |

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
- `main.rs` is dispatch-only at 49 LoC after Cut P3-02; Phase 4 cuts must not touch it (changes belong in manifest + `os_lite/`).
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
- **Active plan (Cursor-internal, do not edit during execution)**: TBD — `/home/jenning/.cursor/plans/task-0023b_phase-6_<hash>.plan.md` to be authored at the start of the Phase-6 session as a separate plan file, scoped to 6 cuts only (P6-01 → P6-06), replay-capability-driven, mirroring the Phase-3 / Phase-4 / Phase-5 plan format.
- **Resume command (when user says "go")**: switch to **plan mode** to author the Phase-6 plan; then switch to agent mode, mark P6-01 todo `in_progress`, and execute the `tools/replay-evidence.sh` skeleton (extract bundle, validate signature via `nexus-evidence verify`, pin git-SHA, set recorded env + kernel cmdline + QEMU args, invoke `just test-os PROFILE=<recorded>`, capture fresh trace). After each cut: `RUSTFLAGS='--cfg nexus_env="os" -W unexpected_cfgs -W dead_code' cargo check -p selftest-client --no-default-features --features os-lite --target riscv64imac-unknown-none-elf` → `cargo test -p dsoftbusd -- --nocapture` → `just test-dsoftbus-quic` → `just test-os PROFILE=full` (verify-uart + evidence post-pass; ladder byte-identical vs pre-Phase-5 baseline) → `cargo test -p nexus-proof-manifest -- --nocapture` → `cargo test -p nexus-evidence -- --nocapture` → `rustfmt +stable <touched .rs>` → `just dep-gate` (chains `arch-gate` first; arch-gate is 6/6 rules) → `just lint` → `cargo clippy -p nexus-evidence --all-targets -- -D warnings` → `tools/verify-evidence.sh target/evidence/<latest>` returns 0. From P6-01 onward also: `tools/replay-evidence.sh target/evidence/<latest>` produces a fresh bundle whose `trace.jsonl` exact-matches the original (modulo the documented allowlist).
- **Phase 6 closure trigger**: tick RFC-0038 Phase-6 checklist (6 boxes); sync `.cursor/{handoff/current.md, next_task_prep.md, current_state.md}`; refresh `tasks/STATUS-BOARD.md`, `tasks/IMPLEMENTATION-ORDER.md`, `docs/testing/index.md`; mark `TASK-0023B` as **CLOSED** (all 6 phases complete: Phase 1 + Phase 2 + Phase 3 + Phase 4 + Phase 5 + Phase 6); unblock `TRACK-OS-PROOF-INFRASTRUCTURE` and extract first candidate (likely CAND-DSC-010 lint crate or CAND-OBS-010 per-phase budgets) into a real `TASK-XXXX`.
