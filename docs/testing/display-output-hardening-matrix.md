<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Display Output Hardening Matrix

This matrix is the fast path for the current visible-output closure. It keeps
`selftest-client` observer-only and requires each service-owned hop to fail with
a precise label before QEMU is used as the final proof.

| Gate | Owner | Host proof | QEMU evidence |
| --- | --- | --- | --- |
| Capability routing for display | `init-lite` | `cargo test -p nx --test interactive_os_startup init_wires_fbdevd_caps_and_routes_for_service_owned_display_observer_chain -- --nocapture` | `init: fbdevd slots ...`, `init: selftest fbdevd slots ...` |
| DisplayServer-v0 IPC protocol | `input-live-protocol` + `windowd` | `cargo test -p input-live-protocol -p windowd -- --nocapture` | no malformed `OP_UPDATE_VISIBLE_STATE`; bounded observer timeout on missing replies |
| First scanout ownership | `windowd` + `fbdevd` | `cargo test -p windowd -p fbdevd -- --nocapture` | `fbdevd: ready`, `fbdevd: map ok`, `fbdevd: ramfb configured`, `fbdevd: flush ok` only after framebuffer registration reaches `windowd` |
| Live present after input | `inputd` + `windowd` | `cargo test -p inputd -p windowd -- --nocapture` | `fps: inputd ... pointer_live=1 keyboard_live=1`; `windowd` owns the resulting visible state; target highlights are transient rather than latched |
| Asset scene evidence | `windowd` + `fbdevd` | `cargo test -p systemui -p input-live-protocol -p fbdevd -- --nocapture`; `cargo test -p nexus-svg --test cursor_golden -- --nocapture` | `windowd: cursor svg loaded`, `windowd: wallpaper visible`, `windowd: text target visible`, `windowd: icon target visible`, `fbdevd: cursor overlay on` |
| Observer closure | `selftest-client` | `cargo test -p selftest-client -- --nocapture` | `SELFTEST: ui v2b assets ok` only after visible input + service-owned asset evidence |

Hard rules:
- A stable QEMU gate failure means a missing host/service proof unless the
  failure is a documented host/QEMU environmental dependency.
- Do not fix display output by making `selftest-client` write frames, synthesize
  service markers, or own final scanout.
- A broad `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
  rerun is last in the loop, after the
  failing hop has a focused host proof or a bounded diagnostic label.
- `just start` is not the automated proof. Use it for the live interactive check
  that the same DisplayServer scene shows JPEG wallpaper, the Mocu SVG cursor,
  Inter-rendered text/icon targets, real pointer movement, transient
  hover/click/key highlights, and distinguishable scroll-up/scroll-down pulses.
