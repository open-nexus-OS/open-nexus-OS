# Current Handoff: TASK-0056 implementation checkpoint (v2a present scheduler + double-buffer + input routing)

**Date**: 2026-04-30  
**Completed task**: `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md` — `Done`  
**Completed contract**: `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md` — `Done`  
**Active task (execution SSOT)**: `tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md` — `In Progress`  
**Completed contract**: `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md` — `Done`  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Carry-in baseline (must stay true)

- `TASK-0055`/`RFC-0047` headless present authority is `Done`.
- `TASK-0055B`/`RFC-0048` visible scanout bootstrap is `Done`.
- `TASK-0055C`/`RFC-0049` visible first SystemUI frame is `Done`.
- 56 must extend the same `windowd` authority path; no sidecar present/input authority.

## TASK-0056 implementation checkpoint

- RFC seed for 56 exists and is linked in both task and RFC index.
- 56 task header is synchronized (`depends-on`, `follow-up-tasks`, Gate E mapping, security invariants, red flags).
- Implemented in `windowd` only:
  - double-buffer surface present contract,
  - minimal scheduler/fence semantics,
  - deterministic input hit-test/focus/keyboard routing.
- `launcher` and `selftest-client` are proof consumers only; they do not own present/input authority.
- Out-of-scope remains explicit:
  - visible cursor polish (`TASK-0056B`),
  - latency/perf tuning (`TASK-0056C`),
  - WM/compositor-v2 breadth (`TASK-0199`/`TASK-0200`),
  - kernel production-grade closure claims.

## Proof state

Green so far:

- Closure rerun `cargo test -p windowd -p launcher -p ui_v2a_host -- --nocapture` — 22 tests across the three target packages.
- Closure rerun `cargo test -p ui_v2a_host reject -- --nocapture` — 5 reject-filtered tests.
- Earlier regression proof `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture`.
- `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite`.
- Closure rerun `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` — `verify-uart` accepted through `SELFTEST: ui v2 input ok`.

Observed v2a marker ladder:

- `windowd: present scheduler on`
- `windowd: input on`
- `windowd: focus -> 1`
- `launcher: click ok`
- `SELFTEST: ui v2 present ok`
- `SELFTEST: ui v2 input ok`

Deferred by user instruction; do not claim `Done` until run:

- `scripts/fmt-clippy-deny.sh`
- `just test-all`
- `just ci-network`
- `make clean`, `make build`, `make test`, `make run`

Closure sync note: touched headers, ADR/architecture/testing docs, task/RFC notes, and marker-honesty gating are updated. `SELFTEST: ui v2 input ok` is emitted only after input routing and launcher click evidence are both true.

Note: the GTK/QEMU window may still show `Guest has not initialized the display (yet)` during or after early marker-stop runs. This checkpoint proves guest-side `ramfb`/UART marker evidence plus v2a scheduler/input semantics; it does not add an independent screenshot/GTK refresh proof.
