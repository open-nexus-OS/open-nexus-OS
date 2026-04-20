---
title: TASK-0023B Selftest-Client production-grade deterministic test architecture refactor + manifest/evidence/replay v1
status: Draft
owner: @runtime
created: 2026-04-16
last-updated: 2026-04-17
depends-on:
  - TASK-0023
follow-up-tasks:
  - TASK-0024
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed: docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md
  - Depends-on (OS QUIC session baseline): tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md
  - Follow-up (transport hardening): tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md
  - Testing harness: scripts/qemu-test.sh
  - 2-VM harness: tools/os2vm.sh
  - Phase contract: docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md
  - Long-running infrastructure track: tasks/TRACK-OS-PROOF-INFRASTRUCTURE.md
---

## Short description

- **Scope**: Refactor `source/apps/selftest-client/src/main.rs` and the surrounding selftest-client architecture into a scalable deterministic-test structure, then promote the result into a manifest-driven, profile-aware, evidence-producing, replayable proof system.
- **Deliver** (phased):
  - Phase 1 (✅ done) — structural extraction; `main.rs` minimal; `os_lite/mod.rs` shrunk from ~6771 → 1226 LOC.
  - Phase 2 (✅ done, 2026-04-17) — two-axis architecture (`os_lite/phases/` orchestration verbs alongside capability nouns); `pub fn run()` collapsed from ~1100 → **14 lines**; `os_lite/mod.rs` 1256 → **31 LoC**; RFC-0014 phase list expanded 8 → 12 (congruent with code phases); QEMU `SELFTEST:` ladder byte-identical (119 markers) across all 18 cuts (P2-00 → P2-17). Anchored by post-closure docs supplement: ADR-0027 (architectural contract), `selftest-client/README.md` (onboarding), CONTEXT headers across all 49 source files (commits `65d299d` + `f52cf60`); both proof gates (`just test-all`, `just test-network`) green at handoff.
  - Phase 3 (✅ done, 2026-04-17) — flattened 13 single-file `name/mod.rs` modules to `name.rs`; extracted host-pfad `run()` into sibling `host_lite.rs::run()` (`main.rs` shrunk 122 → 49 LoC, dispatch-only); landed `scripts/check-selftest-arch.sh` + `just arch-gate` (chained into `just dep-gate`) enforcing 5 mechanical rules with `[marker_emission]`/`[mod_rs_fn]`/`[size_500]` allowlists in `source/apps/selftest-client/.arch-allowlist.txt`; mechanical standards review (`#[must_use]` redundant on `Result` fns since core::result::Result is already `#[must_use]`; Slot newtype deferred to Phase 4 with `TODO(TASK-0023B Phase 4)` note in `context.rs`; Send/Sync intent comment added to `context.rs` documenting single-HART/single-task invariant). 119-marker `SELFTEST:` ladder byte-identical across all four cuts (P3-01 → P3-04).
  - Phase 4 (✅ done, 2026-04-17) — `proof-manifest.toml` is the single source of truth for the marker ladder (433 entries), harness profiles (`full / smp / dhcp / dhcp-strict / os2vm / quic-required`), and runtime selftest profiles (`bringup / quick / ota / net / none`). New host-only crate `nexus-proof-manifest` (parser + CLI: `list-markers / list-env / list-forbidden / list-phases / verify / verify-uart`); `selftest-client/build.rs` generates `markers_generated.rs`; 373 emit sites across 29 files migrated to `crate::markers::M_<KEY>` constants; `[marker_emission]` allowlist now empty. `arch-gate` is 6/6 mechanical rules — Rule 6 forbids `REQUIRE_*` env literals in `test-*` / `ci-*` justfile recipes. `scripts/qemu-test.sh` consumes the manifest (env wiring + mirror-check + `verify-uart` deny-by-default post-pass); `tools/os2vm.sh` consumes the manifest (subset mirror-check). New `os_lite/profile.rs` + `run_or_skip!` macro implement runtime phase skipping with `dbg: phase X skipped` breadcrumbs. `just test-os PROFILE=…` is canonical; `test-smp / test-os-dhcp / test-os-dhcp-strict / test-dsoftbus-2vm / test-network` deleted (replaced by `ci-os-smp / ci-os-dhcp / ci-os-dhcp-strict / ci-os-os2vm / ci-network`). QEMU `SELFTEST:` ladder for `PROFILE=full` byte-identical to the pre-Phase-4 baseline.
  - Phase 5 (✅ done, 2026-04-17) — signed evidence bundles per QEMU run; **7** cuts (P5-00 → P5-06). P5-00 prepended at session start: `proof-manifest.toml` (1433 LoC) split into a `proof-manifest/` directory tree (`manifest.toml` + `phases.toml` + `markers/*.toml` + `profiles/*.toml`) with `[meta] schema_version = "2"` + `[include]` glob expansion (lex-sorted, conflict-checked); v1 single-file back-compat retained. New host-only crate `source/libs/nexus-evidence/` owns canonicalization + Ed25519 sign/verify + secret scan; 102-byte signature wire format (`magic="NXSE" || version=0x01 || label || hash[32] || sig[64]`); `KeyLabel::{Ci, Bringup}` baked into the signature so `verify --policy=ci` rejects bringup-signed bundles. `Bundle::seal` returns `Result<Bundle, EvidenceError>` (callers must handle `EvidenceError::SecretLeak`). Reproducible `tar.gz` packing (`mtime=0`, `uid=0`, `gid=0`, mode `0o644`, lex-sorted entries, fixed gzip OS byte). CI key resolved from env (`NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64`); bringup key from `~/.config/nexus/bringup-key/private.ed25519` with mandatory mode `0600` check. Deny-by-default secret scanner refuses to seal bundles containing PEM private keys, bringup-key paths, `*PRIVATE_KEY*=…` env-style assignments, or ≥64-char base64 high-entropy blobs (with `Bundle::seal_with(&allowlist)` escape hatch for tests). Post-pass evidence pipeline wired into `scripts/qemu-test.sh` (single bundle) and `tools/os2vm.sh` (per-node A/B bundles); CI gate: `CI=1` ⇒ seal mandatory + rejects `NEXUS_EVIDENCE_DISABLE=1`. `tools/{seal,verify,gen-bringup-key,gen-ci-key}-evidence.sh` shipped + `keys/evidence-ci.pub.ed25519` placeholder + `keys/README.md` rotation procedure. 40 tests across 6 integration files in `nexus-evidence` (5 assemble + 6 canonical_hash + 4 key_separation + 5 qemu_seal_gate + 7 scan + 13 sign_verify); `cargo clippy -p nexus-evidence --all-targets -- -D warnings` clean; `just dep-gate` clean (zero new forbidden deps; `ed25519-dalek` was already in OS graph via `userspace/updates`); `nexus-evidence` itself stays host-only. QEMU `SELFTEST:` ladder for `PROFILE=full` byte-identical to pre-Phase-5 baseline.
  - Phase 6 — replay capability (`replay-evidence` + `diff-traces` + `bisect-evidence` + cross-host floor + docs).
