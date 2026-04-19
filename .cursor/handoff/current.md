# Current Handoff: TASK-0023B Phase 4 — CLOSED

**Date**: 2026-04-17 (Phase 4 closure session, 10 cuts landed under Cursor-internal plan `task-0023b_phase-4_57f4bce2.plan.md`)
**Status**: `TASK-0023B` Phase 4 **complete**. All 10 cuts (P4-01 → P4-10) landed. RFC-0038 §"Stop conditions / acceptance" Phase 4 checklist ticked (10 boxes). `source/apps/selftest-client/proof-manifest.toml` is now the **single source of truth** for the marker ladder (433 entries), the harness profile catalog (`full / smp / dhcp / dhcp-strict / os2vm / quic-required`), and the runtime selftest profile catalog (`bringup / quick / ota / net / none`). `arch-gate` is **6/6 mechanical rules** (Rule 6 added in P4-10: no `REQUIRE_*` env literal in `test-*` / `ci-*` justfile recipe bodies). The `[marker_emission]` allowlist is empty (Rule 3 is allowlist-free). New host-only crate `nexus-proof-manifest` provides parser + CLI (`list-markers / list-env / list-forbidden / list-phases / verify / verify-uart`) consumed by `scripts/qemu-test.sh` and `tools/os2vm.sh`. `selftest-client/build.rs` generates `markers_generated.rs` from the manifest; 373 emit sites now reference `crate::markers::M_<KEY>` constants. Deny-by-default UART analyzer (`verify-uart`) wired into `qemu-test.sh` post-pass. Phase 4 closure unblocks `TASK-0024`.

**Pre-Phase-4 status (Phase 3 closure, kept for reference)**: All four Phase-3 cuts (P3-01 → P3-04) landed 2026-04-17 under Cursor-internal plan `task_0023b_phase_3_ee96d119.plan.md`. RFC-0038 §"Stop conditions / acceptance" Phase 3 checklist ticked (4 boxes). QEMU `SELFTEST:` ladder is byte-identical (119 markers) across every Phase-3 cut versus the pre-Phase-3 baseline. `main.rs` shrunk 122 → **49 LoC** (dispatch-only); `scripts/check-selftest-arch.sh` + `just arch-gate` are now chained into `just dep-gate` and gating CI mechanically.

**Working tree at handoff**: still has `uart.log` (test artifact — **do not commit**) plus the Phase-3 source/script changes (uncommitted; user owns the commit decision). Same 6 pre-existing modified files from start of session (`.cursor/handoff/current.md`, `.cursor/current_state.md`, `.cursor/next_task_prep.md`, `docs/rfcs/RFC-0038-*.md`, `tasks/TASK-0023B-*.md`, `uart.log`) plus the Phase-3 deliverables.

