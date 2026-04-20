# Current Handoff: TASK-0023B Phase 6 — Functionally Closed

**Date**: 2026-04-20 (Phase 6 functionally closed; only external CI-runner replay artifact remains as an environmental step)
**Status**: `TASK-0023B` Phase 6 is **functionally closed**. Delivered: P6-01 (`tools/replay-evidence.sh`), P6-02 (`tools/diff-traces.sh` + format spec + fixtures), P6-03 (`tools/bisect-evidence.sh` bounded + binary-search core, with `good|drift|bad` synthetic mode), P6-04 (`scripts/regression-bisect.sh`), P6-06 (`docs/testing/replay-and-bisect.md`), and P6-05 mechanically done — exact-match replays on native + containerized CI-like host (`.cursor/replay-dev-a.json`, `.cursor/replay-ci-like.json`), synthetic bad-bundle classified diff (`.cursor/replay-synthetic-bad.json`, exit 1, `missing_marker` for the injected probe), and 3-commit good→drift→regress bisect smoke (`.cursor/bisect-good-drift-regress.json`, `first_bad_commit: c2cccccc`, `drift_commits: [c1bbbbbb]`). All Phase-6 hard gates verified locally (`--max-seconds`/`--max-commits` mandatory exits; `PROFILE` env override rejected by replay). Only the external CI-runner replay run against the same sealed bundle remains; recipe in `docs/testing/replay-and-bisect.md` §7-§8.

**Working tree at handoff**: still has `uart.log` (test artifact — **do not commit**) plus the Phase-5 source/script changes (uncommitted; user owns the commit decision). New: `keys/evidence-ci.pub.ed25519` + `keys/README.md` (placeholder CI public key, rotation procedure documented), the `nexus-evidence` crate at `source/libs/nexus-evidence/`, `tools/{gen-bringup-key.sh, gen-ci-key.sh, seal-evidence.sh, verify-evidence.sh}`, and the proof-manifest split layout under `source/apps/selftest-client/proof-manifest/`.

