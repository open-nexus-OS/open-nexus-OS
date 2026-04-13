# DSoftBus Mux v2 Contract (TASK-0020)

## Scope

This document summarizes the TASK-0020 host-first contract for DSoftBus Streams v2 mux behavior.

- Execution/proof SSOT: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- Contract SSOT: `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md`
- Out of scope here: QUIC transport (`TASK-0021`) and core/no_std extraction (`TASK-0022`)

## Contract surfaces

- Typed protocol domains (`newtype`): `StreamId`, `PriorityClass`, `StreamName`, `WindowCredit`
- Deterministic reject labels:
  - `mux.reject.frame_oversize`
  - `mux.reject.invalid_stream_state_transition`
  - `mux.reject.window_credit_overflow_or_underflow`
  - `mux.reject.unknown_stream_frame`
  - `mux.reject.unauthenticated_session`
  - `mux.reject.duplicate_stream_name`
  - `mux.reject.invalid_stream_name`
- Bounded defaults:
  - max streams: `128`
  - max frame payload: `32 KiB`
  - max buffered bytes per stream: `256 KiB`
  - keepalive interval/timeout: deterministic tick policy
- Scheduling:
  - priorities `0..=7` (`0` highest)
  - strict high-priority preference with bounded starvation budget
  - deterministic lower-priority round-robin release when lower queues are pending

## Requirement-based host test matrix

| Requirement | Test file | Primary checks |
| --- | --- | --- |
| Rejects and bounds | `userspace/dsoftbus/tests/mux_contract_rejects_and_bounds.rs` | reject taxonomy, bounded backpressure, mixed-priority fairness pressure, naming rejects |
| Frame/state/keepalive contract | `userspace/dsoftbus/tests/mux_frame_state_keepalive_contract.rs` | lifecycle transitions, keepalive semantics, seeded accounting invariants, idempotent RST behavior |
| Endpoint integration contract | `userspace/dsoftbus/tests/mux_open_accept_data_rst_integration.rs` | open/accept/data/rst flow, duplicate-name reject on ingest, unauthenticated fail-closed, teardown reject paths |

## Canonical commands

Host contract proofs:

```bash
cargo test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture
cargo test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture
cargo test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture
cargo test -p dsoftbus -- --nocapture
```

Mandatory slice regressions:

```bash
just test-e2e
just test-os-dhcp
```

OS-gated harness checks (marker ladder still gated):

```bash
RUN_UNTIL_MARKER=1 just test-os
just test-dsoftbus-2vm
```

Review `summary.json` and `summary.txt` under `artifacts/os2vm/runs/<runId>/` for each 2-VM run.

## Marker honesty rule

- Reached today: transport/session DSoftBus markers in canonical harness output.
- Still gated: mux-specific marker ladder (`dsoftbus:mux ...`, `SELFTEST: mux ...`).

Do not claim TASK-0020 complete until mux-specific OS marker stop conditions are actually green.