**Execution SSOT**: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
**Contract SSOT**: `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
**Architectural anchor**: `docs/adr/0027-selftest-client-two-axis-architecture.md`
**Long-running discipline track**: `tasks/TRACK-OS-PROOF-INFRASTRUCTURE.md`

## Phase 3 closure summary

### Cut-by-cut log

| Cut | Scope | Deliverables | Marker parity vs prior cut |
|---|---|---|---|
| P3-01 | Flatten 13 single-file `name/mod.rs` candidates to `name.rs` | `git mv` only (zero content drift). Affected: `os_lite/services/{bootctl,bundlemgrd,execd,keystored,logd,metricsd,policyd,samgrd,statefs}/mod.rs`, `os_lite/{mmio,vfs,timed}/mod.rs`, `os_lite/dsoftbus/quic_os/mod.rs`. 13 folders removed, 13 sibling `name.rs` files added; no parent edits (Rust resolves `mod name;` → `name.rs` and `name/mod.rs` from the same line). `git log --follow` history preserved. | byte-identical |
| P3-02 | Extract host-pfad `run()` from `main.rs` → `host_lite.rs::run()` (then flattened from `host_lite/mod.rs` per the P3-01 single-file rule) | `main.rs` shrunk 122 → **49 LoC** (the original "≤ 35 LoC" target was aspirational — 49 is the rustfmt-canonical floor, given the long cfg expressions are expanded across 5+ lines). Both std and no-std-host `pub(crate) fn run()` bodies live in `host_lite.rs` under their original cfg gates. `main.rs` is now CONTEXT + cfgs + 2 dispatch fns + mod decls — zero logic. | byte-identical |
| P3-03 | `scripts/check-selftest-arch.sh` + `just arch-gate` + allowlist | New script enforces 5 rules (`os_lite/mod.rs` ≤ 80 LoC, no `phases::*` cross-imports, marker strings only in phases/* + `markers.rs`, no `fn` in `mod.rs`, no file ≥ 500 LoC). `source/apps/selftest-client/.arch-allowlist.txt` has 3 sections (`marker_emission`, `mod_rs_fn`, `size_500`); 17 capability files allowlisted as the Phase-2 baseline (Phase 4 manifest work shrinks `marker_emission` to zero per the plan's natural tightening). `just arch-gate` is now a prerequisite of `just dep-gate` so structural drift fails fast. Synthetic-violation tests confirm rules 2/3/4 fire with `file:line`; reverted clean. | byte-identical |
| P3-04 | Standards review + Phase-3 closure | `#[must_use]` survey: `Result`-returning fns are redundant (`core::result::Result` is already `#[must_use]`); only one `fn -> bool` candidate (`smoltcp_probe::tx_send`) lives in cfg-gated bring-up debug code with consumed return — no annotation added. Slot newtype (`Slot(u32)`) deferred to Phase 4 with `TODO(TASK-0023B Phase 4)` note in `context.rs` (~16 call sites across 8 files; non-mechanical). Send/Sync audit landed as a single intent comment in `context.rs` documenting the single-HART/single-task invariant — no marker traits introduced (adding them later without changing the runtime model would mask, not reveal, a real concurrency bug). | byte-identical |

### Final structural state

| File | Pre-Phase-3 | Post-Phase-3 |
|---|---|---|
| `source/apps/selftest-client/src/main.rs` | 122 LoC | **49 LoC** (CONTEXT + cfgs + 2 dispatch fns + 3 mod decls) |
| `source/apps/selftest-client/src/host_lite.rs` | (n/a) | 78 LoC (host slice — std + no-std-host `run()`) |
| `source/apps/selftest-client/src/os_lite/mod.rs` | 31 LoC | 50 LoC (within the 80-LoC arch-gate ceiling; minor expansion from CONTEXT clarification, structurally unchanged) |
| 13 single-file `name/mod.rs` folders | folder + mod.rs | sibling `name.rs` |
| `scripts/check-selftest-arch.sh` | (n/a) | new, 167 LoC, executable, `bash -n` clean |
| `source/apps/selftest-client/.arch-allowlist.txt` | (n/a) | new, 50 LoC, 3 sections, all baseline entries justified by inline comment |
| `justfile` | dep-gate stand-alone | `arch-gate` recipe added; `dep-gate: arch-gate` chain in place |

### Behavioral parity gates (all green at every cut)

- `RUSTFLAGS='--cfg nexus_env="os" -W unexpected_cfgs -W dead_code' cargo check -p selftest-client --no-default-features --features os-lite --target riscv64gc-unknown-none-elf` → clean (only pre-existing `nexus_env` cfg warnings).
- `cargo build -p selftest-client` (host, default features) → clean (verifies host_lite extraction).
- `cargo test -p dsoftbusd -- --nocapture` → all green.
- `just test-dsoftbus-quic` → 6 + 8 tests green.
- `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os` → 119 `SELFTEST:` markers, BYTE-IDENTICAL to the pre-Phase-3 baseline at every cut (`diff /tmp/p3_baseline_selftest.txt /tmp/p3_<n>_post_selftest.txt` empty).
- `just arch-gate` → `[PASS] selftest-client architecture gate (5/5 rules clean)` after P3-03 lands.
- `just lint` → clean.
- Marker strings, marker order, reject behavior, retry/yield budgets, NONCE seeds, and IPC frame layouts all preserved verbatim.
- No new `unwrap`/`expect`. No new dependencies in `selftest-client/Cargo.toml`.

### Operational lessons captured this phase (apply forward)

