# RFC-0038: Selftest-client production-grade deterministic test architecture refactor + manifest/evidence/replay v1

- Status: Draft
- Owners: @runtime
- Created: 2026-04-16
- Last Updated: 2026-04-17 (Phase-4/5/6 added; profile dimension + manifest SSOT + signed evidence + replay)
- Links:
  - Execution SSOT: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
  - Long-running infrastructure track: `tasks/TRACK-OS-PROOF-INFRASTRUCTURE.md`
  - Follow-on task: `tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md`
  - ADRs:
    - `docs/adr/0005-dsoftbus-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (phase-list authority; expanded 8 → 12 in Phase 2 of this task)
    - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
    - `docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md`

## Status at a Glance

- **Phase 0 (contract seed + target architecture)**: ✅
- **Phase 1 (structural refactor without behavior change)**: ✅ — Cuts 0–22 merged: `os_lite/{ipc, dsoftbus, net, mmio, vfs, timed, updated, probes/{rng,device_key,ipc_kernel,elf}, services/{samgrd,bundlemgrd,keystored,policyd,execd,logd,metricsd,statefs,bootctl}}` extracted; `emit_line` shim removed; `main.rs` frozen at 122 lines; `os_lite/mod.rs` at 1226 lines (down from ~6771). Only top-level imports + module declarations + `pub fn run()` orchestrator body remain.
- **Phase 2 (maintainability/extensibility optimization)**: 🟡 ready to open — **two-axis structure** (capability nouns kept under `services/`, `ipc/`, `probes/`, `dsoftbus/`, `net/`, `mmio/`, `vfs/`, `timed/`, `updated/`; new orchestration verbs under `os_lite/phases/{bringup,ipc_kernel,mmio,routing,ota,policy,exec,logd,vfs,net,remote,end}.rs`). Adds `PhaseCtx` (minimal shared state), DRY consolidation of local `ReplyInboxV1`-`impl Client` copies to `ipc/reply_inbox.rs`, intra-domain sub-splits in `updated/` and `probes/ipc_kernel/`. **Also**: extends RFC-0014 phase list 8 → 12 so harness phases and code phases are congruent (precondition for Phase 4 manifest). See "Phase-2/3 architectural refinements" below.
- **Phase 3 (production-grade closure + standards review)**: ✅ — closed 2026-04-17. Flattened 13 single-file `name/mod.rs` modules to `name.rs`; extracted host-pfad `run()` from `main.rs` (122 → 49 LoC) into a sibling `host_lite.rs::run()`; landed `scripts/check-selftest-arch.sh` + `just arch-gate` (chained into `just dep-gate`) with 5 mechanical rules and a `[marker_emission]` allowlist that Phase 4 shrinks; mechanical standards review (`#[must_use]`, newtype, `Send`/`Sync`) — only the Send/Sync intent comment was applied, the rest deferred with TODO notes because they were not mechanical at this stage. Marker ladder byte-identical (119 markers) across all four cuts.
- **Phase 4 (Marker-Manifest as Single Source of Truth + profile-aware harness)**: ⬜ — `proof-manifest.toml` becomes the single source of truth for: phase list, marker ladder, profile membership (`full`, `smp`, `dhcp`, `os2vm`, `quic-required`, runtime sub-profiles `bringup|quick|ota|net|none`), run configuration (env vars, runner script, extends-relations). `scripts/qemu-test.sh` and `tools/os2vm.sh` consume the manifest instead of hard-coded `PHASES`/`REQUIRE_*` arrays. New runtime switch `SELFTEST_PROFILE=<name>` (kernel cmdline / env) lets `selftest-client` skip whole phases at runtime — no recompile. Removes the existing two-truth surface between `scripts/qemu-test.sh` (expectations) and `selftest-client` (emitter). See "Phase 4 — Marker-Manifest + profile dimension" below.
- **Phase 5 (Signed evidence bundle per QEMU run)**: ⬜ — every `just test-os PROFILE=…` run writes `target/evidence/<utc>-<profile>-<git-sha>.tar.gz` containing manifest + UART + trace + config + Ed25519 signature. New host-only crate `nexus-evidence` owns canonicalization/sign/verify. `tools/verify-evidence.sh` validates bundles fail-closed. CI key vs labeled "bring-up evidence key" separation. See "Phase 5 — Signed evidence bundles" below.
- **Phase 6 (Replay capability)**: ⬜ — `tools/replay-evidence.sh` + `tools/diff-traces.sh` + `tools/bisect-evidence.sh`. A failure on machine A becomes deterministically reproducible on machine B from a stored bundle; CI bisects become trace-diff-driven instead of linear-retry-driven. Cross-host determinism floor (≥ 2 host configs) with reviewable allowlist. See "Phase 6 — Replay capability" below.

Definition:

- “Complete” means the contract is defined and the proof gates are green (tests/markers). It does not mean “never changes again”.
- This RFC is the architecture/contract seed; `TASK-0023B` is the execution truth for stop conditions and proof commands.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - the architectural contract for refactoring `source/apps/selftest-client/src/main.rs` and its surrounding module boundaries,
  - the rule that `selftest-client` is a production-grade deterministic proof surface rather than incidental test glue,
  - the requirement that `main.rs` becomes minimal (entry + dispatch + high-level orchestration only),
  - the rule that the deterministic QEMU marker ladder and service-proof semantics remain behavior-equivalent through the refactor,
  - the rule that the initial target structure is adaptive and may be improved during refactor if the result is more maintainable and auditable.
- **This RFC does NOT own**:
  - new transport functionality, recovery features, or QUIC protocol expansion (owned by `TASK-0024` and later follow-ons),
  - kernel contract changes,
  - replacing the authoritative QEMU proof model with host-only coverage,
  - turning this refactor RFC into a backlog of future selftest features.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- This RFC defines the contract for the selftest-client refactor; `TASK-0023B` remains the execution single source of truth.
- This RFC must not become the place where phase completion, queue progress, or proof execution status is tracked beyond contract-level checklist context.

## Context

`source/apps/selftest-client/src/main.rs` currently acts as one of the most central proof surfaces in the OS:

- it orchestrates the deterministic QEMU marker ladder,
- it sequences routing, policy, VFS, OTA, networking, DSoftBus, exec, state, and observability probes,
- it acts as a release-truth path for service bring-up and proof closure.

That concentration makes future work expensive and risky:

