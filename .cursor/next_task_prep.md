# Next Task Preparation (Drift-Free)

## Candidate next execution
- **task**: `TASK-0023` is current queue head but remains `Blocked`; execute `TASK-0024` next unless explicit resequencing to `TASK-0044` is requested.
- **focus**: preserve frozen TASK-0022 core/no_std closure contracts while selecting the next executable distributed slice.

## Latest completed slice (2026-04-15)
- `TASK-0022` implementation closure synchronized (`status: Done`, RFC-0036 `Complete`).
- Real crate split established:
  - `userspace/dsoftbus/core` (`dsoftbus-core`) added as no_std core crate boundary.
- Host/OS adapters remain in `userspace/dsoftbus` and consume/re-export core seams.
- Required reject/contract/perf trait tests are green including:
  - `test_reject_*` family,
  - `test_core_boundary_types_are_send_sync`,
  - `test_perf_backpressure_budget_is_deterministic`,
  - `test_zero_copy_borrow_view_preserves_payload_reference`.
- Gates green:
  - `cargo +nightly-2025-01-15 check -p dsoftbus-core --target riscv64imac-unknown-none-elf`
  - `cargo test -p dsoftbus --test core_contract_rejects -- --nocapture`
  - `cargo test -p dsoftbus -- reject --nocapture`
  - `just test-dsoftbus-quic`
  - `just diag-host`
  - `just deny-check`
  - `just dep-gate && just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `just test-e2e && just test-os-dhcp`

## Current proven baseline
- Host real QUIC transport proof: `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
- Host selection/reject/perf proof: `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
- QUIC keyword regression subset: `cargo test -p dsoftbus -- quic --nocapture`
- Targeted host QUIC aggregate: `just test-dsoftbus-quic`
- OS fallback marker proof: `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- OS dep hygiene: `just dep-gate`
- Regression floor:
  - `just test-e2e`
  - `just test-os-dhcp`

## Boundaries for next slice
- Keep `TASK-0021` closed/done; do not reopen host-proof scope.
- Keep `TASK-0022` closure frozen; do not reopen completed core/no_std split without explicit regression evidence.
- Do not silently pre-enable `TASK-0023` OS QUIC path outside its own feasibility gate contract.
- Do not absorb `TASK-0044` tuning matrix.

## Linked contracts
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
- `docs/rfcs/RFC-0036-dsoftbus-core-no-std-transport-abstraction-v1.md`
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `docs/testing/index.md`
- `docs/architecture/07-contracts-map.md`
- `docs/distributed/dsoftbus-lite.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `tasks/STATUS-BOARD.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`

## Ready condition
- Start from the frozen green proof set above and preserve anti-drift scope constraints.

## Harmonization readiness matrix
- **Achieved now**:
  - first-party `thiserror` line converged to `2.x`,
  - `snow` uplifted to `0.10.x` in `dsoftbus`,
  - `windows-sys` duplicate count reduced (`3 -> 2`) via `tempfile/rustix` uplift,
  - `identity` key generation migrated to `getrandom 0.3` byte-source,
  - unnecessary first-party `ed25519-dalek/rand_core` feature coupling removed,
  - `dsoftbus` local key derivation moved off `x25519-dalek` to `curve25519-dalek`,
  - `nexus-noise-xk` migrated to curve-based wrapper/newtypes with clamp + zeroize + all-zero DH reject.
- **Still open**:
  - `getrandom` split (`0.2` vs `0.3`) and `windows-sys` split (`0.52` vs `0.61`) remain compatibility-constrained and are now handled by narrow `cargo-deny` bans skip entries (strict mode retained).
- **Known blockers**:
  - `ring 0.17.x` binds to `getrandom 0.2` and `windows-sys 0.52`,
  - `tokio/quinn-udp` bind to `windows-sys 0.61`.

## Planning note
- Dependency convergence phase-2 closure remains complete.
- `TASK-0022` closure is complete (`Done`) and frozen; `RFC-0036` is `Complete`.
- Hybrid-phased bulk-path decision is locked for this task family:
  - phase-1 borrow-view seam in `TASK-0022`,
  - handle-first canonicalization remains follow-up scope.

## Post Phase-2 next action
- Treat residual duplicate lines as compatibility-constrained closure work:
  - either upstream-version convergence (ring/quinn/tokio ecosystem),
  - or explicit bounded accept-with-rationale for remaining split lines.
- Keep `TASK-0021` proof floor unchanged while executing `TASK-0022`.
