# Current Handoff: TASK-0056B implementation checkpoint (visible input cursor/hover/focus/click)

**Date**: 2026-04-30  
**Completed task**: `tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md` — `Done`  
**Completed contract**: `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md` — `Done`  
**Active task**: `tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md` — `In Progress` (deterministic visible input; live QEMU input moved to `TASK-0252`/`TASK-0253`)
**Active contract**: `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md` — `In Progress` (Phase 3 re-scoped to `TASK-0252`/`TASK-0253`)
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Carry-in baseline (must stay true)

- `TASK-0055`/`RFC-0047`, `TASK-0055B`/`RFC-0048`, `TASK-0055C`/`RFC-0049`, and `TASK-0056`/`RFC-0050` are `Done`.
- Input authority for hit-test, hover, focus, and click must remain in `windowd`; launcher/selftest stay proof consumers.
- 56B is deterministic visible input only. It must not absorb the live input pipeline (`TASK-0252`/`TASK-0253`), perf/latency closure (`TASK-0056C`), WM-v2 breadth (`TASK-0199`/`TASK-0200`), or display-service integration (`TASK-0251`).
- Downstream fast-lane tasks have been uplifted to target Orbital-Level UX with Open Nexus architecture: live input and SVG-source UI assets are mandatory carry-in for shell/launcher/Desktop claims, while authority remains split across input source, `windowd`, SystemUI, and app/session services.

## 56B implementation checkpoint

- Implemented in the existing `windowd` authority path:
  - routed pointer movement with deterministic visible cursor pixels,
  - focus-follows-click with deterministic focus affordance pixels,
  - one launcher proof surface whose visible click marker is gated on `windowd` visible-input evidence.
- The QEMU `visible-bootstrap` path keeps `windowd` as input/present authority and writes the 56B composed visible-input frame into the same `ramfb` target after the visible SystemUI frame.
- Live host mouse/keyboard input through QEMU is now the immediate follow-up lane: `TASK-0252` host input core, then `TASK-0253` OS/QEMU `inputd`/HID integration.

## Proofs green so far (deterministic route only)

- `cargo test -p ui_v2a_host -- --nocapture` — 19 tests.
- `cargo test -p ui_v2a_host reject -- --nocapture` — 12 reject-filtered tests.
- `cargo test -p windowd -p launcher -- --nocapture` — 15 tests.
- `cargo test -p selftest-client -- --nocapture`.
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` — green through `SELFTEST: ui visible input ok`; `verify-uart` clean.
- Visual-proof fake-green investigation: fixed QEMU `etc/ramfb` config field order in
  `selftest-client` to the required `addr, fourcc, flags, width, height, stride`
  ABI before accepting visible markers.
- Human-visible proof adjustment: the tiny 64x48 `windowd` visible-input frames are
  scaled to the 1280x800 `ramfb` scanout and written as cursor-start,
  hover/cursor-end, and final focus/click frames before visible-input success.

## Immediate follow-up live-input proof

- Add the real QEMU pointer/keyboard device/event path in `TASK-0252`/`TASK-0253`.
- Route live pointer/key events through bounded input authorities into `windowd`/IME; the event source must not own hit-test/hover/focus/click authority.
- Prove live host mouse movement updates visible cursor position in the QEMU window.
- Prove hover affordance, host-mouse click on the proof surface, and minimal keyboard delivery.
- Add marker honesty guards so live-pointer success cannot be satisfied by deterministic selftest injection alone.

## Remaining before Done

- Do not run until user explicitly approves:
  - `scripts/fmt-clippy-deny.sh`,
  - `just test-all`,
  - `just ci-network`,
  - `make clean`, `make build`, `make test`, `make run`.
- Preserve non-claims: no live `TASK-0252`/`TASK-0253` input pipeline in 56B, no `TASK-0056C` perf/latency, no `TASK-0199`/`TASK-0200` WM breadth, no `TASK-0251` display-service integration, no kernel production-grade closure.

## Downstream fast-lane uplift

- `tasks/IMPLEMENTATION-ORDER.md` now includes an Orbital-Level UX gate before `TASK-0119`/`TASK-0120`.
- `TASK-0252` and `TASK-0253` are pulled directly after `TASK-0056B`, and `TASK-0056C` follows them for responsiveness before scroll/animation/launcher quality claims.
- `TASK-0146` and `TASK-0147` are pulled directly after `TASK-0059` so IME keymaps/OSK/focus proofs exist before the SystemUI DSL desktop claim.
- New `TASK-0065B-session-login-greeter-v0.md` tracks the greeter/dev-session and SystemUI shell handoff.
- Dependent tasks `0057`, `0058`, `0059`, `0146`, `0147`, `0061`, `0062`, `0063`, `0064`, `0065`, `0070`, `0072`, `0073`, `0074`, `0080B`, `0080C`, `0119`, and `0120` now include live-input/SVG/session/launcher gates where relevant to prevent drift.