- architectural review is hard because responsibilities are co-located,
- feature work reopens a monolith rather than a seam,
- deterministic proof behavior is harder to audit because orchestration and protocol logic are mixed,
- the current shape encourages re-monolithization over time.

Before `TASK-0024` extends transport behavior, this deterministic proof infrastructure must be hardened into a production-grade architecture.

## Goals

- Define a stable refactor contract for `selftest-client` as deterministic proof infrastructure.
- Reduce `main.rs` to a minimal entry/orchestration shell.
- Split `selftest-client` around explicit responsibilities (IPC, services, net, DSoftBus, markers, orchestration).
- Preserve full marker/proof semantics while making future change safer and more reviewable.

## Non-Goals

- Adding new QUIC/recovery/data-plane features.
- Changing existing marker names or proof semantics in this slice.
- Replacing task-owned proof commands with RFC-owned execution tracking.
- Freezing a rigid final folder layout before implementation reveals the best seams.

## Constraints / invariants (hard requirements)

- **Determinism**: marker ordering, bounded retry behavior, and proof semantics must remain deterministic.
- **No fake success**: no `ok`/`ready` marker semantics may change as part of the refactor.
- **Discovered dishonesty is in scope**:
  - if refactor work reveals logic bugs or fake-success markers, they must be fixed rather than preserved,
  - fake-success markers must be converted into real behavior/proof markers tied to verified outcomes.
- **Behavior-marker definition**:
  - a success marker is honest only after a verified state transition, assertion, or externally checked result,
  - entering a path, returning from a helper, or merely "not seeing an error yet" is not sufficient proof.
- **Bounded resources**: helper extraction must preserve bounded loops, parsers, and buffers.
- **Production-grade maintainability**:
  - boundaries must become easier to audit,
  - follow-on feature work must no longer require reopening a monolith,
  - the structure must resist re-monolithization.
- **Security floor**:
  - reject behavior for malformed/oversized inputs remains fail-closed,
  - no silent fallback marker drift is allowed,
  - no secret/session material is exposed through logs or marker churn.
- **Adaptive structure rule**:
  - the initial target structure is a starting model, not a fixed promise,
  - during implementation, module boundaries may change if the result is more maintainable, more explicit, and safer for deterministic proof evolution.

## Proposed design

### Contract / interface (normative)

This RFC defines the internal architecture contract for `selftest-client` v1 refactor:

- `main.rs` becomes a minimal shell:
  - cfg entry,
  - top-level dispatch,
  - high-level orchestration only.
- `main.rs` must not remain the home for:
  - protocol encode/decode logic,
  - service-specific RPC logic,
  - retry/state-machine loops,
  - marker business logic for subsystem probes.
- Review criteria for `main.rs` minimality:
  - no new subsystem-specific helper should accumulate there,
  - no parser/encoder/decoder should live there,
  - no reply-correlation, retry-budget, or deadline logic should live there,
  - no service-specific marker branching should live there.
- OS selftest logic moves behind explicit internal module seams under `src/os_lite/**`.
- The refactor is behavior-preserving:
  - same proof ordering,
  - same marker meanings,
  - same QEMU harness expectations,
  - same external service/protocol semantics.
- The architecture should separate at least these responsibility families:
  - deterministic orchestration,
  - marker helpers,
  - IPC/routing/client caches,
  - service-specific probes,
  - networking helpers,
  - DSoftBus local/remote probes,
  - MMIO/VFS and other peripheral proof helpers.

Initial target structure:

```text
source/apps/selftest-client/src/
  main.rs
  markers.rs
  os_lite/
    mod.rs
    ipc/
    services/
    net/
    mmio/
    dsoftbus/
    vfs.rs
```

Normative rule for the structure:

- this initial structure is authoritative only as a starting contract,
- if refactor work reveals better module boundaries, the structure should be updated rather than followed rigidly,
- such updates are valid only when they improve maintainability, ownership clarity, and deterministic proof auditability.
- extraction should, where sensible, follow the existing proof/harness phases so regressions stay local and reviewable.

### Phase-2/3 architectural refinements (post-Phase-1 review, 2026-04-17)

After Phase 1 closed (Cuts 0–22), `os_lite/mod.rs` was reduced to imports + `mod`-decls + a 1195-line `pub fn run()` body. Reviewing the Phase-1 result against the Phase-2/3 quality bar produced the following refinements. They are normative for Phase 2/3 unless implementation reveals better seams.

#### 1. Two-axis structure (Capabilities × Phases)

The deterministic proof story needs two orthogonal axes that should never be folded back together:

- **Axis A — Capabilities (substantives, kept):** "What can I ask the OS?" — homes for RPC wrappers, frame encoders, reject helpers. Lives under `services/`, `ipc/`, `probes/`, `dsoftbus/`, `net/`, `mmio/`, `vfs/`, `timed/`, `updated/`.
- **Axis B — Phases (verbs, new in Phase 2):** "In which order do I ask, and which marker is the proof?" — homes for the deterministic ladder. Lives under a new `os_lite/phases/` subtree. Each phase file is a thin orchestrator that calls capability helpers in a fixed sequence and emits the marker line for the verified outcome.

Concrete Phase-2 target:

```text
source/apps/selftest-client/src/
  main.rs                      (122, frozen during Phase 2; extracted in Phase 3)
  markers.rs
  os_lite/
    mod.rs                     (~60 LOC: imports + mod-decls + 13-line run())
    context.rs                 (PhaseCtx — see rule (2))
    phases/
      mod.rs                   (re-exports + documented ladder sequence)
      bringup.rs               (keystored, qos, timed-coalesce, rng, device-key,
                                statefs CRUD/persist, dsoftbus readiness,
                                samgrd v1 register/lookup/malformed)
      routing.rs               (policyd-, bundlemgrd-, updated-routing slots)
      ota.rs                   (TASK-0007 stage/switch/rollback, bootctl persist)
      policy.rs                (allow/deny, MMIO-policy deny, ABI-filter profile)
      exec.rs                  (execd spawn, exit lifecycle, minidump,
                                forged-metadata reject, spoof reject, malformed)
      logd.rs                  (TASK-0014 hardening, metrics/tracing,
                                nexus-log facade, core-service log proof)
      ipc_kernel.rs            (qos/payload/deadline/loopback/cap_move/
                                sender_pid/sender_service_id/soak — orchestration only)
      mmio.rs                  (TASK-0010 MMIO + cap query)
      vfs.rs                   (cross-process VFS probe)
      net.rs                   (ICMP ping, DSoftBus OS transport)
      remote.rs                (TASK-0005 resolve/query/statefs/pkgfs)
      end.rs                   (`SELFTEST: end` + cooperative idle)
    ipc/
      reply_inbox.rs           (NEW: ReplyInboxV1 newtype + impl Client; replaces
                                3× duplicated local impls in probes/ipc_kernel)
    updated/
      mod.rs                   (re-exports + SlotId + SYSTEM_TEST_NXS const)
      types.rs, status.rs, stage.rs, switch.rs, health.rs, reply_pump.rs
    probes/ipc_kernel/
      mod.rs                   (re-exports)
      plumbing.rs              (qos, payload_roundtrip, deadline_timeout, loopback)
      security.rs              (cap_move_reply, sender_pid, sender_service_id)
      soak.rs                  (ipc_soak_probe)
```