- **Out of scope** (this task):
  - New transport features / QUIC recovery / data-plane hardening (owned by `TASK-0024`).
  - Protocol semantic changes.
  - Long-running observability/coverage/process-discipline workstreams (owned by `TRACK-OS-PROOF-INFRASTRUCTURE`).
  - Host-only test reorganization (`test-e2e`, `test-dsoftbus-quic`, `test-host`); these stay outside the proof manifest by design (different mental model: cargo-tested host logic vs. QEMU-attested OS behavior).

## Why this task exists

`source/apps/selftest-client/src/main.rs` was too large for safe iterative development (Phase 1 fixed this).
More importantly, `selftest-client` is one of the central components of this OS because it drives the deterministic QEMU marker ladder and service-proof orchestration. **No other OS we are aware of treats deterministic OS-level proof as a first-class product surface.** That makes this code path release-truth, not test glue.

The expanded scope (Phases 4–6) exists because today our proof infrastructure has three unforced weaknesses:

1. **Two truths for the marker ladder**: marker strings live both in `scripts/qemu-test.sh` (harness expectation) and in `selftest-client` Rust code (emitter). Drift is possible and only caught by a failed CI run.
2. **No portable evidence**: a green QEMU run produces a UART log that lives only on the runner. We cannot prove externally what behavior was attested by which build of which manifest.
3. **No replay**: a failure on machine A cannot be deterministically reproduced on machine B from a stored artifact. Bisects are linear retries instead of trace-diffs.

Before adding transport features in `TASK-0024`, the proof infrastructure must (a) be maintainable and extensible (Phases 1–3), and (b) reach a state where every passing CI run produces a signed, replayable evidence bundle keyed by an explicit profile (Phases 4–6). Without that, the deterministic-proof story is internally usable but not externally provable.

## Goal

After completion:

- `selftest-client` is organized around clear deterministic-test responsibilities rather than one monolithic file.
- `main.rs` is minimal: cfg entry + top-level dispatch/orchestration only.
- The full service-test structure and canonical QEMU marker ladder remain unchanged and green.
- The resulting deterministic test infrastructure is production-grade: maintainable, extensible, deterministic under pressure, and strict about proof integrity.
- Rust discipline review is done and documented where sensible (`newtype`, ownership, `Send`/`Sync`, `#[must_use]`).
- A single `proof-manifest.toml` is the authoritative source for: phase list, marker ladder, profile membership (full/smp/dhcp/os2vm/quic-required/bringup/quick/none/…), and run configuration (env vars, runner script, extends-relations).
- `scripts/qemu-test.sh` and `tools/os2vm.sh` consume the manifest instead of hard-coding `PHASES`/`PHASE_START_MARKER`/`PHASE_END_MARKER` arrays and `REQUIRE_*` flags.
- Each `just test-os PROFILE=…` run produces a signed `evidence-bundle.tar.gz` in `target/evidence/` (manifest, UART log, marker trace, build config, profile, signature).
- `tools/replay-evidence.sh <bundle>` deterministically re-runs the captured profile and `tools/diff-traces.sh` produces a stable diff against the original trace; `tools/bisect-evidence.sh` automates the loop.
- A `SELFTEST_PROFILE=<bringup|quick|net|ota|none|full>` runtime switch (kernel cmdline / env) lets `selftest-client` skip whole phases at runtime — no recompile required for fast local iteration.

## Target quality bar

This task targets **production-grade** quality for the deterministic proof infrastructure carried by `selftest-client`.

Reason:

- `selftest-client` is part of the release-truth path for QEMU/service closure claims.
- If this architecture is brittle, opaque, or hard to evolve safely, the whole deterministic proof story becomes weaker.
- This is therefore not just a cleanup task; it is hardening of a release-critical testing surface.

## Non-Goals

- No new QUIC data-plane/recovery features.
- No marker renaming or semantic drift in the existing deterministic service-test ladder (Phases 1–3). Phase 4 may *add* new markers gated by new profiles (e.g. `SELFTEST: smp ipi ok` under `profile=smp`) but must not rename existing markers.
- No mandatory creation of new unit tests solely for refactor cosmetics.
- No kernel changes (Phases 1–6). Runtime selftest-profile reading from kernel cmdline is a userspace read, not a kernel API change.
- No host-test reorganization. `cargo test --workspace`, `just test-host`, `just test-e2e`, `just test-dsoftbus-quic` remain outside the proof manifest. They prove host-resident logic, not OS-attested behavior; collapsing both into one model would weaken both.
- No long-running observability/coverage/process-discipline workstreams. Those land in `TRACK-OS-PROOF-INFRASTRUCTURE` as candidate tasks.
- No automatic publication of evidence bundles to a remote artifact store (Phase 5 produces and verifies bundles locally; transport/storage is a follow-on).