**Execution SSOT**: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
**Contract SSOT**: `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
**Architectural anchor**: `docs/adr/0027-selftest-client-two-axis-architecture.md`
**Long-running discipline track**: `tasks/TRACK-OS-PROOF-INFRASTRUCTURE.md`
**Bundle spec**: `docs/testing/evidence-bundle.md`

## Phase 5 closure summary

### Cut-by-cut log

| Cut | Scope | Deliverables |
|---|---|---|
| P5-00 | proof-manifest layout split v1 → v2 | `source/apps/selftest-client/proof-manifest.toml` (1433 LoC) split into a directory tree (`manifest.toml` + `phases.toml` + `markers/*.toml` + `profiles/*.toml`); `[meta] schema_version = "2"` introduced; `nexus-proof-manifest` parser extended with `[include]` glob expansion (lex-sorted, conflict-checked) while keeping v1 single-file back-compat. `scripts/qemu-test.sh`, `tools/os2vm.sh`, `selftest-client/build.rs`, and the CLI repointed to `proof-manifest/manifest.toml`. `PROFILE=full` ladder byte-identical to the pre-split baseline. |
| P5-01 | `nexus-evidence` skeleton + canonical hash spec | New host-only crate `source/libs/nexus-evidence/` with `Bundle` + per-artifact subtypes, `canonical_hash` (`H(meta) \|\| H(manifest_bytes) \|\| H(uart_normalized) \|\| H(sorted(trace)) \|\| H(sorted(config))`), 6 integration tests in `tests/canonical_hash.rs`. Spec authored in `docs/testing/evidence-bundle.md`. |
| P5-02 | Bundle assembly + trace extractor + config builder + CLI | `Bundle::assemble`, `extract_trace` (substring matching against all manifest literals; `[ts=…ms]` timestamp prefix; deny-by-default for orphan `SELFTEST:` / `dsoftbusd:` lines), `gather_config`, reproducible `tar.gz` packing in `bundle_io.rs` (`mtime=0`, `uid=0`, `gid=0`, mode `0o644`, lex-sorted entries, gzip OS byte fixed). `nexus-evidence` CLI ships `assemble / inspect / canonical-hash`. 5 integration tests in `tests/assemble.rs`. |
| P5-03 | Ed25519 sign / verify + 5 tamper classes + shell wrappers | `ed25519-dalek` (already in OS graph via `userspace/updates`). 102-byte signature wire format (`magic="NXSE" \|\| version=0x01 \|\| label \|\| hash[32] \|\| sig[64]`); `KeyLabel::{Ci, Bringup}` baked into the signature so `verify --policy=ci` rejects bringup-signed bundles. `Bundle::seal` / `Bundle::verify` + signature carried by `bundle_io::{read_unsigned, write_unsigned}`. CLI extended with `seal / verify / keygen`. `tools/seal-evidence.sh` + `tools/verify-evidence.sh`. 13 integration tests in `tests/sign_verify.rs`. Placeholder `keys/evidence-ci.pub.ed25519` checked in. |
| P5-04 | Key separation (CI env vs bringup file) + secret scanner | `nexus_evidence::key::from_env_or_dir` (CI: env, bringup: file with mandatory `0600` perm check). `tools/gen-bringup-key.sh` + `tools/gen-ci-key.sh`. Deny-by-default secret scanner in `src/scan.rs` (PEM blocks, `bringup-key/private` paths, `*PRIVATE_KEY*=…` env-style assignments, ≥64-char base64 high-entropy blobs) wired into `Bundle::seal`. **API change**: `Bundle::seal` now returns `Result<Bundle, EvidenceError>` (callers must handle the new `EvidenceError::SecretLeak`). `.gitignore` rejects `**/private.ed25519` belt-and-braces. 11 integration tests (7 scan + 4 key_separation). |
| P5-05 | qemu-test.sh + os2vm.sh seal post-pass + CI gate | Post-pass evidence pipeline wired into `scripts/qemu-test.sh` (single bundle) and `tools/os2vm.sh` (per-node A/B bundles `…-a.tar.gz` / `…-b.tar.gz`). Env knobs: `NEXUS_EVIDENCE_SEAL=1`, `CI=1` (implies seal + rejects `NEXUS_EVIDENCE_DISABLE=1`), `NEXUS_EVIDENCE_DISABLE=1`. Label resolution: CI key when `NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64` is set, bringup otherwise. Failure to assemble or seal is fatal. 5 integration tests in `tests/qemu_seal_gate.rs`. |
| P5-06 | Closure (this cut) | `docs/testing/evidence-bundle.md` final pass (§3a Assembly, §3b Signing & verification, §3c Key separation, §3d Secret scanner, §5 Operational gates with the env-knob matrix and CI hard gates). RFC-0038 §"Stop conditions / acceptance" Phase 5 ticked (7 boxes). `.cursor/{handoff,current_state,next_task_prep}` synced. Phase-6 plan to be authored at the start of the Phase-6 session as a separate plan file. |

### Behavioral parity gates (verified at every cut)

- QEMU `SELFTEST:` ladder for `PROFILE=full` byte-identical to the pre-Phase-5 baseline (manifest split is structural-only; `pm_mirror_check` enforces this on every `qemu-test.sh` run).
- `cargo test -p nexus-evidence` → 40 tests across 6 integration files (5 assemble + 6 canonical_hash + 4 key_separation + 5 qemu_seal_gate + 7 scan + 13 sign_verify); 0 failures.
- `cargo test -p nexus-proof-manifest` → still green (v1 + v2 layout both supported; back-compat tests added in P5-00).
- `cargo clippy -p nexus-evidence --all-targets -- -D warnings` → clean.
- `just dep-gate` → no forbidden crates in OS graph (the `ed25519-dalek` add was already in the graph via `userspace/updates`; the Phase-5 work added zero new forbidden deps).
- `bash scripts/check-selftest-arch.sh` → 6/6 rules clean.
- End-to-end smoke (assemble → seal → verify) confirmed for the bringup label against a synthetic uart.log + the v2 manifest.

### Operational lessons captured this phase (apply forward)

- The trace extractor's deny-by-default for orphan `SELFTEST:` / `dsoftbusd:` lines is the right floor, but mirroring `verify-uart`'s substring-against-all-manifest-literals rule (instead of a small allowlist of marker prefixes) is what made it usable on the real ladder. Tighten the manifest first, not the extractor.
- Reproducible `tar.gz` requires both `mtime=0` on every header AND a fixed gzip OS byte (`flate2` defaults to "unknown=0xff", which differs across stdlib versions); without the latter, two assembles of the same bundle on the same machine differ by one byte.
- `Bundle::seal` returning `Result` (instead of an infallible signature) is a deliberate ratchet: the secret scanner runs *before* signing, so a bundle that would commit a leaked PEM block / private-key path / high-entropy blob refuses to seal. Tests use `Bundle::seal_with(&allowlist)` to suppress the high-entropy heuristic on synthetic UART; production callers always use `Bundle::seal`.
- Permission check for the bringup key file is mode `== 0o600` exactly (not `<= 0o600`). World-readable bringup keys are rejected with `EvidenceError::KeyMaterialPermissions { mode }` so the operator gets a stable diagnostic + remediation.
- `ed25519-dalek` was already in the OS graph through `userspace/updates`, so the `nexus-evidence` add introduced zero new forbidden crates. Always `cargo tree -i <crate>` before assuming a new dep is fresh.
- The 102-byte signature wire format is now an external contract. The `magic` + `version` + `label` prefix lets verifiers reject malformed / wrong-version / wrong-label bundles before invoking the (relatively expensive) Ed25519 verify. Future format changes need an RFC tick + version bump.
- `scripts/qemu-test.sh` and `tools/os2vm.sh` both use the same env-knob matrix (`CI=1` ⇒ seal mandatory; `NEXUS_EVIDENCE_DISABLE=1` rejected when seal is mandatory). Locking the matrix in `tests/qemu_seal_gate.rs` (5 tests over `bash -c` of the actual gate snippet) caught one drift bug during Phase-5 development.

## Phase 6 — current state

Phase-6 contract is locked in `RFC-0038` and is 6 cuts (replay capability). Active execution plan file: `/home/jenning/.cursor/plans/task-0023b-phase6-cut-loop_1d8498c9.plan.md`.

Phase-6 scope summary:

- `tools/replay-evidence.sh` — extract bundle, validate signature (Phase 5), pin git-SHA, set recorded env + kernel cmdline + QEMU args, invoke `just test-os PROFILE=<recorded>`, capture fresh trace, compare against original.
- `tools/diff-traces.sh` — deterministic phase-by-phase, order-aware diff with classes (`exact_match`, `extra_marker`, `missing_marker`, `reorder`, `phase_mismatch`); exits 0 only on exact match modulo documented allowlist.
- `tools/bisect-evidence.sh` — walk git-SHA range with mandatory `--max-commits` + `--max-seconds` budgets.
- `scripts/regression-bisect.sh` — typical CI-failure flow wrapper.
- Cross-host determinism floor (CI runner + 1 dev box) for the same bundle; documented allowlist for non-deterministic surfaces.
- `docs/testing/replay-and-bisect.md` documents workflow + allowlist + extension procedure.
- Completed cuts: P6-01 / P6-02 / P6-03 / P6-04 / P6-06.
- Open cut: P6-05 (external CI-runner evidence capture; native + containerized CI-like pass already recorded).

## Frozen baseline that must stay green (verified end-of-Phase-5; carries into Phase 6)

- Host:
  - `cargo test -p dsoftbusd -- --nocapture`
  - `just test-dsoftbus-quic`
  - `cargo test -p nexus-proof-manifest -- --nocapture`
  - `cargo test -p nexus-evidence -- --nocapture` (40 tests; new in Phase 5)
- OS:
  - `just test-os PROFILE=full` (writes + seals bundle when `NEXUS_EVIDENCE_SEAL=1`)
  - `just test-os PROFILE=smp`
  - `just test-os PROFILE=quic-required`
  - `just test-os PROFILE=os2vm`
  - `just test-os PROFILE=bringup` (runtime profile)
  - `just test-os PROFILE=none` (runtime profile)
- Hygiene:
  - `just dep-gate` (chains `arch-gate` first; both must pass)
  - `just diag-os && just fmt-check && just lint`
  - `cargo clippy -p nexus-evidence --all-targets -- -D warnings`
- Evidence:
  - `tools/verify-evidence.sh target/evidence/<latest> --policy=bringup` returns 0 (or `--policy=ci` in CI)

## Boundaries reaffirmed

- Phase 5 is closed and behavior-preserving: same marker order, same proof meanings, same reject behavior across all 7 cuts. `PROFILE=full` ladder byte-identical to the pre-Phase-5 baseline.
- The 102-byte signature wire format (`magic="NXSE" || version=0x01 || label || hash[32] || sig[64]`) is now an external contract. Format changes require an RFC tick + version bump.
- The `EvidenceError` enum is append-only across cuts (no rename, no removal). Phase 5 added: `SignatureMissing`, `SignatureMalformed`, `SignatureMismatch`, `KeyLabelMismatch`, `UnsupportedSignatureVersion`, `SecretLeak`, `KeyMaterialMissing`, `KeyMaterialPermissions`. Phase 6 may add more (e.g. for replay determinism failures) but cannot rename.
- `nexus-evidence` is host-only; `cargo tree -i ed25519-dalek` confirms it stays out of the OS-only path that `dep-gate` enforces (the dep already existed in the OS graph through `userspace/updates`, but `nexus-evidence` itself is host-only). Phase 6 tools must keep this property.
- Visibility ceiling for new code: `pub(crate)` unless an external contract is required. No new `unwrap`/`expect`. Private key material never lands on disk in CI (env-only); on local dev it lives at `~/.config/nexus/bringup-key/private.ed25519` (mode `0600`) and `**/private.ed25519` is blanket-gitignored.
- No kernel changes across all 6 phases. Phase 6 replay tooling reads bundles + invokes existing recipes; it does not add kernel APIs.
- `TASK-0024` was unblocked at Phase 4 closure and is no longer in this task's blocking set.

## Next handoff target

- **Active plan**: `/home/jenning/.cursor/plans/task-0023b-phase6-cut-loop_1d8498c9.plan.md`.
- **Resume point**: Cut **P6-05** — execute replay on external CI runner for `target/evidence/20260420T133203Z-full-b84e4c2.tar.gz` and compare against existing `.cursor/replay-dev-a.json` and `.cursor/replay-ci-like.json`.
- **Per-cut cadence (Phase 6 carries the Phase-5 floor + adds replay determinism)**:
  1. `RUSTFLAGS='--cfg nexus_env="os" -W unexpected_cfgs -W dead_code' cargo +nightly check -p selftest-client --no-default-features --features os-lite --target riscv64imac-unknown-none-elf`
  2. `cargo test -p dsoftbusd -- --nocapture`
  3. `just test-dsoftbus-quic`
  4. `just test-os PROFILE=full` (verify-uart + evidence post-pass; should be deterministic)
  5. `cargo test -p nexus-proof-manifest -- --nocapture`
  6. `cargo test -p nexus-evidence -- --nocapture`
  7. `rustfmt +stable <touched .rs files only>`; verify and revert any submodule drift via `git checkout -- <unintended>`
  8. `just dep-gate` (chains `arch-gate` first; both must pass)
  9. `just lint` + `cargo clippy -p nexus-evidence --all-targets -- -D warnings`
  10. From P6-01 onward: `tools/verify-evidence.sh target/evidence/<latest>` returns 0
  11. From P6-01 onward: `tools/replay-evidence.sh target/evidence/<latest>` produces a fresh bundle whose `trace.jsonl` exact-matches the original (modulo the documented allowlist).
- **Phase 6 closure tasks (after external CI replay artifact capture)**: tick RFC-0038 Phase 6 checklist (6 boxes); sync `.cursor/{handoff/current.md, next_task_prep.md, current_state.md}`; refresh `tasks/STATUS-BOARD.md` + `tasks/IMPLEMENTATION-ORDER.md`; mark TASK-0023B as **CLOSED** (all 6 phases complete).

### Phase-6 proof-floor evidence (verified locally)

| Floor item | Evidence |
| --- | --- |
| Empty diff vs good bundle (native) | `.cursor/replay-dev-a.json` — `trace_diff.status == "exact_match"` |
| Empty diff vs good bundle (CI-like container) | `.cursor/replay-ci-like.json` — `trace_diff.status == "exact_match"` |
| Synthetic bad-bundle classified diff + non-zero exit | `.cursor/replay-synthetic-bad.{log,json}` — `status: "diff", classes: ["missing_marker"]`, exit 1 |
| 3-commit good→drift→regress bisect | `.cursor/bisect-good-drift-regress.json` — `first_bad_commit: c2cccccc`, `drift_commits: [c1bbbbbb]` |
| Performance floor | warm replay ~14s vs cold ~67s (script log shows `NEXUS_SKIP_BUILD=1` on second run) |
| Hard gate: `--max-seconds` mandatory | `tools/replay-evidence.sh <bundle>` exits 2 with usage |
| Hard gate: env override rejected | `PROFILE=full tools/replay-evidence.sh ... --max-seconds=30` exits 1 with explicit `[replay][error]` |
| Hard gate: `--max-commits` mandatory | `tools/bisect-evidence.sh <good> <bad> --max-seconds=30` exits 1 |

### Single remaining Phase-6 item (environmental, not a code/doc gap)

- [ ] External project CI runner replays `target/evidence/20260420T133203Z-full-b84e4c2.tar.gz` per `docs/testing/replay-and-bisect.md` §7-§8, archives `.cursor/replay-ci.json` + `.cursor/replay-ci.log`, then flips P6-05 line in `tasks/TASK-0023B-...md`, ticks Phase-6 checkbox in `docs/rfcs/RFC-0038-...md`, and mirrors status in STATUS-BOARD / IMPLEMENTATION-ORDER / `.cursor/{current_state,handoff/current,next_task_prep}.md`.