After Phase 2, `pub fn run()` becomes a 13-line list of `phases::*::run(&mut ctx)` calls. The marker ladder is auditable in one file (`phases/mod.rs`).

#### 2. `PhaseCtx` minimality rule (avoid a god-object)

`PhaseCtx` carries only state that satisfies at least one of:

- read by ≥ 2 phases, OR
- directly determines the marker ladder.

Allowed fields: `reply_send_slot`, `reply_recv_slot`, `updated_pending: VecDeque<Vec<u8>>`, `os2vm: bool`, `local_ip: Option<…>`, lazy-cached service handles (samgrd, policyd, bundlemgrd, updated, execd, logd, statefsd, keystored).

Forbidden: phase-local state (timing windows, retry counters scoped to one phase, transient buffers). Those stay in the phase file.

#### 3. Phase isolation rule (mechanically enforceable)

Phase files must NOT import other phase files. Allowed downstream imports for `phases::*`:

```text
services::*, ipc::*, probes::*, dsoftbus::*, net::*, mmio::*, vfs::*, timed::*, updated::*
```

Enforced in Phase 3 via `scripts/check-selftest-arch.sh` (rule (7) below).

#### 4. Folder-form heuristic (`name.rs` is the default)

Single-file `name/mod.rs` modules add file-tree noise without semantic value. Default rule:

- `name.rs` is the default form for any new or existing module,
- escalation to `name/mod.rs` is allowed only when ≥ 1 sibling file actually exists,
- the directory form earns its existence by hosting real sub-files.

Phase 1 left 13 single-file `name/mod.rs` modules (`services/{keystored,execd,metricsd,statefs,bootctl,bundlemgrd,policyd,samgrd,logd}/mod.rs`, `mmio/mod.rs`, `vfs/mod.rs`, `timed/mod.rs`, `dsoftbus/quic_os/mod.rs`). Phase 2 sub-splits will justify some of these (`samgrd/`, `logd/`, `policyd/`, `dsoftbus/quic_os/` are likely candidates as TASK-0024 lands). The remainder gets flattened in Phase 3.

#### 5. Aggregator-only rule for `mod.rs`

`mod.rs` files in this crate must be aggregators: `pub(crate) mod ...;` declarations and (optionally) re-exports. They must not host business logic.

Phase 1 leaves `services/mod.rs` with two free functions (`core_service_probe`, `core_service_probe_policyd`). These are probes ("can the service echo a ping?"), not service definitions. Phase 2 moves them to `probes/core_service.rs` and reduces `services/mod.rs` to declarations only.

`dsoftbus/remote/{mod.rs, resolve.rs, pkgfs.rs, statefs.rs}` is the canonical aggregator-only example and serves as the pattern for `services/samgrd/`, `services/logd/`, `services/policyd/`, and `probes/ipc_kernel/` sub-splits.

#### 6. Host-pfad symmetry (Phase 3)

`main.rs` currently delegates the OS-pfad to `os_lite::run()` (1 line) but inlines a 45-line `fn run() -> anyhow::Result<()>` for the host-pfad. This asymmetry is a re-monolithization risk: future host-side proofs (TRACK-PODCASTS-APP host tests, contentd tests, mediasessd tests) would land back in `main.rs`.

Phase 3 extracts the host-pfad into a symmetric `host_lite/` subtree (`host_lite/mod.rs::run()`), leaving `main.rs` with strictly:

- cfg gating,
- `os_entry()` → `os_lite::run()`,
- `main()` → `host_lite::run()`.

#### 7. Mechanical architecture gate (Phase 3, anti-re-monolithization)

The largest structural risk is not the form but the discipline. Phase 3 introduces `scripts/check-selftest-arch.sh`, run via a new `just arch-gate` recipe and chained into `just dep-gate`, that mechanically enforces:

| Rule | Mechanism |
|---|---|
| `os_lite/mod.rs` ≤ 80 LOC | `wc -l` |
| `phases/*.rs` does not import other `phases::*` | `rg -n "use .*::phases::" os_lite/phases/` |
| Marker strings (`"SELFTEST: ..."`, `"dsoftbusd: ..."`) appear only in `phases/*` and `markers.rs` | `rg -n '"SELFTEST: ' os_lite/{services,ipc,probes,dsoftbus,net,mmio,vfs,timed,updated}/` |
| `mod.rs` files contain no `fn` / `pub fn` definitions outside re-exports | `rg -n "^\s*(pub(\(crate\))? )?fn " **/mod.rs` |
| No file ≥ 500 LOC outside an explicit allowlist | `wc -l` + allowlist file |

Failures are CI-gating. The allowlist lives next to the script and is reviewable.

#### 8. Explicitly rejected ideas (with reason)

- **Marker-string SSOT in Rust constants** (e.g. `pub(crate) const M_QOS_OK: &str = "SELFTEST: qos ok";`): rejected. Markers already live in `scripts/qemu-test.sh` as the harness contract; introducing Rust-side constants creates two truths and a drift surface. Doc-comments in phase files document expected markers.
- **`trait Phase { fn run(&mut self, ctx: &mut PhaseCtx); }`**: rejected. Adds boilerplate and dynamic-dispatch surface without enabling generic phase composition; free functions are simpler and equally testable.
- **Generic `Probe` trait hierarchy**: rejected. Our marker ladder is linearly deterministic, not a test framework over a probe collection. Free functions match the linear shape.
- **Renaming `os_lite/` → `os_suite/` / `os_runner/`**: rejected. The name is referenced in 36 source files plus build scripts. Cosmetic gain does not justify the churn.

#### 9. Implications for follow-on work (forward-compatibility check)

