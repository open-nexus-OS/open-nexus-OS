<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# UI Testing

UI testing is **host-first** with deterministic goldens. QEMU tests are bounded smoke checks.

Performance note:

- UI scenes with glass, overlays, transitions, and animation are valid bounded performance gates.
- Follow `docs/dev/ui/foundations/quality/performance-philosophy.md` when deciding what should be cached, refreshed, or degraded.

## Golden snapshots

Use headless rendering + deterministic fixtures.

```bash
# Example (tooling varies by task/lane)
nx dsl snapshot <appdir> --route / --profile desktop --locale en-US
```

Guidance:

- Prefer stable fixtures (no wallclock, no RNG).
- Keep goldens small and representative.
- Any “ok” marker must correspond to real behavior (no fake success).

## TASK-0054 host renderer snapshots

TASK-0054 uses a host-only proof floor for the narrow BGRA8888 CPU renderer:

```bash
cargo test -p ui_renderer -- --nocapture
cargo test -p ui_host_snap -- --nocapture
cargo test -p ui_host_snap reject -- --nocapture
```

The renderer proof asserts expected pixels, 64-byte stride alignment, exact buffer
length, deterministic damage behavior, and stable reject classes. The snapshot
harness compares canonical BGRA pixels against repo-owned goldens under
`tests/ui_host_snap/goldens/`; PNG files are deterministic artifacts only, and
metadata such as gamma or iCCP chunks must not affect equality.

Golden updates are disabled by default. Regeneration must be an explicit
`UPDATE_GOLDENS=1` action, and fixture paths must stay under the snapshot
fixture root. TASK-0054 does not use OS/QEMU present markers.

## TASK-0055 headless windowd present

TASK-0055 adds the first headless `windowd` surface/layer/present proof. It is
still not a visible scanout claim.

```bash
cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture
cargo test -p ui_windowd_host reject -- --nocapture
cargo test -p ui_windowd_host capnp -- --nocapture
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

The host proof asserts desired behavior: two damaged surfaces composite to exact
pixels, no damage skips present, layer ordering is deterministic, minimal
present acknowledgements are emitted only after composition, and invalid or
unauthorized VMO/surface/layer/commit requests fail closed. It also runs the
TASK-0055 Cap'n Proto schemas through generated bindings and roundtrips
surface, queue-buffer damage, scene commit, vsync subscribe, and input subscribe
messages.

The OS proof uses a small deterministic headless desktop profile
(`64x48@60Hz`) to stay within current selftest heap limits. Its markers are
supporting evidence only and are accepted by postflight only through the
canonical QEMU harness and proof-manifest verification.

VMO scope is deliberately narrow here: TASK-0055 proves `windowd` rejects
missing, forged, wrong-rights, wrong-size, or non-surface buffer handles. It does
not claim new kernel VMO capability transfer, sealing/reuse, IPC fastpath, or
zero-copy production behavior.

## TASK-0055B visible QEMU scanout bootstrap

TASK-0055B adds one deterministic visible QEMU first-frame path. The
`visible-bootstrap` profile is a proof-manifest harness/marker profile; it is
not a SystemUI/launcher start profile such as desktop, TV, mobile, or car.

```bash
cargo test -p windowd -p ui_windowd_host -- --nocapture
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

The fixed visible mode is `1280x800` ARGB8888 (`5120` byte stride). QEMU uses
`ramfb`, and the guest configures `etc/ramfb` through the `fw_cfg` MMIO device
only after `nexus-init` grants `selftest-client` the scoped
`device.mmio.fwcfg` capability. Guest success markers appear only after
`windowd` present evidence exists and the framebuffer VMO has been
written/configured. Harness acceptance is verified after the run by
`verify-uart`; it is not encoded as a guest-emitted marker.

- `display: bootstrap on`
- `display: mode 1280x800 argb8888`
- `windowd: present ok (seq=1 dmg=1)`
- `display: first scanout ok`
- `SELFTEST: display bootstrap guest ok`

This does not prove visible SystemUI/launcher profile selection, input, cursor,
multi-display, virtio-gpu, display service dirty-rect behavior, or frame-budget
performance.

## TASK-0055C visible windowd present + SystemUI first frame

