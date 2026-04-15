# Cursor Current State (SSOT)

## Current architecture state
- **last_decision**: close `TASK-0022` as `Done` after green quality/security/performance gates and synchronized closure docs.
- **active_constraints**:
  - execute only `TASK-0022` core/no_std extraction scope (no pre-enable work from `TASK-0023`),
  - do not absorb `TASK-0044` QUIC tuning breadth,
  - preserve `TASK-0021` strict `mode=quic` fail-closed and deterministic `mode=auto` fallback behavior,
  - keep closure language production-class (avoid broad production-ready overclaims for the distributed stack).

## Current focus (execution)
- **active_task**: `TASK-0022` closure synchronized (`Done`), with `TASK-0021` frozen
- **seed_contract**:
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
  - `docs/rfcs/RFC-0036-dsoftbus-core-no-std-transport-abstraction-v1.md`
  - `docs/testing/index.md`

## Task-0022 prep snapshot (2026-04-14)
- RFC seed created and linked: `RFC-0036`.
- Contract guidance sharpened for:
  - discovery/auth/transmission plane separation,
  - auth separate from policy authority,
  - channel-authoritative service identity (`sender_service_id`),
  - transport-agnostic core behavior,
  - zero-copy-first bulk data policy.
- `.cursor` prep files aligned for TASK-0022 kickoff (`context_bundles`, `handoff/current`, `next_task_prep`, `pre_flight`, `stop_conditions`).

## Implemented in this slice
- Added host QUIC probe backend: `userspace/dsoftbus/src/host_quic.rs`.
- Wired host runtime transport selection into daemon start (`DSOFTBUS_TRANSPORT=tcp|quic|auto`) in `userspace/dsoftbus/src/lib.rs`.
- Added host QUIC daemon runtime path with session-gate handshake (connect request/response before payload streams) in `userspace/dsoftbus/src/lib.rs`.
- Added behavior-first real transport suite: `userspace/dsoftbus/tests/quic_host_transport_contract.rs`.
- Added explicit QUIC+mux smoke coverage (`test_quic_carries_mux_contract_smoke_payload`) proving TASK-0020 wire-event payload roundtrip + receiver ingest over real QUIC.
- Updated selection contract metadata + `#[must_use]` hardening in `userspace/dsoftbus/src/transport_selection.rs`.
- Added targeted QUIC proof gate: `just test-dsoftbus-quic`.
- Kept OS fallback marker contract unchanged in behavior (`dsoftbus: quic os disabled (fallback tcp)` ladder).

