<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Input

Input is routed through the window manager and translated into UI events.

Goals:

- deterministic delivery order,
- consistent focus model,
- accessibility semantics (actions/roles),
- cross-device affordances (touch + pointer + keyboard).

## v2a windowd routing baseline

`TASK-0056` makes `windowd` the single authority for the first functional input-routing baseline.

- Pointer hit-test walks the committed layer tree from topmost to bottommost visible surface.
- Focus follows a routed pointer-down event.
- Keyboard delivery targets the focused surface only.
- Stale or unauthorized surface references reject deterministically.
- Input event queues are bounded; overflow is a stable reject, not a silent success.

The v2a baseline intentionally does not claim cursor visuals, pointer polish, latency budgets, HID/touch device plumbing, or WM-wide shortcuts. Those remain follow-up scope.