- Aspirational LoC targets (e.g. "≤ 35 LoC for `main.rs`") collide with rustfmt's canonical formatting of long cfg expressions. The achievable floor is what matters; document the deviation honestly rather than fighting rustfmt.
- "Single-file folder" is a recursive rule: P3-02 created `host_lite/mod.rs`, which immediately triggered the P3-01 flatten rule. Future cuts that create new modules should default to `name.rs`, not `name/mod.rs`.
- Mechanical arch-gate rules collide with real Phase-2 baselines (e.g. capability files emitting `SELFTEST:` markers directly). The fix is allowlist-baseline + planned tightening (Phase 4 shrinks the `marker_emission` allowlist to zero), not retrofit-by-fiat.
- Bash chained `&&`/`;` revert sequences can desync when the failing command is `just <recipe>` (recipe failures print full output even when piped); always run a separate explicit revert + `git status` after a synthetic-violation test rather than relying on the chain.
- `git mv` requires the source to be tracked; an untracked file (e.g. one created by an in-flight cut and not yet `git add`-ed) needs plain `mv`. The `Failed to mount ".git/hooks" read-only` sandbox error keeps surfacing — re-run with `required_permissions: ["all"]`.

## Phase 4 closure summary

### Cut-by-cut log

| Cut | Scope | Deliverables |
|---|---|---|
| P4-01 | Manifest skeleton + parser crate | `docs/testing/proof-manifest.md` schema doc; `proof-manifest.toml` skeleton (`[meta]` + 12 `[phase.X]` declarations); new host-only crate `nexus-proof-manifest` with `Manifest` + `ParseError` + 8 reject tests. |
| P4-02 | RFC-0014 binding | RFC-0014 §appendix declares `proof-manifest.toml` normative; `docs/testing/proof-manifest.md` 1:1 phase-mapping table. No code touched. |
| P4-03 | Populate manifest + generate constants | 179 gating markers added to `proof-manifest.toml`; parser extended with `[marker.…]` schema + `Marker.const_key()` + phase/profile reference validation; `selftest-client/build.rs` generates `markers_generated.rs`. |
| P4-04 | Replace emit sites + tighten arch-gate | 254 additional diagnostic / fragment / FAIL-label markers back-filled (433 total); 373 emit sites across 29 files migrated to `crate::markers::M_<KEY>` (string + byte-string forms); `arch-gate` Rule 3 made allowlist-free; `[marker_emission]` allowlist emptied; synthetic-violation regression confirmed Rule 3 still fires. |
| P4-05 | Host CLI + harness consumes manifest | `nexus-proof-manifest` CLI (`list-markers / list-env / list-forbidden / list-phases / verify`); harness profiles populated (`full / smp / dhcp / os2vm / quic-required`); `qemu-test.sh` sources env via `pm_apply_profile_env` + `pm_mirror_check`. |
| P4-06 | `just` recipes migrated to PROFILE | `test-os PROFILE=…` is canonical; `test-smp / test-os-dhcp / test-dsoftbus-2vm / test-network` soft-deprecated for one cycle (deleted in P4-10); `ci-os-full / ci-os-smp / ci-os-dhcp / ci-os-quic / ci-os-os2vm / ci-network` added. |
| P4-07 | `tools/os2vm.sh` consumes manifest | `--profile=<name>` flag (default `os2vm`); `pm_apply_profile_env` + `pm_mirror_subset_check` against the `os2vm` projection; manifest extended with five `dsoftbus:mux crossvm *` markers gated `emit_when = { profile = "os2vm" }`. |
| P4-08 | Runtime profiles + dispatcher | 5 `runtime_only = true` profiles with explicit `phases = [...]` lists; 12 `dbg: phase X skipped` breadcrumb markers; `os_lite/profile.rs` (`Profile`, `PhaseId`, `from_kernel_cmdline_or_default`, `includes`, `skip_marker`); `run_or_skip!` macro in `os_lite/mod.rs`; `cargo:rerun-if-env-changed=SELFTEST_PROFILE`; `ci-os-runtime-bringup / quick / none` recipes. |
| P4-09 | Deny-by-default analyzer | `nexus-proof-manifest verify-uart --profile=<name> --uart=<path>` (forbidden + unexpected = exit 1); wired into `qemu-test.sh` post-pass (`PM_VERIFY_UART=1` default); 6 integration tests cover clean / unexpected / forbidden / json / missing-file / missing-arg paths. |
| P4-10 | Closure | Deleted four soft-deprecated aliases; added `ci-os-dhcp-strict` recipe + `[profile.dhcp-strict]` (extends `dhcp`); arch-gate Rule 6 ("no `REQUIRE_*` env literal in `test-*` / `ci-*` justfile bodies") + `[justfile_require_env]` allowlist (empty); manifest-driven workflow documented in `docs/testing/index.md`; `.cursor/{handoff,current_state,next_task_prep}` synced; `TASK-0024` `depends-on` updated; `STATUS-BOARD` + `IMPLEMENTATION-ORDER` refreshed; Phase-5 plan to be authored at the start of the Phase-5 session. |

