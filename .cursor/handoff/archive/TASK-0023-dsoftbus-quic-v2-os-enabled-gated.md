# Current Handoff: TASK-0023 OS QUIC closure

**Date**: 2026-04-16  
**Status**: `TASK-0023` is `Done` with real OS QUIC-v2 session path proven in QEMU.  
**Execution SSOT**: `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`

## What changed
- `dsoftbusd` OS path now selects QUIC transport and runs real UDP+Noise session flow.
- `selftest-client` probe now exercises the same QUIC-v2 framing/auth path over UDP IPC.
- `netstackd` loopback UDP routing was hardened for cross-port datagram exchange.
- `scripts/qemu-test.sh` now requires QUIC markers and fails on fallback markers for QUIC-required profile.
- Pure frame helpers added (`source/services/dsoftbusd/src/os/session/quic_frame.rs`) with reject-path tests.

## Security and evidence posture
- Strict fail-closed behavior remains mandatory (`test_reject_*` suites stay green).
- Host evidence green:
  - `just test-dsoftbus-quic`
  - `cargo test -p dsoftbusd -- --nocapture`
- OS evidence green:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  - required markers:
    - `dsoftbusd: transport selected quic`
    - `dsoftbusd: auth ok`
    - `dsoftbusd: os session ok`
    - `SELFTEST: quic session ok`
  - fallback markers absent:
    - `dsoftbusd: transport selected tcp`
    - `dsoftbus: quic os disabled (fallback tcp)`
    - `SELFTEST: quic fallback ok`

## Next handoff target
- Queue head remains `TASK-0024` for follow-up transport breadth/tuning.
- Keep `TASK-0023` closure semantics frozen; do not regress to fallback-only marker posture.
