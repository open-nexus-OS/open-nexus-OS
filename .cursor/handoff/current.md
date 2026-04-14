# Current Handoff: TASK-0021 closure + dependency harmonization

**Date**: 2026-04-14  
**Status**: `TASK-0021` is closed as `Done`; dependency harmonization is closed with strict deny + narrow compatibility exceptions for known upstream-constrained duplicate lines.  
**Execution SSOT**: `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`

## What is now proven
- Real host QUIC connect + bidirectional stream exchange is proven in:
  - `userspace/dsoftbus/tests/quic_host_transport_contract.rs`
- QUIC+mux smoke payload proof is now explicit in:
  - `test_quic_carries_mux_contract_smoke_payload` (same suite)
- QUIC selection/reject/fallback semantics remain green in:
  - `userspace/dsoftbus/tests/quic_selection_contract.rs`
- Host runtime selection path is wired in daemon start:
  - `DSOFTBUS_TRANSPORT=tcp|quic|auto` in `userspace/dsoftbus/src/lib.rs`
  - QUIC path has an explicit session gate before payload streams
- OS fallback marker ladder remains green (`REQUIRE_DSOFTBUS=1 ... just test-os`):
  - `dsoftbus: quic os disabled (fallback tcp)`
  - `dsoftbusd: transport selected tcp`
  - `SELFTEST: quic fallback ok`
- OS dependency hygiene remains green (`just dep-gate`).
- Regression floor commands are freshly green:
  - `just test-e2e`
  - `just test-os-dhcp`

## Scope lock carried forward
- `TASK-0021` includes host-only real QUIC proof + deterministic OS fallback contract.
- `TASK-0022` core/no_std extraction is **not** absorbed.
- `TASK-0023` OS QUIC enablement is **not** pre-implemented.

## Touched implementation points
- `userspace/dsoftbus/src/host_quic.rs`
- `userspace/dsoftbus/src/transport_selection.rs`
- `userspace/dsoftbus/src/lib.rs`
- `userspace/dsoftbus/tests/quic_host_transport_contract.rs`
- `userspace/dsoftbus/tests/quic_selection_contract.rs`
- `userspace/dsoftbus/Cargo.toml`
- `justfile`
- `docs/architecture/07-contracts-map.md`
- `docs/architecture/README.md`
- `docs/architecture/networking-authority.md`
- `docs/distributed/dsoftbus-lite.md`
- `docs/distributed/dsoftbus-mux.md`
- `docs/adr/0005-dsoftbus-architecture.md`

## Harmonization delta (higher-version-first)
- Applied first-party `thiserror` uplift to `2.x` across userspace/services touched by current graph.
- Upgraded `userspace/dsoftbus` `snow` from `0.9` to `0.10` and adapted builder call sites for new Result-returning API.
- Updated `tempfile`/`rustix` to newest compatible line, reducing `windows-sys` fragmentation from 3 versions to 2.
- Decoupled `userspace/identity` from direct `rand_core 0.6` usage by switching identity generation to `getrandom 0.3` + `SigningKey::from_bytes`.
- Removed `rand_core`-coupled `ed25519-dalek` feature flags in first-party manifests where unnecessary.
- Current `just deny-check` status:
  - `multiple-versions = "deny"` remains active,
  - narrow skips are defined only for `getrandom` (`0.2` + `0.3`) and `windows-sys` (`0.52` + `0.61`).

## Remaining blockers (explicit)
- `ring 0.17.x` keeps the `getrandom 0.2` line alive.
- `ring`/`quinn-udp` still pin `windows-sys 0.52`, while `tokio`/`quinn-udp` use `0.61`.

## Next action
- Keep `TASK-0021` frozen/done and move sequential execution to `TASK-0022` planning and scoped implementation.

## Planned next slice
- Start `TASK-0022` planning/execution with explicit focus on:
  - extracting reusable core/no_std transport seams from the proven host path,
  - preserving existing `TASK-0021` host proof contracts unchanged,
  - preparing a clean handoff boundary for `TASK-0023` OS QUIC enablement work.

## Phase 2 result
- Completed identity RNG decoupling from direct `rand_core 0.6` dependency.
- Completed first-party dalek feature minimization (`ed25519-dalek` `rand_core` feature removed where not needed).
- Removed `dsoftbus` direct `x25519-dalek` dependency for local key derivation; switched to `curve25519-dalek` derivation path.
- Hardened `nexus-noise-xk` with minimal internal wrappers/newtypes around curve-based X25519 operations:
  - clamp on secret construction,
  - zeroize on secret/shared drop,
  - all-zero DH output rejection.
- Residual duplicates are now compatibility-anchored by:
  - `ring 0.17.x` (`getrandom 0.2`, `windows-sys 0.52`),
  - `tokio`/`mio` (`windows-sys 0.61`).
- Required gates remained green:
  - `just deny-check`, `just test-dsoftbus-quic`, `just dep-gate`, `just test-os-dhcp`.