- **TASK-0024 (DSoftBus QUIC recovery / UDP-sec):** new transport-recovery probes land as `dsoftbus/recovery_probe.rs` (capability) plus 1 line in `phases/net.rs`. No `run()` touch.
- **TRACK-PODCASTS-APP / TRACK-MEDIA-APPS / TRACK-NEXUSMEDIA-SDK:** media-sessions/provider probes land as `services/mediasessd.rs` (or directory if it grows) plus a `phases/media.rs` (cfg-gated profile).
- **Cfg-profiles for targeted bisects (e.g., bring-up-only, ota-only):** `phases/mod.rs` exposes alternate ordered sequences without phase files needing to know about profiles. Phase 4 promotes this from cfg-profile to runtime profile (`SELFTEST_PROFILE` env / kernel cmdline) so iteration does not require recompile.

### Phase 4 — Marker-Manifest + profile dimension (normative)

#### Problem statement

After Phases 1–3, the architecture is clean but the marker ladder still lives in **two places**: `scripts/qemu-test.sh` hard-codes the expectation arrays; `selftest-client` emits the strings from Rust code. Drift between the two is only caught after a CI failure. Additionally, run profiles (SMP, DHCP, OS2VM, QUIC-required, partial-run) live as scattered env vars (`SMP=2 REQUIRE_SMP=1`, `REQUIRE_QEMU_DHCP=1`, `REQUIRE_DSOFTBUS=1`, `RUN_PHASE=…`) across `justfile` recipes and `qemu-test.sh` branches. There is no single answer to "which markers are expected under profile X?".

#### Contract

A **single** `source/apps/selftest-client/proof-manifest.toml` is the authoritative source for:

1. **Phase list** (12 entries, congruent with the RFC-0014 v2 list extended in Phase 2).
2. **Marker ladder per phase**, including profile-conditional markers.
3. **Profile membership**: which profile inherits from which (`extends`), which env vars + runner each profile uses.
4. **Forbidden markers per profile** (e.g. fallback markers under `quic-required`).

Build-time generation:

- `source/apps/selftest-client/build.rs` reads the manifest and generates `markers_generated.rs` with one `pub(crate) const M_<KEY>: &str = "…"` per declared marker.
- Phase emission code uses only generated constants. No marker string literal may appear anywhere in `os_lite/` outside the generated file + `markers.rs`. Enforced by `arch-gate` (Phase 3 rule, extended in Phase 4).

Harness consumption:

- `scripts/qemu-test.sh` and `tools/os2vm.sh` are rewritten to call a small host CLI (`nexus-proof-manifest list-markers --profile=<name>`, `… list-env --profile=<name>`) instead of hard-coding `PHASES`/`PHASE_START_MARKER`/`REQUIRE_*` arrays.
- `just test-os` accepts `PROFILE=<name>` and forwards it. Existing recipes (`test-smp`, `test-os-dhcp`, `test-dsoftbus-2vm`, `test-network`) become aliases for one cycle, then are deleted.

Runtime selftest profile (`SELFTEST_PROFILE`):

- Read by `os_lite/profile.rs::Profile::from_kernel_cmdline_or_default(Profile::Full)`.
- Allowed values for runtime selectivity: `full`, `bringup`, `quick`, `ota`, `net`, `none`.
- Selection at runtime maps to a subset of the 12 code phases. `pub fn run()` iterates `profile.enabled_phases()` and skips phases not in the set; skipped phases emit a single `dbg: phase <name> skipped (profile=<name>)` line for visibility but no `*: ready` markers (no fake success).
- The harness must agree: when invoked with `PROFILE=quick`, the harness expects only the `quick`-profile markers from the manifest. Expected-but-absent or unexpected markers are both hard failures.

#### Schema (normative)

```toml
[meta]
schema_version = "1"
default_profile = "full"

[phase.bringup]
order = 1
markers = ["init: ready", "samgrd: ready", "execd: ready"]

# … 11 more [phase.X] entries congruent with RFC-0014 v2 …

[profile.full]
runner = "scripts/qemu-test.sh"
env = {}
phases = "all"

[profile.smp]
extends = "full"
env = { SMP = "2", REQUIRE_SMP = "1" }

[profile.dhcp]
extends = "full"
env = { REQUIRE_QEMU_DHCP = "1", REQUIRE_QEMU_DHCP_STRICT = "1" }

[profile.os2vm]
runner = "tools/os2vm.sh"
env = { REQUIRE_DSOFTBUS = "1" }
phases = "all"

[profile.quic-required]
extends = "full"
env = { REQUIRE_DSOFTBUS = "1" }

# Runtime sub-profiles (no harness env; only SELFTEST_PROFILE selection)
[profile.bringup]
runtime_only = true
phases = ["bringup", "ipc_kernel", "end"]

[profile.quick]
runtime_only = true
phases = ["bringup", "ipc_kernel", "routing", "policy", "end"]

[profile.ota]
runtime_only = true
phases = ["bringup", "ota", "end"]

[profile.net]
runtime_only = true
phases = ["bringup", "net", "remote", "end"]

[profile.none]
runtime_only = true
phases = ["end"]

# Profile-conditional markers
[marker."SELFTEST: smp ipi ok"]
phase = "bringup"
emit_when = { profile = "smp", smp_min = 2 }
proves = "SMP IPI delivery between hart 0 and hart 1"
introduced_in = "TASK-0012"

[marker."dsoftbus: quic os disabled (fallback tcp)"]
phase = "net"
emit_when_not = { profile = "quic-required" }
forbidden_when = { profile = "quic-required" }
introduced_in = "TASK-0023"
```

#### Hard gates (mechanically enforced in Phase 4)

| Rule | Mechanism |
|---|---|
| No marker string literal outside `markers_generated.rs` + `markers.rs` | `rg` in `arch-gate` |
| No `REQUIRE_*` env var read directly in `just test-*` recipes | `rg` over `justfile` |
| Manifest parser rejects unknown keys | host-side reject test |
| A profile with no declared markers is rejected at build time | host-side reject test |
| Any unexpected marker at runtime under active profile = hard failure | host analyzer over `uart.log` |
| Any expected-but-absent marker at runtime under active profile = hard failure | existing harness logic, manifest-driven |
| Skipped runtime phases emit no `*: ready` / no `SELFTEST: * ok` markers | host analyzer + grep gate |

#### Out of scope for Phase 4