## Constraints / invariants (hard requirements)

- Behavioral parity: same success/failure semantics as before refactor.
- Marker honesty: no new fake-success markers.
- Deterministic bounded loops and parsing paths remain intact.
- Keep ownership boundaries explicit; avoid large mutable shared state blobs.
- The deterministic test ladder is the product here, not incidental test glue.
- Production-grade maintainability is required: the resulting structure must make future changes safer, not just move code around.
- If refactor work reveals logic bugs, marker dishonesty, or fake-success markers, the task must fix them instead of preserving them.
- When fake-success markers are found, they must be replaced by real behavior markers/proofs tied to actual verified outcomes.

## Deterministic testing role (explicit)

`selftest-client` is not just a test binary.
It is the orchestrator for deterministic OS proof in QEMU and therefore a first-class architecture surface.
This task must preserve that role while improving maintainability and extensibility.

## Canonical proof contract (full ladder authority)

- The authoritative proof contract is the full QEMU ladder enforced by `scripts/qemu-test.sh`, not only the QUIC subset.
- Any refactor phase that keeps a small subset green but regresses the wider ladder is considered a failure.
- QUIC markers remain a critical subset, but this task protects the whole service-proof structure.

Behavior-marker rule:

- A marker counts as an honest behavior/proof marker only when it is emitted after a real verified condition or assertion.
- A marker does not count as honest proof when it only follows:
  - entering a code path,
  - returning from a helper call,
  - reaching an expected branch without validating the end condition,
  - assuming success because no error was observed yet.

## Initial target structure (explicitly adaptive, not rigid)

Initial target structure for this task:

```text
source/apps/selftest-client/src/
  main.rs
  markers.rs
  os_lite/
    mod.rs
    ipc/
      mod.rs
      clients.rs
      routing.rs
      reply.rs
      probes.rs
    services/
      keystored.rs
      samgrd.rs
      bundlemgrd.rs
      policyd.rs
      updated.rs
      execd.rs
      logd.rs
      statefs.rs
      bootctl.rs
      metrics.rs
    net/
      mod.rs
      netstack_rpc.rs
      local_addr.rs
      icmp_ping.rs
    mmio/
      mod.rs
    dsoftbus/
      mod.rs
      quic_os/
        mod.rs
        types.rs
        frame.rs
        udp_ipc.rs
        session_probe.rs
        markers.rs
      remote.rs
    vfs.rs
```

Notes:

- This structure is an initial target model, not a rigid final promise.
- If the refactor reveals better module boundaries, the structure should be adjusted to achieve the best maintainable result.
- Any structural adjustment during the refactor is allowed and desired when it improves:
  - deterministic test clarity,
  - ownership/module boundaries,
  - maintainability/extensibility,
  - reduction of protocol/business logic inside `main.rs`.
- `main.rs` should end as minimal as realistically possible: entrypoint + high-level orchestration only.

Normative end-state for `main.rs`:

- `main.rs` MAY contain:
  - cfg-gated entry wiring,
  - top-level dispatch into host/os-lite runners,
  - high-level phase/lifecycle orchestration.
- `main.rs` MUST NOT remain the home for:
  - service-specific RPC implementations,
  - protocol frame encode/decode logic,
  - retry loops or parser state machines,
  - marker-string business logic for subsystem probes.

Review gate for `main.rs` minimality:

- no new helper in `main.rs` should own subsystem-specific behavior,
- no parser/decoder/encoder should live in `main.rs`,
- no retry counters, reply-matching loops, or deadline machinery should live in `main.rs`,
- no service-specific marker text or proof-state branching should be introduced in `main.rs`.

## Touched paths (allowlist)

Phases 1–3 (selftest-client refactor + arch-gate):

