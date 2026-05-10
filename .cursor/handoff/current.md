# Current Handoff: TASK-0253 closure to TASK-0056C

**Date**: 2026-05-10  
**Reviewed task**: `tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md` — `In Review`  
**Closed contract seed**: `docs/rfcs/RFC-0053-input-v1_0b-os-qemu-live-input-hidrawd-touchd-inputd-contract.md` — `Done`  
**Closed driver-layer RFC**: `docs/rfcs/RFC-0054-input-v1_0c-os-qemu-virtio-input-driver-layer-contract.md` — `Done`  
**Next queue head**: `tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md`  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Closure snapshot

- The service-owned live-input chain is review-closed:
  - `virtio-input -> hidrawd -> inputd -> windowd -> fbdevd -> ramfb`
  - `selftest-client` remains observer-only for visible/input proof collection.
- The last closure pass fixed:
  - stale/missing headers across the 0253 slice and last-6-commit Rust files,
  - missing RFC-0054 negative-test coverage,
  - stale `systemui` desktop-profile test expectations,
  - runner honesty for time-capped `make run`.

## Verified green

- Focused proofs:
  - `cargo test -p virtio-input -- --nocapture`
  - `cargo test -p hidrawd -- --nocapture`
  - `cargo test -p inputd -- --nocapture`
  - `cargo test -p fbdevd -- --nocapture`
  - `cargo test -p selftest-client --test boot_cfg_runtime -- --nocapture`
  - `cargo test -p nx --test interactive_os_startup -- --nocapture`
  - `RUN_PHASE=input-startup RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s scripts/qemu-test.sh --profile=visible-bootstrap`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os visible-bootstrap`
- Non-excluded broad gates:
  - `just dep-gate`
  - `just diag-os`
  - `just diag-host`
  - `just ci-network`
  - `make clean -> make build`
  - `make test`
  - `RUN_TIMEOUT=220s make run`
  - `RUN_TIMEOUT=220s just start`

## Explicitly deferred

- `scripts/fmt-clippy-deny.sh`
- `just test-all`

These remain excluded by explicit user instruction and are the only named gates not rerun in this closeout.

## Carry-forward constraints

- Keep `windowd` as hit-test/hover/focus/click authority.
- Keep RFC-0052 crates as the only parser/keymap/repeat/accel authority.
- Do not back-claim perf/latency closure from 0253; that remains `TASK-0056C`.
- `nx input keymap set`, `nx input cursor`, and `nx input test type` remain host/preflight helpers until a later live-RPC task says otherwise.