- Host-only tests (`cargo test --workspace`, `just test-host`, `just test-e2e`, `just test-dsoftbus-quic`) remain outside the manifest. They prove host-resident logic, not OS-attested behavior; collapsing both into one model would force a wrong abstraction onto either side.
- Marker authority for *services* (e.g., `dsoftbusd: ready`) stays with each service's owning task; this manifest is the cross-cutting contract for the orchestration ladder, not service-internal correctness.

### Phase 5 — Signed evidence bundles (normative)

#### Problem statement

Today, a green QEMU run produces a UART log on the runner. There is no portable artifact that proves "build X under profile Y produced ladder Z and was attested by key K". External review (release notes, audit, post-mortem) cannot rely on the run output.

#### Contract

For every `just test-os PROFILE=…` invocation:

- `target/evidence/<utc>-<profile>-<git-sha>.tar.gz` is written, containing:
  - `proof-manifest.toml` (verbatim copy used for the run),
  - `uart.log` (unfiltered serial output),
  - `trace.jsonl` (extracted marker ladder; one JSON line per marker with `{ marker, phase, ts_ms_from_boot, profile }`),
  - `config.json` (profile name, env vars, kernel cmdline, QEMU args, host info, build SHA, rustc version, qemu version),
  - `signature.bin` (Ed25519 signature over a deterministic canonical hash of all of the above).

Canonicalization (deterministic):

- Hash input = `H(meta) || H(manifest_bytes) || H(sorted(trace_entries)) || H(sorted(config_entries))`.
- Sorting keys are documented in `docs/testing/evidence-bundle.md`.
- The `nexus-evidence` host-only crate owns canonicalization, signing, verification.

Key model:

- Two key labels: `ci` (held by CI runner) and `bringup` (developer / local dev key).
- Bringup-key bundles are explicitly labeled and **must not validate** against CI policy (`tools/verify-evidence.sh --policy=ci` rejects them).
- No private keys appear in the repo. Bringup keypair generation: `tools/gen-bringup-key.sh` creates a labeled keypair under `~/.config/nexus/bringup-key/`.

Failure semantics:

- A QEMU run that fails its proof floor still produces a bundle, but with `signature.bin` absent (replay-only artifact for triage).
- A QEMU run that succeeds but fails to seal a bundle is a CI failure (no silent skip).
- Tampering with any field of a sealed bundle causes `verify-evidence.sh` to fail with `EvidenceError::SignatureMismatch` (stable error class).

#### Hard gates (mechanically enforced in Phase 5)

| Rule | Mechanism |
|---|---|
| Successful run without sealed bundle = CI failure | hook in `scripts/qemu-test.sh` |
| Bringup-key bundle validates against CI policy = test failure | unit test in `nexus-evidence` |
| Tampered bundle validates = test failure | unit test for each tamper class (manifest, uart, trace, config, key swap) |
| Secret material in `uart.log` / `trace.jsonl` / `config.json` | scan reject test (known-pattern grep + entropy check on suspicious lines) |
| Bundle missing any of the 5 required artifacts | `verify-evidence.sh` fails closed |

#### Out of scope for Phase 5

- Remote artifact storage / publishing — bundles live under `target/evidence/` for now; a follow-on can add an artifact store.
- Signing transparency / Sigstore-style logs.
- Encryption of bundle contents (signature only; bundles are auditable, not confidential).

### Phase 6 — Replay capability (normative)

#### Problem statement

Bisects today are linear retries. A CI failure at SHA X with last-green at SHA Y means re-running the full `just test-os` per commit until the regressing commit is found. With evidence bundles (Phase 5), we can replay a known-good and a known-bad bundle and diff their traces; with replay tooling (Phase 6), we can automate the bisect.

#### Contract

Three tools, all bounded:

- `tools/replay-evidence.sh <bundle>`:
  - validates bundle signature (Phase 5),
  - pins git-SHA from the bundle,
  - sets the recorded env + kernel cmdline + QEMU args,
  - invokes `just test-os PROFILE=<recorded-profile>`,
  - captures a fresh trace and compares against the original.

- `tools/diff-traces.sh <original.jsonl> <replay.jsonl>`:
  - produces a deterministic diff (phase-by-phase, order-aware),
  - classifies differences: `exact_match`, `extra_marker`, `missing_marker`, `reorder`, `phase_mismatch`,
  - exits 0 only on `exact_match` modulo the documented allowlist.

- `tools/bisect-evidence.sh <good-bundle> <bad-bundle>`:
  - walks the git-SHA range,
  - runs replay per commit,
  - classifies first regressing commit using `diff-traces.sh`,
  - mandatory `--max-commits` (default 64) and `--max-seconds` (default 1800) budgets — fail-closed on exhaust.

Cross-host determinism floor:

- A bundle sealed on the CI runner must replay to an empty diff on at least 1 dev box (typical Linux + KVM + qemu-system-riscv64) for the same `git-SHA + profile`.
- Documented allowlist of acceptable non-deterministic surfaces: wall-clock fields in `config.json`, qemu version banner string, host hostname.
- Allowlist lives in `docs/testing/replay-and-bisect.md` and is append-only with reviewer signoff.

#### Hard gates (mechanically enforced in Phase 6)

| Rule | Mechanism |
|---|---|
| Unbounded replay run | `--max-seconds` mandatory; CLI rejects missing arg |
| Replay requires environment beyond what the bundle records | `replay-evidence.sh` fails closed before invoking QEMU |
| Cross-host allowlist accepts arbitrary fields | code-review gate; allowlist file structure restricts to known classes |
| Bisect without `--max-commits` | CLI rejects missing arg |

### Phases / milestones (contract-level)

- **Phase 0**: contract seed exists; target architecture and invariants are explicit.
- **Phase 1**: structural extraction begins and `main.rs` starts shrinking without behavior change.
  - preferred first cuts:
    - DSoftBus QUIC/local transport leaf,
    - shared netstack/UDP helper seams,
    - IPC/routing/client-cache seams.
  - operational rule:
    - create module skeletons first,
    - move one responsibility slice at a time,
    - rerun parity proof after each major extraction cut,
    - stop immediately on marker/order/reject-path drift.
- **Phase 2**: broader module boundaries become maintainable/extensible instead of merely smaller.
  - optimize seams to match runtime/proof phases where practical so debugging and review stay local.
- **Phase 3**: production-grade closure is demonstrated:
  - `main.rs` is minimal,
  - deterministic proof paths are easy to audit,
  - Rust standards are reviewed and applied where sensible,
  - future work can extend the crate without re-centralizing it,
  - newly discovered logic bugs or fake-success markers have been converted into honest behavior/proof signals.