- `source/apps/selftest-client/src/main.rs`
- `source/apps/selftest-client/src/**` (new/refactored modules)
- `docs/testing/index.md` (only if proof command list changes)
- `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (Phase 2 / Cut P2-02-equivalent: phase list 8 → 12; congruent with code phases)
- `scripts/check-selftest-arch.sh` (new, Phase 3)
- `justfile` (Phase 3: add `arch-gate` recipe; chain into `dep-gate`)
- `tasks/STATUS-BOARD.md`
- `tasks/IMPLEMENTATION-ORDER.md`

Phase 4 (manifest + profiles):

- `source/apps/selftest-client/proof-manifest.toml` (new)
- `source/libs/nexus-proof-manifest/**` (new, host-only crate; OS feature-gated out)
- `source/apps/selftest-client/src/os_lite/profile.rs` (new)
- `source/apps/selftest-client/build.rs` (extend or add: generate `markers_generated.rs` from manifest)
- `source/apps/selftest-client/src/markers_generated.rs` (build-time generated; `.gitignore`)
- `scripts/qemu-test.sh` (rewrite to consume manifest)
- `tools/os2vm.sh` (rewrite to consume manifest)
- `justfile` (route all `test-*` through `test-os PROFILE=…`)
- `docs/testing/index.md`
- `docs/testing/proof-manifest.md` (new)

Phase 5 (evidence bundle):

- `source/libs/nexus-evidence/**` (new, host-only crate)
- `tools/extract-trace.sh`, `tools/seal-evidence.sh`, `tools/verify-evidence.sh` (new)
- `scripts/qemu-test.sh` (hook seal step after pass/fail)
- `target/evidence/` (build artifact; `.gitignore`)
- `docs/testing/evidence-bundle.md` (new)

Phase 6 (replay):

- `tools/replay-evidence.sh`, `tools/diff-traces.sh`, `tools/bisect-evidence.sh` (new)
- `scripts/regression-bisect.sh` (new wrapper)
- `docs/testing/replay-and-bisect.md` (new)
- `docs/testing/trace-diff-format.md` (new)

Out-of-allowlist (must not touch in this task):

- `source/kernel/**` (no kernel API changes)
- `source/drivers/**`
- `source/services/**` (services are owned by their own tasks; `selftest-client` only orchestrates)
- `tasks/TASK-0024-*.md` (delay marker added; substantive scope owned by TASK-0024 itself)

## Execution phases (mandatory sequence)

Each phase must end with the phase proof floor before the next phase starts.

### Phase 1 - structural refactor only (no behavior change)

Scope:

- Create and wire the initial target structure (or a justified improved variant discovered during the work).
- Move logic out of `main.rs` without changing runtime behavior, marker ordering, or transport semantics.
- Extract first deterministic-test responsibility seams so `main.rs` immediately shrinks.
- Keep symbols/flows equivalent; this phase is decomposition only.

Preferred extraction order inside Phase 1:

1. DSoftBus local QUIC leaf (`os_lite/dsoftbus/quic_os/`)
2. shared netstack/UDP helper seams (`os_lite/net/`)
3. IPC/routing/client-cache seams (`os_lite/ipc/`)
4. service probe families and remaining peripheral helpers

Reason:

- extraction order should mirror the deterministic proof/harness structure as much as possible so failures stay local and reviewable.

Operational Phase-1 sequence:

1. create destination module skeletons and wire `mod.rs`/imports only,
2. move one responsibility slice at a time without semantic edits,
3. rerun the phase proof floor after each major extraction cut, not only at phase end,
4. stop and fix parity immediately if a moved slice changes marker behavior, ordering, or reject behavior.

Phase-1 proof floor:

- `cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd -- --nocapture`
- `cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic`
- `cd /home/jenning/open-nexus-OS && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`

### Phase 2 - maintainability/extensibility optimization (✅ closed 2026-04-17)

Scope (all delivered):

- Introduced two-axis structure: capability nouns (existing `services/`, `ipc/`, `probes/`, `dsoftbus/`, `net/`, `mmio/`, `vfs/`, `timed/`, `updated/`) + new orchestration verbs under `os_lite/phases/{bringup, routing, ota, policy, exec, logd, ipc_kernel, mmio, vfs, net, remote, end}.rs`.
- Collapsed `pub fn run()` to **14 lines** (`PhaseCtx::bootstrap()?` + 12 phase calls); `os_lite/mod.rs` 1256 → **31 LoC**.
- Sub-split high-density modules (`updated/` 451 → 30 LoC across 7 files; `probes/ipc_kernel/` 393 → 28 LoC across 4 files).
- Consolidated the 3× duplicated `ReplyInboxV1` impls into `ipc/reply_inbox.rs` (single source of truth).
- Reduced `services/mod.rs` to aggregator-only (51 → 23 LoC) by moving `core_service_probe*` to `probes/core_service.rs`.
- **Extended RFC-0014 phase list 8 → 12** (`bringup → ipc_kernel → mmio → routing → ota → policy → exec → logd → vfs → net → remote → end`); harness phases and code phases are now congruent (precondition for Phase 4 manifest).
- Behavior unchanged: marker order, marker strings, reject behavior, retry/yield budgets, NONCE seeds, IPC frame layouts all preserved verbatim across all 18 cuts (`diff` of `grep -E '^SELFTEST: '` empty between every adjacent cut and against P2-00 baseline; 119 markers total).
- Plan deviation: cuts executed in actual `pub fn run()` order (P2-02 → P2-05 → P2-06 → P2-07 → P2-08 → P2-09 → P2-03 → P2-04 → P2-10 → P2-11 → P2-12 → P2-13 → P2-14 → P2-15 → P2-16 → P2-17) rather than the plan's assumed numerical order. Closure executed under Cursor-internal plan `task-0023b_phase_2_plan_5e547ada.plan.md`.

Post-closure docs supplement (commits `65d299d` + `f52cf60`, 2026-04-17, no code-behavior change):

- `docs/adr/0027-selftest-client-two-axis-architecture.md` — architectural contract anchoring the two-axis decision (rejected alternatives, consequences, invariants).
- `source/apps/selftest-client/README.md` — onboarding guide (std vs. os-lite flavors, folder map, marker-ladder contract, decision tree for new proofs, determinism rules, common pitfalls).
- All 49 Rust source files in `source/apps/selftest-client/src/` updated to docs-standard CONTEXT headers (2026 copyright, SPDX, OWNERS/STATUS/API_STABILITY/TEST_COVERAGE, ADR-0027 reference). 17 pre-existing headers repointed from ADR-0017 to ADR-0027.
- Style commit `f52cf60` cleared pre-existing rustfmt drift in 6 files (`phases/{bringup,routing}.rs`, `probes/ipc_kernel/{plumbing,security,soak}.rs`, `updated/stage.rs`); pure formatting.
- Verification at handoff: `just test-all` exit 0 (440 s, 119 markers, QEMU clean shutdown), `just test-network` exit 0 (185 s, all 2-VM phases `status=ok`, `result=success`). Logs in `.cursor/test-all.output.log` and `.cursor/test-network.output.log`.

Cut sequence: see `.cursor/next_task_prep.md` ("Phase-2 plan (18 cuts) — CLOSED" table).

Phase-2 proof floor (carried into Phase 3):

- Same commands as Phase 1 (must stay green per cut).

### Phase 3 - closure review and standards check

Scope:

- Verify docs/status surfaces reflect the final refactor state.
- Review and apply Rust standards where sensible:
  - `newtype` wrappers for safety-relevant IDs/state selectors,
  - explicit ownership transfer boundaries,
  - `Send`/`Sync` assumptions reviewed (no unsafe shortcuts),
  - `#[must_use]` on decision-bearing results where useful.
- Verify that the final architecture leaves `main.rs` minimal and prevents re-monolithization.
- Verify that the final architecture is production-grade in practice:
  - responsibilities are legible,
  - critical deterministic proof paths are easy to audit,
  - follow-up feature work can land without re-centralizing the crate.

Mandatory anti-re-monolithization review:

- new logic added during the refactor must land in seam modules, not flow back into `main.rs`,
- newly discovered proof bugs or fake-success markers must be corrected into honest behavior markers/proofs,
- the resulting structure must be easier to extend for future tasks without collapsing orchestration and implementation back together.

Phase-3 proof floor:

- Same commands as Phase 1 (must stay green).
- `cd /home/jenning/open-nexus-OS && just dep-gate && just diag-os && just arch-gate`

### Phase 4 — Marker-Manifest as Single Source of Truth + profile-aware harness

Scope (Apple-grade evidence foundation, A1):

- Introduce `source/apps/selftest-client/proof-manifest.toml` as the **single source of truth** for: phase list, marker ladder, profile membership, run configuration.
- Generate Rust constants for marker emission and the harness expectation list from the manifest at build time (no two truths).
- Migrate `scripts/qemu-test.sh` and `tools/os2vm.sh` from hard-coded `PHASES`/`PHASE_START_MARKER`/`PHASE_END_MARKER`/`REQUIRE_*` arrays to manifest-driven expectations.
- Add `SELFTEST_PROFILE=<full|bringup|quick|ota|net|none>` runtime switch (kernel cmdline / env) — `selftest-client` reads the profile and dynamically enables/disables whole phases. Default = `full`.
- Add `PROFILE=<name>` argument to `just test-os`; the harness uses it to select expected markers from the manifest.

Manifest schema (normative):

```toml
[meta]
schema_version = "1"

[phase.bringup]
order = 1
markers = ["init: ready", "execd: ready"]

[profile.full]
runner = "scripts/qemu-test.sh"
env = {}

[profile.smp]
extends = "full"
runner = "scripts/qemu-test.sh"
env = { SMP = "2", REQUIRE_SMP = "1" }

[profile.dhcp]
extends = "full"
env = { REQUIRE_QEMU_DHCP = "1", REQUIRE_QEMU_DHCP_STRICT = "1" }

[profile.os2vm]
runner = "tools/os2vm.sh"
env = { REQUIRE_DSOFTBUS = "1" }

[profile.quic-required]
extends = "full"
env = { REQUIRE_DSOFTBUS = "1" }

[profile.bringup]   # runtime sub-profile
runtime_only = true
phases = ["bringup", "ipc_kernel", "end"]

[marker."SELFTEST: smp ipi ok"]
phase = "bringup"
emit_when = { profile = "smp", smp_min = 2 }
proves = "SMP IPI delivery between hart 0 and hart 1"
introduced_in = "TASK-0012"

[marker."dsoftbus: quic os disabled (fallback tcp)"]
phase = "net"
emit_when_not = { profile = "quic-required" }
forbidden_when = { profile = "quic-required" }
```

Cuts (10):

- **P4-01**: write the manifest schema doc + `proof-manifest.toml` skeleton (meta + phase declarations only); add `nexus-proof-manifest` host-only crate to parse it; unit tests for parser + reject paths.
- **P4-02**: extend RFC-0014 phase list from 8 → 12 (`bringup → ipc_kernel → mmio → routing → ota → policy → exec → logd → vfs → net → remote → end`) so harness phases and code phases are congruent. (Done via cross-reference; RFC-0014 update is the artifact.)
- **P4-03**: populate `proof-manifest.toml` with all current markers from `scripts/qemu-test.sh` + `selftest-client` source (1:1, no behavior change). Build script generates `markers_generated.rs` (Rust constants).
- **P4-04**: replace marker emission in `phases/*` with the generated constants; `arch-gate` enforces no marker string literals outside the generated file + `markers.rs`.
- **P4-05**: add `[profile.*]` definitions for `full`, `smp`, `dhcp`, `os2vm`, `quic-required`. `scripts/qemu-test.sh` reads the manifest via a small `nexus-proof-manifest` host CLI to compute expected markers + env.
- **P4-06**: migrate `just test-os`, `just test-smp`, `just test-os-dhcp`, `just test-dsoftbus-2vm`, `just test-network` to call `just test-os PROFILE=…`. Old recipes become aliases for ≥ 1 cycle, then the alias is removed.
- **P4-07**: add `tools/os2vm.sh` manifest support (`profile.os2vm`).
- **P4-08**: add `[profile.bringup|quick|ota|net|none]` (runtime-only); add `os_lite/profile.rs` + `Profile::from_kernel_cmdline_or_default(Profile::Full)`; modify `pub fn run()` to iterate `profile.enabled_phases()`; add per-profile QEMU smoke tests.
- **P4-09**: deny-by-default check: any marker emitted at runtime that is not declared in the manifest for the active profile is a hard failure (host-side analyzer). Conversely, any manifest-declared marker not seen is a hard failure (existing behavior).
- **P4-10**: hard-deprecate `RUN_PHASE`/`REQUIRE_*` direct env usage in CI; CI must invoke `just test-os PROFILE=<name>`. Document the migration in `docs/testing/index.md`.

Phase-4 proof floor:

- All Phase-1 proofs.
- `cd /home/jenning/open-nexus-OS && cargo test -p nexus-proof-manifest -- --nocapture` (host parser + reject tests).
- `cd /home/jenning/open-nexus-OS && just test-os PROFILE=full` green.
- `cd /home/jenning/open-nexus-OS && just test-os PROFILE=smp` green.
- `cd /home/jenning/open-nexus-OS && just test-os PROFILE=quic-required` green.
- `cd /home/jenning/open-nexus-OS && just test-os PROFILE=bringup` green and short-circuits before `routing` phase.
- `cd /home/jenning/open-nexus-OS && just test-os PROFILE=none` exits cleanly with `SELFTEST: end` and no probe markers.
- `cd /home/jenning/open-nexus-OS && just dep-gate && just diag-os && just arch-gate`.

Phase-4 hard gates (mechanically enforced):

- No marker string literal outside `markers_generated.rs` + `markers.rs` (arch-gate).
- No `REQUIRE_*` env var read directly in `just test-*` recipes (allowlist: only inside the manifest CLI).
- Manifest parser rejects unknown keys (forward-compat checked by reject tests).
- A profile with no declared markers is rejected at build time.

### Phase 5 — Signed evidence bundle per QEMU run (✅ done, 2026-04-17)

Scope (Apple-grade evidence foundation, A2):

- Each `just test-os PROFILE=…` run writes `target/evidence/<utc>-<profile>-<git-sha>.tar.gz` containing:
  - `manifest.tar` (deterministic tar of the `proof-manifest/` v2 directory tree used for the run),
  - `uart.log` (unfiltered serial output),
  - `trace.jsonl` (extracted marker ladder with timestamps + phase tags; substring-against-all-manifest-literals; deny-by-default for orphan `SELFTEST:` / `dsoftbusd:` lines),
  - `config.json` (profile name, env vars, kernel cmdline, QEMU args, host info, build SHA, rustc version, qemu version),
  - `signature.bin` (102-byte: `magic="NXSE" || version=0x01 || label || hash[32] || sig[64]` Ed25519 signature over the canonical hash; CI label or bringup label baked into the signature byte so `verify --policy=ci` rejects bringup-signed bundles).
- Verification tool `tools/verify-evidence.sh <bundle> [--policy=ci|bringup]` re-derives the canonical hash and validates the signature; fails closed across 5 tamper classes.
- Host-only crate `source/libs/nexus-evidence/` owns canonicalization (`H(meta) || H(manifest_bytes) || H(uart_normalized) || H(sorted(trace)) || H(sorted(config))`), Ed25519 sign/verify, secret scanner, and reproducible `tar.gz` packing (`mtime=0`, `uid=0`, `gid=0`, mode `0o644`, lex-sorted entries, fixed gzip OS byte). Uses `ed25519-dalek` (already in OS graph via `userspace/updates`); `nexus-evidence` itself stays host-only.

Cuts (7; P5-00 prepended at session start):

- **P5-00 (✅ done)**: `proof-manifest.toml` (1433 LoC) split into a `source/apps/selftest-client/proof-manifest/` directory tree (`manifest.toml` + `phases.toml` + `markers/*.toml` + `profiles/*.toml`); `[meta] schema_version = "2"`; `nexus-proof-manifest` parser extended with `[include]` glob expansion (lex-sorted, conflict-checked); v1 single-file back-compat retained. `scripts/qemu-test.sh`, `tools/os2vm.sh`, `selftest-client/build.rs`, and the CLI repointed to `proof-manifest/manifest.toml`. `PROFILE=full` ladder byte-identical.
- **P5-01 (✅ done)**: `nexus-evidence` skeleton + `Bundle` + per-artifact subtypes + `canonical_hash` + 6 integration tests in `tests/canonical_hash.rs`. Spec authored in `docs/testing/evidence-bundle.md`.
- **P5-02 (✅ done)**: `Bundle::assemble` + `extract_trace` (substring-against-all-manifest-literals; `[ts=…ms]` timestamp prefix; deny-by-default for orphan markers) + `gather_config` + reproducible `tar.gz` packing in `bundle_io.rs`. `nexus-evidence` CLI ships `assemble / inspect / canonical-hash`. 5 integration tests in `tests/assemble.rs`.
- **P5-03 (✅ done)**: Ed25519 sign/verify with `KeyLabel::{Ci, Bringup}` baked into the signature byte; CLI extended with `seal / verify / keygen`; `tools/seal-evidence.sh` + `tools/verify-evidence.sh` shell wrappers; placeholder `keys/evidence-ci.pub.ed25519` checked in. 13 integration tests in `tests/sign_verify.rs` covering 5 tamper classes (manifest / uart / trace / config / key-label swap).
- **P5-04 (✅ done)**: Key separation via `nexus_evidence::key::from_env_or_dir` (CI: `NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64`; bringup: `~/.config/nexus/bringup-key/private.ed25519` with mandatory `0600` perm check). Deny-by-default secret scanner in `src/scan.rs` (PEM blocks, `bringup-key/private` paths, `*PRIVATE_KEY*=…` env-style assignments, ≥64-char base64 high-entropy blobs) wired into `Bundle::seal` (now returns `Result<Bundle, EvidenceError>`); `Bundle::seal_with(&allowlist)` escape hatch for tests. `.gitignore` rejects `**/private.ed25519`. `tools/{gen-bringup-key.sh, gen-ci-key.sh}` + `keys/README.md` rotation procedure. 11 integration tests (7 scan + 4 key_separation).
- **P5-05 (✅ done)**: Post-pass evidence pipeline wired into `scripts/qemu-test.sh` (single bundle) and `tools/os2vm.sh` (per-node A/B bundles `…-a.tar.gz` / `…-b.tar.gz`). Env knobs: `NEXUS_EVIDENCE_SEAL=1`, `CI=1` (implies seal + rejects `NEXUS_EVIDENCE_DISABLE=1`), `NEXUS_EVIDENCE_DISABLE=1`. Label resolution: CI key when env set, bringup otherwise. Failure to assemble or seal is fatal. 5 integration tests in `tests/qemu_seal_gate.rs`.
- **P5-06 (✅ done)**: `docs/testing/evidence-bundle.md` final pass (§3a Assembly, §3b Signing & verification, §3c Key separation, §3d Secret scanner, §5 Operational gates with the env-knob matrix and CI hard gates). RFC-0038 §"Stop conditions / acceptance" Phase 5 ticked (7 boxes). `.cursor/{handoff,current_state,next_task_prep}` + `tasks/{STATUS-BOARD,IMPLEMENTATION-ORDER}` synced.

Phase-5 proof floor (all green at closure):

- All Phase-4 proofs.
- `cargo test -p nexus-evidence -- --nocapture` → 40 tests across 6 integration files (5 assemble + 6 canonical_hash + 4 key_separation + 5 qemu_seal_gate + 7 scan + 13 sign_verify); 0 failures.
- `cargo clippy -p nexus-evidence --all-targets -- -D warnings` → clean.
- `just test-os PROFILE=full` writes a verifiable bundle; `tools/verify-evidence.sh target/evidence/<latest>` returns 0 (or `--policy=ci` in CI).
- Tamper tests across 5 classes (manifest / uart / trace / config / key-label swap) cause verify to fail with stable, classified errors (`EvidenceError::SignatureMismatch` or `KeyLabelMismatch`).
- `just dep-gate` clean (zero new forbidden deps; `ed25519-dalek` was already in OS graph via `userspace/updates`); `nexus-evidence` itself stays host-only.

Phase-5 hard gates (mechanically enforced at closure):

- A successful run that fails to seal an evidence bundle is itself a CI failure when `CI=1` (or `NEXUS_EVIDENCE_SEAL=1`); `NEXUS_EVIDENCE_DISABLE=1` is rejected when seal is mandatory.
- Bringup-signed bundles do not validate against `--policy=ci` (label byte in signature wire format).
- Secret scanner is deny-by-default and runs *before* signing in `Bundle::seal`; bundles containing PEM private keys, bringup-key paths, `*PRIVATE_KEY*=…` env-style assignments, or ≥64-char base64 high-entropy blobs refuse to seal.
- Reproducible `tar.gz` (byte-identical for same inputs): `mtime=0` + `uid=0` + `gid=0` + mode `0o644` + lex-sorted entries + fixed gzip OS byte.
- Bringup key file with mode ≠ `0600` rejected (`EvidenceError::KeyMaterialPermissions`).
- `PROFILE=full` marker ladder byte-identical to pre-Phase-5 baseline (`pm_mirror_check` enforces on every run).

### Phase 6 — Replay capability

Scope (Apple-grade evidence foundation, A3):

- `tools/replay-evidence.sh <bundle>`: re-builds from the bundle's recorded git-SHA, replays under the recorded profile + env + QEMU args, captures a fresh trace.
- `tools/diff-traces.sh <original-trace> <replay-trace>`: produces a deterministic diff (phase-by-phase, order-aware). Empty diff = exact replay; bounded diff = drift report; structural diff = regression candidate.
- `tools/bisect-evidence.sh <good-bundle> <bad-bundle>`: walks the git-SHA range between the two bundles, runs a replay per commit, classifies first regressing commit using the diff tool. Bounded by max-commits + wallclock budget; never unbounded.

Cuts (6):

- **P6-01**: `tools/replay-evidence.sh` skeleton — extract bundle, validate signature (P5-05), pin git-SHA, set env, invoke `just test-os PROFILE=<recorded>`.
- **P6-02**: trace diff format spec (`docs/testing/trace-diff-format.md`) + `tools/diff-traces.sh` implementation; unit fixtures for "exact match", "extra marker", "missing marker", "reorder", "phase mismatch".
- **P6-03**: `tools/bisect-evidence.sh` with mandatory `--max-commits` and `--max-seconds` budgets; fail-closed on budget exhaust.
- **P6-04**: integrate bisect into `scripts/regression-bisect.sh` wrapper for the typical CI failure flow ("CI failed at SHA X, last green at SHA Y, replay-bisect → first bad SHA").
- **P6-05**: cross-host determinism floor: replay must reach the same trace on at least 2 host configurations (the CI runner + 1 dev box) for the same bundle. CI runner records trace once, dev re-runs once, diff must be empty modulo a documented allowlist (e.g. wall-clock, qemu version banner).
- **P6-06**: `docs/testing/replay-and-bisect.md` documents the workflow + known non-deterministic surfaces + the documented allowlist + how to extend it.

Phase-6 proof floor:

- All Phase-5 proofs.
- `cd /home/jenning/open-nexus-OS && tools/replay-evidence.sh target/evidence/<good-bundle>` produces an empty diff against the recorded trace.
- Synthetic bad-bundle test: a manually corrupted bundle replay produces a non-empty, classified diff and exits non-zero.
- Bisect smoke: a 3-commit synthetic range (good → drift → regress) is correctly bisected to the regressing commit.

Phase-6 hard gates:

- No replay step may run unbounded (`--max-seconds` mandatory; default cap = 300s per replay).
- A replay that requires a kernel cmdline change beyond what is recorded in the bundle is a hard failure (no environmental drift hidden under "replay").
- Cross-host determinism allowlist is reviewable and append-only.

## Sequencing with TASK-0024

- `TASK-0024` (DSoftBus QUIC recovery / UDP-sec) currently lists `TASK-0023B` as `depends-on`. With the expanded scope, `TASK-0024` is now blocked until **Phase 4 closure** of `TASK-0023B`.
- Reason: `TASK-0024` introduces new markers (recovery probes) that must land directly into the profile-aware manifest with `emit_when = { profile = "quic-required" }`. Adding them before Phase 4 would create another two-truth surface that Phase 4 has to reverse-engineer.
- After Phase 4 closes, `TASK-0024` may proceed in parallel with Phases 5/6.
- `TASK-0024`'s implementation pattern under the new architecture: `dsoftbus/recovery_probe.rs` (capability) + 1 line in `phases/net.rs` (orchestration) + N marker entries in `proof-manifest.toml` (contract).

## Security considerations

### Threat model

- Refactor drift may accidentally bypass reject checks or marker gating.
- Parser/helper extraction may introduce subtle truncation/length bugs.

### Security invariants (MUST hold)

- Reject behavior for malformed/oversized frames remains fail-closed.
- Existing deterministic service-test marker contract remains unchanged.
- No silent fallback marker reintroduction in QUIC-required profile.
- Any discovered fake-success or logic-error marker path must be converted to honest behavior proof before closure.

### DON'T DO

- DON'T change protocol semantics under "refactor" label.
- DON'T ship refactor without parity proofs.
- DON'T hide new behavior behind renamed markers.
- DON'T optimize local structure while damaging the global deterministic test architecture.
- DON'T preserve dishonest markers just because they pre-date the refactor.

## Security proof

### Required tests / commands

- This is primarily a refactor task for selftest code; adding many new standalone tests is optional.
- Mandatory closure proof is parity/regression evidence after each phase.

- `cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd -- --nocapture`
- `cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic`
- `cd /home/jenning/open-nexus-OS && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`

### Required QEMU markers (unchanged from TASK-0023)

- This task preserves the whole service-test ladder; the QUIC-required markers below are only a critical subset.
- The complete expected ladder in `scripts/qemu-test.sh` remains authoritative.
- `dsoftbusd: transport selected quic`
- `dsoftbusd: auth ok`
- `dsoftbusd: os session ok`
- `SELFTEST: quic session ok`

### Forbidden markers in QUIC-required profile

- `dsoftbusd: transport selected tcp`
- `dsoftbus: quic os disabled (fallback tcp)`
- `SELFTEST: quic fallback ok`

## Stop conditions (Definition of Done)

1. Phases 1–6 completed in order, with green proof floor after each phase.
2. The initial target structure exists or has been intentionally improved during the refactor with better module boundaries.
3. `main.rs` is minimal and no longer acts as monolithic storage for service/protocol logic.
4. No behavior regressions in host/service and QEMU proof floors at any phase boundary.
5. The broader deterministic service-test structure remains green and unchanged, including the `TASK-0023` QUIC marker subset.
6. Rust standards closure review is complete and reflected in touched code/docs where sensible.
7. The resulting architecture meets a production-grade bar for deterministic proof infrastructure rather than a one-off refactor-only bar.
8. Any discovered logic bugs or fake-success markers have been converted into honest behavior/proof markers rather than preserved.
9. `TASK-0024` blocked on this task until **Phase 4 closure**, then unblocked.
10. **Phase 4 closure**: `proof-manifest.toml` is the single source of truth; `scripts/qemu-test.sh` and `tools/os2vm.sh` consume it; all `just test-*` recipes route through `just test-os PROFILE=…`; `SELFTEST_PROFILE` runtime switch works for `bringup|quick|ota|net|none|full`; arch-gate enforces no marker string literals outside the generated file + `markers.rs`.
11. **Phase 5 closure** (✅ done, 2026-04-17): every successful `just test-os PROFILE=…` run produces a `target/evidence/<utc>-<profile>-<git-sha>.tar.gz` (manifest tar + uart.log + trace.jsonl + config.json + signature.bin when seal is required); `tools/verify-evidence.sh target/evidence/<latest>` validates signature across 5 tamper classes; bringup-signed bundles do not validate under `--policy=ci`; deny-by-default secret scanner refuses to seal bundles with leaked key material; CI gate (`CI=1`) makes seal mandatory and rejects `NEXUS_EVIDENCE_DISABLE=1`. P5-00 prepended at session start: `proof-manifest.toml` split into a `proof-manifest/` directory tree (`manifest.toml` + `phases.toml` + `markers/*.toml` + `profiles/*.toml`) with `[meta] schema_version = "2"` + `[include]` glob expansion. 40 tests across 6 integration files in `nexus-evidence` clean; `just dep-gate` clean; `PROFILE=full` ladder byte-identical to pre-Phase-5 baseline.
12. **Phase 6 closure**: `tools/replay-evidence.sh` produces an empty diff for a known-good bundle on at least 2 host configurations; `tools/bisect-evidence.sh` correctly identifies a synthetic regression in a bounded run; `docs/testing/replay-and-bisect.md` documents the workflow and the determinism allowlist.

## Plan (small PRs)

1. **Phase 1 PR** (✅ done): create scalable `os_lite` structure and shrink `main.rs` without behavior changes.
2. **Phase 2 PR**: introduce `os_lite/phases/`, collapse `pub fn run()` to ~13 lines, sub-split high-density modules, expand RFC-0014 phase list 8 → 12. (~17 cuts.)
3. **Phase 3 PR**: `host_lite/`, single-file flatten, mechanical `arch-gate`, standards review. (~4 cuts.)
4. **Phase 4 PR(s)**: `proof-manifest.toml` + manifest-driven harness + runtime profile switch. (~10 cuts; may split into 4a parser/schema, 4b harness migration, 4c runtime profiles.)
5. **Phase 5 PR(s)**: `nexus-evidence` crate + sealing/verification toolchain. (~6 cuts.)
6. **Phase 6 PR(s)**: replay + diff + bisect tooling. (~6 cuts.)

Total: ~43 cuts after Phase 1. Each cut keeps the proof floor green; each phase has additional hard gates as listed above.

## SSOT rule

- This task is the execution single source of truth for:
  - phase completion,
  - proof commands,
  - stop conditions,
  - queue/dependency updates,
  - Phase 4 marker manifest content authority (the manifest itself is the technical SSOT for marker strings + profile membership; this task authorizes that mapping).
- `RFC-0038` defines architecture intent and constraints; it must not become the execution tracker.
- `TRACK-OS-PROOF-INFRASTRUCTURE` defines long-running discipline workstreams (B/C/D) that *consume* the manifest + evidence + replay infrastructure delivered here, but does not modify them.
