<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Display Output Hardening Matrix

This matrix is the fast path for the current visible-output closure. It keeps
`selftest-client` observer-only and requires each service-owned hop to fail with
a precise label before QEMU is used as the final proof.

| Gate | Owner | Host proof | QEMU evidence |
| --- | --- | --- | --- |
| Capability routing for display | `init-lite` | `cargo test -p nx --test interactive_os_startup init_wires_fbdevd_caps_and_routes_for_service_owned_display_observer_chain -- --nocapture` | `init: fbdevd slots ...`, `init: selftest fbdevd slots ...` |
| Visible-state protocol | `fbdevd` | `cargo test -p fbdevd -- --nocapture` | no `fbdevd: recv failed`; bounded observer timeout on missing replies |
| First scanout ownership | `windowd` + `fbdevd` | `cargo test -p windowd -p fbdevd -- --nocapture` | `fbdevd: ready`, `fbdevd: map ok`, `fbdevd: ramfb configured`, `fbdevd: flush ok` |
| Live present after input | `inputd` + `windowd` | host service contract must assert input-visible state causes compose/present counters | `fps: inputd ... pointer_live=1 keyboard_live=1`, then `fps: windowd compose_hz>0 present_hz>0` |
| Final scanout refresh | `fbdevd` | host service contract must assert present generation causes flush/vsync accounting | `fps: fbdevd flush_hz>0` after the first frame |
| Observer closure | `selftest-client` | `cargo test -p nx --test interactive_os_startup proof_mode_selftest_is_observer_only_for_live_input -- --nocapture` | `display: bootstrap on`, `SELFTEST: ui visible input ok` only after service state is true |

Hard rules:
- A stable QEMU gate failure means a missing host/service proof unless the
  failure is a documented host/QEMU environmental dependency.
- Do not fix display output by making `selftest-client` write frames, synthesize
  service markers, or own final scanout.
- A broad `just test-os visible-bootstrap` rerun is last in the loop, after the
  failing hop has a focused host proof or a bounded diagnostic label.