### Behavioral parity gates (verified at every cut)

- QEMU `SELFTEST:` ladder for `PROFILE=full` byte-identical to the pre-Phase-4 baseline (mirror-check via `pm_mirror_check` enforces this on every `qemu-test.sh` run).
- `cargo test -p nexus-proof-manifest` → 40 tests (lib + bin + cli_smoke 10 + cli_verify_uart 6 + parse_markers 5 + parse_skeleton 8 + profiles 6 + runtime_profiles 5).
- `bash scripts/check-selftest-arch.sh` → 6/6 rules clean.
- `RUSTFLAGS='--cfg nexus_env="os" -W unexpected_cfgs -W dead_code' cargo +nightly check -p selftest-client --no-default-features --features os-lite --target riscv64imac-unknown-none-elf` → clean (only pre-existing `nexus_env` cfg warnings).

### Operational lessons captured this phase (apply forward)

- Mass marker-literal migration is feasible with a one-shot Rust tool (`/tmp/migrate_markers.rs`) **provided** byte-string substitutions (`b"foo"` → `M_FOO.as_bytes()`) run before string substitutions (`"foo"` → `M_FOO`); the reverse order corrupts byte literals.
- `println!(CONST)` is a compile error — the macro requires a string literal as the first argument. Migrating host-side emit sites required `println!("{}", CONST)`.
- `markers_generated.rs` must be unconditionally included (not cfg-gated to OS only) because `host_lite.rs` also references the constants. The fix was a top-level `markers_generated.rs` shim that `include!`s the build-script output for both cfg branches.
- Rule 6 parsing must skip comment-only lines from violation matching; otherwise `# `REQUIRE_*` mentions in explanatory comments trigger false positives.
- `[profile.<name>]` `extends` chains must reject cycles at parse time (single-pass DFS); we hit this immediately when re-using `dhcp` as a parent for `dhcp-strict`.

## Phase 5 — what's next

Phase-5 contract is locked in `RFC-0038` and is 6 cuts (signed evidence bundles). The Phase-5 plan file is `/home/jenning/.cursor/plans/task-0023b_phase-5_<hash>.plan.md` (to be authored at the start of the Phase-5 session as a separate plan file).

Phase-5 scope summary:

- New host-only crate `nexus-evidence` with deterministic canonicalization spec + Ed25519 signing.
- `tools/extract-trace.sh` produces `trace.jsonl` from `uart.log` using manifest phase tags.
- `tools/seal-evidence.sh` builds + signs `target/evidence/<utc>-<profile>-<git-sha>.tar.gz`; `EVIDENCE_KEY=ci|bringup` selection.
- Hook seal step into `scripts/qemu-test.sh` post-pass.
- `tools/verify-evidence.sh` validates fail-closed; reject tests for tamper classes (manifest / uart / trace / key swap).
- `docs/testing/evidence-bundle.md` documents bundle layout, key model, verify workflow.
- 6 cuts: P5-01 (crate skeleton + canonicalization) → P5-02 (extract-trace) → P5-03 (seal-evidence) → P5-04 (qemu-test.sh integration) → P5-05 (verify-evidence + key separation) → P5-06 (docs).

## Frozen baseline that must stay green (verified end-of-Phase-3; carries into Phase 4)

- Host:
  - `cargo test -p dsoftbusd -- --nocapture`
  - `just test-dsoftbus-quic`
  - Phase 4+: `cargo test -p nexus-proof-manifest -- --nocapture`
  - Phase 5+: `cargo test -p nexus-evidence -- --nocapture`
- OS (Phase 1–3):
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
- OS (Phase 4+ replaces with):
  - `just test-os PROFILE=full`
  - `just test-os PROFILE=smp` (replaces `just test-smp`)
  - `just test-os PROFILE=quic-required` (replaces `REQUIRE_DSOFTBUS=1 …`)
  - `just test-os PROFILE=os2vm` (replaces `just test-dsoftbus-2vm`)
  - `just test-os PROFILE=bringup` (runtime profile; short-circuits before `routing`)
  - `just test-os PROFILE=none` (runtime profile; only `SELFTEST: end`)
- Hygiene:
  - `just dep-gate` (now chains `arch-gate` first)
  - `just diag-os && just fmt-check && just lint`
  - Phase 5+: `tools/verify-evidence.sh target/evidence/<latest>` returns 0

## Boundaries reaffirmed

- Phase 3 is closed and behavior-preserving: same marker order, same proof meanings, same reject behavior across all 4 cuts. 119 markers byte-identical.
- `arch-gate` is now mechanical, not honor-system. New `mod.rs` `fn` definitions, new marker emissions outside `phases/*`+`markers.rs`, new `phases::*` cross-imports, and new files ≥ 500 LoC will fail `just dep-gate` in CI without an explicit allowlist entry.
- `main.rs` is now 49 LoC dispatch-only. Phase 4 work should not touch it (the dispatch shape is correct; Phase 4 changes belong in the manifest + `os_lite/`).
- Phase 4 may *add* new markers gated by new profiles (e.g. `SELFTEST: smp ipi ok`) but must not rename existing markers.
- Visibility ceiling: `pub(crate)`. No new `unwrap`/`expect`. No new dependencies in `selftest-client/Cargo.toml`; new host-only crates (`nexus-proof-manifest`, `nexus-evidence`) are separate.
- Do not absorb `TASK-0024` transport features. `TASK-0024` unblocks at Phase 4 closure (manifest-driven `emit_when = { profile = "quic-required" }` is the integration point).
- No kernel changes across all 6 phases. `SELFTEST_PROFILE` reading from kernel cmdline is a userspace read, not a kernel API change.
- Defer STATUS-BOARD / IMPLEMENTATION-ORDER updates until Phase 4 closure (per-cut updates create drift); Phase 4 closure also unblocks `TASK-0024` metadata.

## Next handoff target

- **Active plan**: TBD — author `task-0023b_phase-5_<hash>.plan.md` (Cursor-internal) at the start of the Phase-5 session as a separate plan file; scope: 6 cuts P5-01 → P5-06 only.
- **Resume point**: Cut **P5-01** — `nexus-evidence` crate skeleton + canonicalization spec + unit tests for deterministic hashing.
- **Per-cut cadence (Phase 5 carries the Phase-4 floor + adds evidence verify)**:
  1. `RUSTFLAGS='--cfg nexus_env="os" -W unexpected_cfgs -W dead_code' cargo +nightly check -p selftest-client --no-default-features --features os-lite --target riscv64imac-unknown-none-elf`
  2. `cargo test -p dsoftbusd -- --nocapture`
  3. `just test-dsoftbus-quic`
  4. `just test-os PROFILE=full` (verify-uart wired; should be deterministic)
  5. `cargo test -p nexus-proof-manifest -- --nocapture`
  6. `cargo test -p nexus-evidence -- --nocapture` (from P5-01 onward)
  7. `rustfmt +stable <touched .rs files only>`; verify and revert any submodule drift via `git checkout -- <unintended>`
  8. `just dep-gate` (chains `arch-gate` first; both must pass; arch-gate is now 6/6 rules)
  9. `just lint`
  10. From P5-04 onward: `tools/verify-evidence.sh target/evidence/<latest>` returns 0
- **Phase 5 closure tasks (after P5-06)**: tick RFC-0038 Phase 5 checklist (6 boxes); sync `.cursor/{handoff/current.md, next_task_prep.md, current_state.md}`; open `task-0023b_phase-6_<hash>.plan.md` (6 cuts, replay capability); refresh `tasks/STATUS-BOARD.md`, `tasks/IMPLEMENTATION-ORDER.md`.