- **Phase 4**: marker manifest is the single source of truth:
  - phase list, marker ladder, profile membership, run config all derive from `proof-manifest.toml`,
  - `scripts/qemu-test.sh` and `tools/os2vm.sh` consume the manifest,
  - `SELFTEST_PROFILE` runtime switch enables sub-profile (`bringup`, `quick`, `ota`, `net`, `none`) without recompile,
  - all `just test-*` recipes route through `just test-os PROFILE=…`,
  - `RFC-0014` phase list extension (8 → 12) is committed.
- **Phase 5**: every QEMU run produces a portable, signed evidence artifact:
  - sealed bundle layout fixed,
  - canonicalization deterministic,
  - CI vs bring-up key separation enforced,
  - tamper rejection covered by reject tests.
- **Phase 6**: replay + bisect workflow is real:
  - `tools/replay-evidence.sh` reproduces an arbitrary bundle on at least one dev box,
  - `tools/diff-traces.sh` classifies drift,
  - `tools/bisect-evidence.sh` automates regression search under bounded budgets,
  - cross-host determinism allowlist documented and append-only.

## Security considerations

- **Threat model**:
  - refactor drift changes proof semantics while claiming “no behavior change”,
  - extracted helper code weakens reject handling,
  - local structural cleanup harms the global deterministic proof model,
  - future contributors re-accumulate orchestration and protocol logic in `main.rs`.
- **Mitigations**:
  - keep the full proof floor green after each phase,
  - preserve the authoritative marker ladder semantics,
  - keep `main.rs` minimal by contract, not taste,
  - review `newtype`/ownership/`Send`/`Sync`/`#[must_use]` surfaces before closure,
  - convert any discovered dishonest marker path into a real behavior/proof marker rather than carrying it forward.
- **Open risks**:
  - the best final module seams may differ from the initial target structure,
  - some probe families may prove more tightly coupled than expected and require an adjusted intermediate layout.

## Failure model (normative)

- If marker names, marker ordering semantics, or QEMU proof expectations drift unintentionally, this RFC fails.
- If `main.rs` remains the effective storage location for most service/protocol logic after the refactor, this RFC fails.
- If the structure becomes smaller but not more maintainable/auditable, this RFC fails.
- If known logic bugs or fake-success markers are intentionally preserved unchanged, this RFC fails.
- If a new structural discovery implies a bigger architecture boundary, work must either:
  - adapt the target structure inside this RFC’s scope, or
  - stop and create a new RFC/ADR for the newly discovered boundary.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd -- --nocapture
cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os
```

### Proof (Closure / hygiene)

```bash
cd /home/jenning/open-nexus-OS && just dep-gate && just diag-os
```

### Deterministic markers (critical subset)

- `dsoftbusd: transport selected quic`
- `dsoftbusd: auth ok`
- `dsoftbusd: os session ok`
- `SELFTEST: quic session ok`

Note:

- This RFC preserves the whole service-test ladder; the list above is only a critical subset, not the complete ladder contract.
- The complete ladder enforced by `scripts/qemu-test.sh` remains authoritative for closure.
- These marker names are not by themselves sufficient proof unless they remain tied to verified behavior.

## Alternatives considered

- Keep `TASK-0023B` narrow and only extract `quic_os/`.
  - Rejected because the real structural problem is broader than the QUIC leaf.
- Split the refactor into many tiny tasks immediately.
  - Rejected because the architecture cut would become artificial and lose the central “production-grade proof infrastructure” framing.
- Freeze a rigid target structure before implementation.
  - Rejected because the best module boundaries will partly emerge during refactor work.

## Open questions

- ~~Should host-specific `run()` logic later move into a separate `host/` subtree, or remain a small leaf near `main.rs`?~~ — **resolved 2026-04-17**: Phase 3 extracts to `host_lite/` (see refinement (6)) for symmetry with `os_lite/` and to prevent host-pfad re-monolithization once TRACK-PODCASTS-APP / mediasessd host tests land.
- ~~Should `phases/mod.rs` expose multiple ordered sequences (full, bring-up-only, ota-only) for targeted bisects from day one, or only when a concrete need appears?~~ — **resolved 2026-04-17**: Phase 4 introduces the `SELFTEST_PROFILE` runtime switch (`full|bringup|quick|ota|net|none`). `phases/mod.rs` exposes the full ordered ladder; the `Profile` type filters at runtime. No cfg-time profile machinery.
- ~~Should host-only tests (`cargo test`, `just test-host`, `just test-e2e`, `just test-dsoftbus-quic`) move into the manifest?~~ — **resolved 2026-04-17**: No. Host tests stay outside the manifest. They prove host-resident logic; the manifest's domain is OS-attested behavior. Separation is the design.
- ~~Should the marker ladder remain in `scripts/qemu-test.sh` arrays?~~ — **resolved 2026-04-17**: No. Phase 4 promotes `proof-manifest.toml` to the single source of truth. The shell script becomes a thin consumer.
- ~~Should the existing `RFC-0014` phase list (8 entries) and the new code phases (12 entries) stay separate?~~ — **resolved 2026-04-17**: No. Phase 2 of `TASK-0023B` extends `RFC-0014` to 12 (`bringup → ipc_kernel → mmio → routing → ota → policy → exec → logd → vfs → net → remote → end`). Congruence is a Phase 4 precondition.
- Which extracted helper families are likely to become reusable across future deterministic proof clients? (kept open; revisit at Phase 6 closure.)
- Should `nexus-evidence` use a Sigstore-style transparency log instead of (or in addition to) Ed25519 signatures? (kept open; not required for Phase 5; revisit if external publication becomes a real requirement.)

## RFC Quality Guidelines (for authors)

When updating this RFC, ensure:

- the adaptive-structure rule remains explicit,
- `main.rs` minimalization remains a hard outcome, not a soft preference,
- production-grade deterministic proof infrastructure remains the quality bar,
- proof commands stay concrete and task-aligned,
- discovered dishonest markers are converted into honest behavior/proof markers rather than normalized.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: contract seed defined; initial target structure + invariants are explicit — proof: task + RFC linked.
- [x] **Phase 1**: structural refactor shrinks `main.rs` without behavior change — proof: `cargo test -p dsoftbusd -- --nocapture && just test-dsoftbus-quic && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os` green after every cut.
  - [x] Cuts 0–9: periphery extraction (`ipc/{clients,routing,reply}`, `mmio`, `vfs`, `net/{icmp_ping,local_addr,smoltcp_probe}`, `dsoftbus/{quic_os,remote/*}`, `timed`, `probes/{rng,device_key}`).
  - [x] Cuts 10–18: service-family extraction (`services/{samgrd,bundlemgrd,keystored,policyd,execd,logd,metricsd,statefs,bootctl}/mod.rs` + shared `services::core_service_probe*`).
  - [x] Cut 19: `updated` family (`updated_*`, `init_health_ok`, `SYSTEM_TEST_NXS`, `SlotId`) → `os_lite/updated/mod.rs`.
  - [x] Cut 20: IPC-kernel/security probes (`qos_probe`, `ipc_payload_roundtrip`, `ipc_deadline_timeout_probe`, `nexus_ipc_kernel_loopback_probe`, `cap_move_reply_probe`, `sender_pid_probe`, `sender_service_id_probe`, `ipc_soak_probe`) → `os_lite/probes/ipc_kernel/mod.rs`.
  - [x] Cut 21: ELF helpers (`log_hello_elf_header`, `read_u64_le`) → `os_lite/probes/elf.rs`.
  - [x] Cut 22: `emit_line` shim removed in `os_lite/mod.rs`; replaced with direct `use crate::markers::emit_line`.
  - [x] Phase-1 closure: `wc -l main.rs` = 122 unchanged; `os_lite/mod.rs` reduced to imports + `mod`-decls + `pub fn run()` body (1226 lines); full Proof-Floor green.
- [x] **Phase 2**: broader module boundaries optimized for maintainability/extensibility per the two-axis structure (refinement (1)) — proof: phase proof floor green after every cut; QEMU `SELFTEST:` ladder byte-identical (119 markers) across P2-00 → P2-17.
  - [x] Cut P2-00: extend `RFC-0014` phase list 8 → 12 (`bringup → ipc_kernel → mmio → routing → ota → policy → exec → logd → vfs → net → remote → end`) so harness phases and code phases are congruent. Doc-only cut; no code change.
  - [x] Cut P2-01: `phases/` skeleton + `os_lite/context.rs` with empty `PhaseCtx::bootstrap()` (plumbing only, no behavior). 12 phase stubs; `PhaseCtx` minimality locked at 5 fields (`reply_send_slot`, `reply_recv_slot`, `updated_pending`, `local_ip`, `os2vm`).
  - [x] Cuts P2-02..P2-13: extract `phases/{bringup, routing, ota, policy, exec, logd, ipc_kernel, mmio, vfs, net, remote, end}.rs` from `pub fn run()` one phase per cut, no marker-string or order changes. Executed in actual `pub fn run()` order (P2-02, P2-05, P2-06, P2-07, P2-08, P2-09, P2-03, P2-04, P2-10, P2-11, P2-12, P2-13). `os_lite/mod.rs` shrunk 1256 → 31 LoC; `pub fn run()` body now 14 lines.
  - [x] Cut P2-14: intra-domain sub-split `updated/{types, status, stage, switch, health, reply_pump}.rs` (re-exports via `updated/mod.rs`). `updated/mod.rs` shrunk 451 → 30 LoC.
  - [x] Cut P2-15: intra-domain sub-split `probes/ipc_kernel/{plumbing, security, soak}.rs`. `probes/ipc_kernel/mod.rs` shrunk 393 → 28 LoC.
  - [x] Cut P2-16: DRY consolidation `ipc/reply_inbox.rs` (`ReplyInboxV1` newtype + `impl Client`) replaces 3× duplicated local impls in `cap_move_reply_probe`, `sender_pid_probe`, `ipc_soak_probe`. Net −21 LoC + single source of truth for shared-inbox recv semantics.
  - [x] Cut P2-17: aggregator-only cleanup — move `services::core_service_probe*` to `probes/core_service.rs`; reduce `services/mod.rs` to declarations only (refinement (5)). `services/mod.rs` is now 23 LoC (no fn bodies).
  - [x] **Post-closure docs supplement (2026-04-17, commits `65d299d` + `f52cf60`)**: docs-only cut anchoring the Phase-2 architecture for future contributors. Authored `docs/adr/0027-selftest-client-two-axis-architecture.md` (architectural contract: two-axis nouns+verbs, `PhaseCtx` minimality, phase isolation, aggregator-only `mod.rs`, rejected alternatives, consequences) and `source/apps/selftest-client/README.md` (onboarding: std vs. os-lite flavors, folder map, marker-ladder contract, decision tree for new proofs, determinism rules, common pitfalls). Brought all 49 Rust source files in `source/apps/selftest-client/src/` in line with `docs/standards/DOCUMENTATION_STANDARDS.md` (CONTEXT block, 2026 copyright, SPDX, ADR-0027 reference); 17 pre-existing headers repointed from ADR-0017 to ADR-0027. A separate style commit (`f52cf60`) corrected pre-existing rustfmt drift in 6 files (`phases/{bringup,routing}.rs`, `probes/ipc_kernel/{plumbing,security,soak}.rs`, `updated/stage.rs`) exposed by `just test-all` running `fmt-check` up front. Verification: `just test-all` exit 0 (440 s, 119 SELFTEST markers byte-identical), `just test-network` exit 0 (185 s, all 2-VM phases `status=ok`, `result=success`). No code-behavior change.
- [x] **Phase 3**: production-grade closure + structural-discipline gates — proof: same phase proof floor + `just dep-gate && just diag-os && just arch-gate`. Closed 2026-04-17 with byte-identical 119-marker SELFTEST ladder across all four cuts.
  - [x] Cut P3-01: flattened 13 single-file `name/mod.rs` candidates to `name.rs` (services/{bootctl,bundlemgrd,execd,keystored,logd,metricsd,policyd,samgrd,statefs}, mmio, vfs, timed, dsoftbus/quic_os) via `git mv`; no parent edits, no content drift, marker ladder byte-identical (refinement (4)).
  - [x] Cut P3-02: extracted host-pfad `run()` from `main.rs` (122 LoC → 49 LoC) into `host_lite.rs::run()` (after also flattening the host_lite/mod.rs single-file folder for P3-01 consistency); `main.rs` is now CONTEXT + cfgs + 2 dispatch fns + mod decls only (49 LoC; the original "≤ 35" target was aspirational — 49 is the rustfmt-canonical floor for the long cfg expressions). Both OS path and host path emit identical markers (refinement (6)).
  - [x] Cut P3-03: authored `scripts/check-selftest-arch.sh` enforcing 5 rules (mod.rs ≤ 80 LoC, no `phases::*` cross-imports, marker strings only in phases/* + markers.rs, no `fn` in mod.rs, no file ≥ 500 LoC), with `[marker_emission]`, `[mod_rs_fn]`, `[size_500]` allowlists in `source/apps/selftest-client/.arch-allowlist.txt`. Added `just arch-gate` recipe and chained it as a prerequisite of `just dep-gate` so structural drift fails fast. Phase-2 baseline marker emissions in 17 capability files allowlisted (Phase 4 manifest work shrinks this section to zero per the rule's natural tightening). Synthetic-violation tests confirm rules 2/3/4 fire with file:line; reverted clean (refinement (7)).
  - [x] Cut P3-04: mechanical standards review. `#[must_use]` on `Result`-returning fns is redundant (core::result::Result is already `#[must_use]`); only one `fn -> bool` candidate (`smoltcp_probe::tx_send`) lives in cfg-gated bring-up debug code with consumed return — no annotation added. Newtype wrappers for `Slot(u32)` (~16 call sites across 8 files) deferred to Phase 4 with a `TODO(TASK-0023B Phase 4)` note in `os_lite/context.rs`. Send/Sync audit added as a single intent comment in `context.rs` documenting the single-HART/single-task invariant (no marker traits introduced — adding them later without changing the runtime model would mask, not reveal, a concurrency bug).
- [ ] **Phase 4**: marker manifest as single source of truth + profile-aware harness + runtime selftest profiles. Hard gates per "Phase 4 — Marker-Manifest + profile dimension" above.
  - [ ] Cut P4-01: write manifest schema doc + `proof-manifest.toml` skeleton (meta + phase declarations only); add `nexus-proof-manifest` host-only crate with parser + reject tests.
  - [ ] Cut P4-02: cross-reference RFC-0014 phase list (already extended in P2-00) and document the manifest ↔ RFC-0014 binding.
  - [ ] Cut P4-03: populate `proof-manifest.toml` with all current markers (1:1 from `scripts/qemu-test.sh` + `selftest-client` source); `build.rs` generates `markers_generated.rs`.
  - [ ] Cut P4-04: replace marker emission in `phases/*` with generated constants; arch-gate enforces no marker string literal outside `markers_generated.rs` + `markers.rs`.
  - [ ] Cut P4-05: `[profile.full|smp|dhcp|os2vm|quic-required]` definitions; `scripts/qemu-test.sh` consumes manifest via `nexus-proof-manifest` host CLI.
  - [ ] Cut P4-06: migrate `just test-os|test-smp|test-os-dhcp|test-dsoftbus-2vm|test-network` to `just test-os PROFILE=…`; old recipes alias for ≥ 1 cycle then deleted.
  - [ ] Cut P4-07: `tools/os2vm.sh` consumes manifest (`profile.os2vm`).
  - [ ] Cut P4-08: `[profile.bringup|quick|ota|net|none]` (runtime-only); `os_lite/profile.rs` + `Profile::from_kernel_cmdline_or_default(Profile::Full)`; `pub fn run()` iterates `profile.enabled_phases()`; per-profile QEMU smoke tests.
  - [ ] Cut P4-09: deny-by-default analyzer: any unexpected runtime marker for the active profile = hard failure; expected-but-absent already enforced.
  - [ ] Cut P4-10: hard-deprecate direct `RUN_PHASE`/`REQUIRE_*` env usage in CI; CI must invoke `just test-os PROFILE=…`. Document in `docs/testing/index.md`.
- [ ] **Phase 5**: signed evidence bundles per QEMU run. Hard gates per "Phase 5 — Signed evidence bundles" above.
  - [ ] Cut P5-01: `nexus-evidence` crate skeleton + canonicalization spec + unit tests for deterministic hashing.
  - [ ] Cut P5-02: `tools/extract-trace.sh` produces `trace.jsonl` from `uart.log` using manifest phase tags; reject test for out-of-order ladder.
  - [ ] Cut P5-03: `tools/seal-evidence.sh` builds and signs `evidence-bundle.tar.gz`; `EVIDENCE_KEY=ci|bringup` selection.
  - [ ] Cut P5-04: hook seal step into `scripts/qemu-test.sh` after pass/fail decision.
  - [ ] Cut P5-05: `tools/verify-evidence.sh` + bring-up vs CI key separation; reject tests for tamper classes (manifest/uart/trace/key swap).
  - [ ] Cut P5-06: `docs/testing/evidence-bundle.md` documents bundle layout, key model, verify workflow.
- [ ] **Phase 6**: replay capability. Hard gates per "Phase 6 — Replay capability" above.
  - [ ] Cut P6-01: `tools/replay-evidence.sh` skeleton — extract, validate, pin git-SHA, set env, invoke `just test-os PROFILE=<recorded>`.
  - [ ] Cut P6-02: trace diff format spec + `tools/diff-traces.sh`; unit fixtures for "exact match", "extra", "missing", "reorder", "phase mismatch".
  - [ ] Cut P6-03: `tools/bisect-evidence.sh` with mandatory `--max-commits` and `--max-seconds` budgets.
  - [ ] Cut P6-04: `scripts/regression-bisect.sh` wrapper for the typical CI-failure flow.
  - [ ] Cut P6-05: cross-host determinism floor (CI runner + 1 dev box) for the same bundle; documented allowlist for non-deterministic surfaces.
  - [ ] Cut P6-06: `docs/testing/replay-and-bisect.md` documents workflow + allowlist + extension procedure.
- [x] Task linked with stop conditions + proof commands.
- [x] QEMU markers remain green in `scripts/qemu-test.sh` (verified after every Cut 0–22).
- [x] Security-relevant negative behavior remains fail-closed (Cuts 0–22: reject paths preserved; `keystored_sign_denied`, `policyd_requester_spoof_denied`, `metricsd_security_reject_probe`, `statefs_unauthorized_access`, `logd_hardening_reject_probe` all intact).
- [x] Any discovered logic-error or fake-success-marker path is converted into honest behavior/proof signaling — none discovered in Cuts 0–22; rule remains active for Phase 2/3.
- [ ] Phase 4–6 add new markers only via manifest entries; all existing markers carry over byte-identical (no rename).
- [ ] After Phase 4 closure, `TASK-0024` is unblocked; until then it remains gated.