## Proof status (green)
- `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
- `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
- `cargo test -p dsoftbus -- quic --nocapture`
- `just test-dsoftbus-quic`
- `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- `just dep-gate`
- `just test-e2e`
- `just test-os-dhcp`

## Scope boundaries reaffirmed
- `TASK-0022`: active scope (core/no_std split + transport abstraction only).
- `TASK-0023`: untouched; OS QUIC remains disabled-by-default.
- `TASK-0044`: untouched; no tuning breadth absorbed into this task.

## Dependency harmonization status (2026-04-14)
- **Target matrix (prefer higher line)**:
  - `thiserror`: target `2.x` (achieved in first-party manifests).
  - `snow`: target `0.10.x` (achieved in `userspace/dsoftbus`).
  - `getrandom`/`rand_core`: target `0.3.x`/`0.9.x` where feasible (advanced; residual blockers remain).
  - `windows-sys`: reduce fragmentation toward newest compatible line (improved).
- **Measured delta**:
  - `thiserror` duplicate warnings removed from `just deny-check`.
  - `windows-sys` duplicate versions reduced from `3` to `2` after `tempfile/rustix` update.
  - `identity` no longer pins `rand_core 0.6` directly (switched to `getrandom 0.3` key generation path).
  - `dsoftbus` no longer enables `ed25519-dalek` `rand_core` feature.
  - Remaining compatibility-constrained duplicate lines: `getrandom`, `windows-sys`.
- **Confirmed blockers**:
  - `ring 0.17.x` keeps `getrandom 0.2` alive.
  - `ring 0.17.x` and `tokio/quinn-udp` keep split `windows-sys` lines (`0.52` + `0.61`).

## Next handoff target
- Keep `TASK-0021` and `TASK-0022` frozen/done; evaluate queue execution from `TASK-0023` blocked state.

## Active follow-up planning
- **next_plan**: keep `TASK-0023` marked blocked and select the next executable distributed slice (`TASK-0024`) unless explicitly resequenced.
- **plan_goal**: preserve the new core crate boundary and reject/determinism contracts while avoiding scope absorption.

## Dependency convergence closure snapshot (2026-04-14)
- **Implemented**:
  - `userspace/identity`: switched key generation to `getrandom 0.3` bytes + `SigningKey::from_bytes` (removed direct `rand_core` dependency).
  - `userspace/dsoftbus` and `userspace/identity`: removed unnecessary `ed25519-dalek` `rand_core` feature coupling.
  - `userspace/dsoftbus`: replaced local `x25519-dalek` key-derivation usage with `curve25519-dalek` base-point derivation, removing `dsoftbus` as a direct `rand_core 0.6` anchor.
- **Residual duplicate owners**:
  - `getrandom 0.2`: pinned by `ring 0.17.x`.
  - `windows-sys 0.52`: pinned by `ring`/`quinn-udp` side; `windows-sys 0.61`: pinned by `tokio`/`mio`.
  - both are explicitly and narrowly allowed via `config/deny.toml` bans skip entries while `multiple-versions = "deny"` remains enforced.
- **Gate results**:
  - `just deny-check`: pass (duplicates remain as above, no bypass).
  - `just test-dsoftbus-quic`: pass.
  - `just dep-gate`: pass.
  - `just test-os-dhcp`: pass.

## Noise wrapper hardening snapshot (2026-04-14)
- `source/libs/nexus-noise-xk` migrated from `x25519-dalek` to `curve25519-dalek` with minimal internal wrapper newtypes:
  - secret/public/shared wrappers added,
  - secret clamping centralized on construction,
  - secret/shared material zeroized on drop,
  - DH now rejects all-zero shared secret (`NoiseError::InvalidSharedSecret`).
- **Duplicate delta after hardening**:
  - `rand_core` duplicate warning removed from `just deny-check`.
  - remaining duplicate warnings: `getrandom`, `windows-sys`.

## TASK-0022 execution snapshot (2026-04-15, hybrid phase-1)
- Added `userspace/dsoftbus/src/core_contract.rs` with no_std-friendly (`core + alloc`) transport-neutral helpers:
  - `BorrowedFrameTransport` adapter seam,
  - ownership-safe `OwnedRecord`/borrow-view contract,
  - bounded correlation nonce guard,
  - deterministic reject labels for state/correlation/bounds/auth/identity-spoof paths.
- Wired host identity reject through shared core helper in `userspace/dsoftbus/src/host.rs`:
  - payload identity is checked against channel-authoritative identity using `validate_payload_identity_spoof_vs_sender_service_id`.
- Replaced implicit OS placeholder seam with explicit unsupported adapter boundary in `userspace/dsoftbus/src/os.rs`.
- Added requirement-named reject proofs:
  - `userspace/dsoftbus/tests/core_contract_rejects.rs`
  - `test_reject_invalid_state_transition`
  - `test_reject_nonce_mismatch_or_stale_reply`
  - `test_reject_oversize_frame_or_record`
  - `test_reject_unauthenticated_message_path`
  - `test_reject_payload_identity_spoof_vs_sender_service_id`
- Proof snapshot (green):
  - `cargo test -p dsoftbus --test core_contract_rejects -- --nocapture`
  - `cargo test -p dsoftbus -- reject --nocapture`
  - `just test-dsoftbus-quic`
  - `just diag-host`
  - `just deny-check`
  - `just dep-gate && just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `just test-e2e && just test-os-dhcp`

## TASK-0022 closure resolution (2026-04-15)
- Gap-lock resolution completed in this slice:
  - `dsoftbus-core` crate boundary is real (`userspace/dsoftbus/core`, package `dsoftbus-core`),
  - explicit no_std proof is green: `cargo +nightly-2025-01-15 check -p dsoftbus-core --target riscv64imac-unknown-none-elf`,
  - deterministic Phase-D evidence is green (`test_perf_backpressure_budget_is_deterministic`, `test_zero_copy_borrow_view_preserves_payload_reference`),
  - `Send`/`Sync` compile-time contract assertion is green (`test_core_boundary_types_are_send_sync`),
  - Phase-E sync completed across task/RFC/testing/status/handoff/current-state surfaces.
- `os2vm` note clarified for TASK-0022:
  - 2-VM proof remains conditional on asserting new distributed behavior claims; this closure slice does not assert new distributed behavior.

## TASK-0022 review verification pass (2026-04-15)
- Re-ran quality/security/performance gates while task status is `In Review`:
  - `cargo +nightly-2025-01-15 check -p dsoftbus-core --target riscv64imac-unknown-none-elf`
  - `cargo test -p dsoftbus --test core_contract_rejects -- --nocapture`
  - `cargo test -p dsoftbus -- reject --nocapture`
  - `just test-dsoftbus-quic`
  - `just deny-check`
  - `just dep-gate && just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `just test-e2e && just test-os-dhcp`