TASK-0055C replaces the visible bootstrap pattern claim with a real visible
`windowd` + SystemUI first-frame path. The same `visible-bootstrap`
proof-manifest profile remains a harness/marker profile; it is not a SystemUI
start-profile or dev-preset matrix.

```bash
cargo test -p windowd -p ui_windowd_host -p systemui -- --nocapture
cargo test -p ui_windowd_host reject -- --nocapture
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

The SystemUI first-frame source is intentionally tiny but not monolithic:
`source/services/systemui/` owns a TOML-backed `desktop` profile/shell seed and
small `profile`, `shell`, and `frame` modules. Host tests prove deterministic
manifest validation, profile/shell compatibility, stable BGRA first-frame
pixels, row-copy behavior, and pre-visible marker rejection. Host evidence
stores the full `windowd`-composed 1280x800 frame. The OS/QEMU path writes
`windowd`-composed rows to `ramfb` to stay inside the current selftest heap, so
the marker ladder still proves the present lifecycle rather than a sidecar
framebuffer copy.

Expected visible marker ladder:

- `display: bootstrap on`
- `display: mode 1280x800 argb8888`
- `windowd: backend=visible`
- `windowd: present visible ok`
- `display: first scanout ok`
- `systemui: first frame visible`
- `SELFTEST: ui visible present ok`

This slice still does not prove input, cursor/focus/click, display-service
integration, dev display/profile presets, frame-budget smoothness, or
kernel/core production-grade display closure.

## TASK-0056 v2a present scheduler + input routing

TASK-0056 adds the first functional v2a real-time baseline inside the existing
`windowd` authority path. Host tests are the primary proof that scheduler/fence
and routing semantics are real; QEMU markers summarize the same evidence after a
small v2a smoke path.

```bash
cargo test -p ui_v2a_host -- --nocapture
cargo test -p ui_v2a_host reject -- --nocapture
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

Host proof requirements:

- rapid frame-indexed submits coalesce deterministically,
- minimal fences are unsignaled before the scheduler tick and signaled after it,
- no-damage/no-state-change skips present and emits no success marker,
- overlapping surfaces route pointer focus to the topmost committed layer,
- keyboard delivery targets the focused surface only,
- stale/unauthorized/oversize scheduler and input paths reject fail-closed.

Expected v2a marker ladder under `visible-bootstrap`:

- `windowd: present scheduler on`
- `windowd: input on`
- `windowd: focus -> 1`
- `launcher: click ok`
- `SELFTEST: ui v2 present ok`
- `SELFTEST: ui v2 input ok`

This slice does not prove visible cursor polish, real HID/touch device input,
click-to-frame latency budgets, WM-lite behavior, screenshot/GTK refresh
evidence, GPU/display-driver integration, or kernel/MM/zero-copy production
closure.

## TASK-0056B visible input cursor/hover/focus/click

TASK-0056B adds the first deterministic QEMU-visible input floor on top of the
v2a routing baseline. Host tests assert deterministic pixels and state
transitions; live QEMU pointer/keyboard input follows immediately in
`TASK-0252`/`TASK-0253`:

```bash
cargo test -p ui_v2a_host -- --nocapture
cargo test -p ui_v2a_host reject -- --nocapture
cargo test -p windowd -p launcher -- --nocapture
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

The host proof covers cursor pixels, focus affordance pixels, and clicked-surface
pixels in `windowd`-composed frames. Required reject coverage includes
pre-state marker rejection, out-of-bounds pointer movement, no committed hit
surface, stale and unauthorized surface references, bounded input queue
overflow, and launcher visible-click marker rejection without a visible state
change.

Expected visible-input marker ladder under `visible-bootstrap`:

- `windowd: input visible on`
- `windowd: cursor move visible`
- `windowd: hover visible`
- `windowd: focus visible`
- `launcher: click visible ok`
- `SELFTEST: ui visible input ok`

The QEMU path writes the 56B `windowd`-composed visible-input frames into the
same `ramfb` target after the visible SystemUI present baseline: cursor-start,
hover/cursor-end, then final focus/click. This is a deterministic visible
affordance floor, not a live HID/touch/keymap/IME, gesture, perf, WM-v2, or
kernel-production closure claim.
