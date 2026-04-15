# Cursor Current State (SSOT)

## Current architecture state
- **last_decision**: close `TASK-0021` as `Done` with host-only real QUIC proof slice complete and OS QUIC still disabled-by-default.
- **active_constraints**:
  - execute only `TASK-0022` core/no_std extraction scope (no pre-enable work from `TASK-0023`),
  - do not absorb `TASK-0044` QUIC tuning breadth,
  - preserve `TASK-0021` strict `mode=quic` fail-closed and deterministic `mode=auto` fallback behavior,
  - keep closure language production-class (avoid broad production-ready overclaims for the distributed stack).

## Current focus (execution)
- **active_task**: `TASK-0022` planning follow-up (core/no_std transport split), with `TASK-0021` frozen
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
- Keep `TASK-0021` frozen/done and hand over execution planning to `TASK-0022`.

## Active follow-up planning
- **next_plan**: `TASK-0022` execution planning (core/no_std split) after `TASK-0021` closure.
- **plan_goal**: keep `TASK-0021` proof floor frozen while extracting reusable core transport seams for OS follow-on work.

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
