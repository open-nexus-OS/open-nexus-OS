# Current Handoff: TASK-0023B Phase 3 — CLOSED

**Date**: 2026-04-17 (Phase 3 closure session, 4 cuts landed under Cursor-internal plan `task_0023b_phase_3_ee96d119.plan.md`)
**Status**: `TASK-0023B` Phase 3 **complete**. All four cuts (P3-01 → P3-04) landed. RFC-0038 §"Stop conditions / acceptance" Phase 3 checklist ticked (4 boxes). QEMU `SELFTEST:` ladder is byte-identical (119 markers) across every cut versus the pre-Phase-3 baseline. `main.rs` shrunk 122 → **49 LoC** (dispatch-only); `scripts/check-selftest-arch.sh` + `just arch-gate` are now chained into `just dep-gate` and gating CI mechanically.

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

## Phase 4 — what's next

Phase-4 contract is locked in `RFC-0038` and is 10 cuts (manifest-driven). The Phase-4 plan file is `/home/jenning/.cursor/plans/task-0023b_phase-4_<hash>.plan.md` (to be authored at the start of the Phase-4 session). Phase 4 closure unblocks `TASK-0024`.

Phase-4 scope summary:

- `source/apps/selftest-client/proof-manifest.toml` becomes the SSOT for phase list, marker ladder, profile membership, and run configuration.
- `build.rs` generates `markers_generated.rs` from the manifest. `arch-gate` rule 3 tightens to "only in `markers_generated.rs` + `markers.rs`" (the `[marker_emission]` allowlist shrinks to zero).
- `scripts/qemu-test.sh` and `tools/os2vm.sh` consume the manifest via a host CLI (`nexus-proof-manifest list-markers --profile=…`).
- Harness profiles (`full`, `smp`, `dhcp`, `os2vm`, `quic-required`) and runtime profiles (`bringup`, `quick`, `ota`, `net`, `none`) defined in manifest. `SELFTEST_PROFILE=…` env / kernel cmdline drives runtime phase skipping.
- 10 cuts: P4-01 (schema + skeleton parser crate) → P4-02 (RFC-0014 binding doc) → P4-03 (populate manifest + generate constants) → P4-04 (replace marker emission, tighten arch-gate) → P4-05 (qemu-test.sh consumes manifest) → P4-06 (migrate `just test-*` → `PROFILE=`) → P4-07 (os2vm.sh consumes manifest) → P4-08 (runtime profiles + `os_lite/profile.rs`) → P4-09 (deny-by-default analyzer) → P4-10 (deprecate direct env-var usage).

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

- **Active plan**: TBD — author `task-0023b_phase-4_<hash>.plan.md` (Cursor-internal) at the start of the Phase-4 session; scope: 10 cuts P4-01 → P4-10 only.
- **Resume point**: Cut **P4-01** — write manifest schema doc (`docs/testing/proof-manifest.md`) + `proof-manifest.toml` skeleton (meta + 12 phase declarations) + new host-only crate `nexus-proof-manifest` with parser + reject tests.
- **Per-cut cadence (extends Phase 3 with manifest-host tests)**:
  1. `RUSTFLAGS='--cfg nexus_env="os" -W unexpected_cfgs -W dead_code' cargo check -p selftest-client --no-default-features --features os-lite --target riscv64gc-unknown-none-elf`
  2. `cargo test -p dsoftbusd -- --nocapture`
  3. `just test-dsoftbus-quic`
  4. `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os` (or `PROFILE=…` once P4-06 lands)
  5. `cargo test -p nexus-proof-manifest -- --nocapture` (from P4-01 onward)
  6. `rustfmt +stable <touched .rs files only>`; verify and revert any submodule drift via `git checkout -- <unintended>`
  7. `just dep-gate` (chains `arch-gate` first; both must pass)
  8. `just lint`
- **Phase 4 closure tasks (after P4-10)**: tick RFC-0038 Phase 4 checklist (10 boxes); sync `.cursor/{handoff/current.md, next_task_prep.md, current_state.md}`; open `task-0023b_phase-5_<hash>.plan.md` (6 cuts, signed-evidence); unblock `TASK-0024` (`depends-on` update); refresh `tasks/STATUS-BOARD.md`, `tasks/IMPLEMENTATION-ORDER.md`, `docs/testing/index.md`.
