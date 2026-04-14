# Next Task Preparation (Drift-Free)

## Candidate next execution
- **task**: `TASK-0022` (core/no_std transport refactor)
- **focus**: extract reusable transport core seams without reopening `TASK-0021` host QUIC scope.

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
- Do not absorb `TASK-0022` core/no_std extraction.
- Do not pre-enable or pre-unblock `TASK-0023` OS QUIC path.
- Do not absorb `TASK-0044` tuning matrix.

## Linked contracts
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
- Dependency convergence phase-2 closure is complete; next sequential planning target is `TASK-0022`.

## Post Phase-2 next action
- Treat residual duplicate lines as compatibility-constrained closure work:
  - either upstream-version convergence (ring/quinn/tokio ecosystem),
  - or explicit bounded accept-with-rationale for remaining split lines.
- Keep `TASK-0021` proof floor unchanged while evaluating that closure path.
