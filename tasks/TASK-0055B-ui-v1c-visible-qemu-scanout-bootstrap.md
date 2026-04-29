---
title: TASK-0055B UI v1c: visible QEMU scanout bootstrap (ramfb window + first visible frame)
status: Done
owner: @ui @runtime
created: 2026-03-28
depends-on:
  - TASK-0055
  - TASK-0010
follow-up-tasks:
  - TASK-0055C
  - TASK-0251
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC seed contract: docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md
  - UI v1b headless compositor baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Renderer abstraction OS wiring: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Display host core: tasks/TASK-0250-display-v1_0a-host-simplefb-compositor-backend-deterministic.md
  - Display OS integration follow-up: tasks/TASK-0251-display-v1_0b-os-fbdevd-windowd-integration-cursor-selftests.md
  - Device MMIO access model: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0055` gives us a headless `windowd` present path with deterministic markers, but no visible display.
That is sufficient for early CI bring-up, yet it is too abstract for UI and app iteration.

We need the earliest possible **real guest-visible scanout** in QEMU so that later Launcher/SystemUI/DSL work can
be seen in an actual window, without waiting for the full Display v1.0 task family.

This task is intentionally a **bootstrap slice**:

- one fixed visible display path,
- one fixed resolution and pixel format,
- one deterministic QEMU graphics window,
- and no second compositor or temporary host-side mirror path.

Current-state check (2026-04-29 prep):

- `TASK-0055` is `Done` with green host + OS proof floor (`just test-all`, `just ci-network`,
  `scripts/fmt-clippy-deny.sh`, `make clean/build/test/run`).
- Headless marker ladder is already proven (`windowd: present ok`, `launcher: first frame ok`, `SELFTEST: ui launcher present ok`),
  but this does not yet prove guest-visible scanout.
- `userspace/apps/launcher` is canonical and already wired into marker/evidence flow; visible scanout should reuse this path.
- Gate E mapping in `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` remains `production-floor`: first visible frame must be real,
  deterministic, and measured honestly without claiming input/perf closure.

## Goal

Delivered:

1. Minimal visible scanout path for QEMU `virt`:
   - `NEXUS_DISPLAY_BOOTSTRAP=1` replaces pure `-nographic` bring-up for this bootstrap path with `-display gtk -device ramfb`
   - a contiguous VMO backs the QEMU `ramfb` framebuffer and is configured through `fw_cfg` DMA
   - fixed guest-visible mode is `1280x800`, stride `5120`, pixel format `argb8888`/BGRA bytes
2. Bootstrap display authority:
   - `windowd` owns the fixed bootstrap mode, present evidence, pixel pattern, and marker gating
   - `selftest-client` only writes/configures the visible VMO after `windowd` present evidence exists
3. Proof of visible output:
   - a deterministic test pattern or splash frame appears in the QEMU graphics window
   - UART markers remain available for CI and bounded selftests
4. Clear handoff boundary:
   - this task unlocks visible OS bring-up for `windowd`, SystemUI, and DSL
   - richer display features (cursor, dirty rects, settings, CLI) remain for follow-up tasks

## Non-Goals

- Full Display v1.0 (`TASK-0250`/`TASK-0251`).
- Cursor support.
- Input routing.
- Multi-display or hotplug.
- GPU acceleration or virtio-gpu.

## Constraints / invariants (hard requirements)

- No second renderer or second display stack.
- The visible scanout path must sit behind the same `windowd`/renderer contracts that later tasks use.
- Deterministic guest-visible output for a fixed build and fixed boot path.
- No fake success: visible-frame markers only after a real frame is written to the visible buffer.
- Keep the bootstrap surface small and fixed; avoid feature creep.

## Security / authority invariants

- `windowd` remains the authority for surface/layer/present sequencing; no parallel display authority is introduced.
- MMIO/display capability routing must stay under `TASK-0010` capability policy boundaries; no ambient MMIO access shortcuts.
- Marker honesty is mandatory:
  - `display: first scanout ok` is emitted only after a real guest-side framebuffer write and `ramfb` configuration succeed,
  - `SELFTEST: display bootstrap guest ok` summarizes guest-side bootstrap completion only,
  - `verify-uart` acceptance is post-run harness evidence, not a guest-emitted fact.
- Reject-path proofs must fail closed for unsupported mode/stride/format, invalid display capability handoff, and pre-scanout marker attempts.
- Logs/markers may include bounded mode metadata only (resolution, format, sequence), never raw framebuffer dumps.

## Red flags / decision points

- **Second-stack drift risk:** bootstrap code must reuse `windowd` + existing renderer path, not create a sidecar compositor.
- **Fake-visible-marker risk:** screenshot/manual visual checks are supportive only; canonical closure comes from deterministic UART marker + harness proof.
- **Profile drift risk:** scanout mode must stay fixed for this task (single deterministic mode). The `visible-bootstrap`
  proof-manifest profile is a harness/marker profile, not a future SystemUI/launcher start profile such as desktop/TV/mobile/car.
- **Kernel/perf overclaim risk:** this task must not claim input routing, cursor, latency budgets, or kernel production-grade display closure.

Red-flag mitigation now:

- Keep scope to one fixed visible mode and one deterministic marker ladder.
- Require both host-side harness verification and guest-visible output confirmation before claiming done.
- Route any required kernel/MM/IPC/perf uplift to owning follow-ups (`TASK-0054B/C/D`, `TASK-0288`, `TASK-0290`).

## Gate E quality mapping (TRACK alignment)

`TASK-0055B` contributes to Gate E (`Windowing, UI & Graphics`, `production-floor`) by closing the first **visible**
QEMU scanout bootstrap on top of the already-proven headless present path.

- **first-frame/present:** this task must prove visible first scanout with deterministic markers.
- **surface ownership/reuse:** this task reuses `windowd` ownership rules from `TASK-0055`; no new ownership model.
- **input paths:** still out of scope here; remains follow-up (`TASK-0056B`).
- **perf claims:** only bounded deterministic behavior is allowed; no latency/smoothness claim without dedicated perf evidence.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required

UART markers:

- `display: bootstrap on`
- `display: mode 1280x800 argb8888`
- `display: first scanout ok`
- `SELFTEST: display bootstrap guest ok`

Quality gates (must be green for closure):

- `scripts/fmt-clippy-deny.sh`
- `just test-all`
- `just ci-network`
- `make clean`, `make build`, `make test`, `make run` (in order)
- Visible bootstrap proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`

Visual proof:

- QEMU opens a graphics window
- a deterministic bootstrap pattern or splash frame is visible without manual guest interaction

## Touched paths (allowlist)

- QEMU runner/harness configuration for graphics-capable UI boot
- QEMU `ramfb`/`fw_cfg` bootstrap mode
- `nexus-init` capability distribution and `policies/base.toml` for the `device.mmio.fwcfg` grant
- `source/services/windowd/` (only as needed to target the visible buffer)
- `source/apps/selftest-client/`
- `source/apps/init-lite/` / `source/init/nexus-init/` as needed for scoped capability handoff
- `source/apps/selftest-client/proof-manifest/`
- `docs/dev/ui/foundations/quality/testing.md`

## Plan (small PRs)

1. QEMU graphics-capable boot mode + deterministic harness plumbing
2. bootstrap scanout authority + visible test pattern marker
3. docs + selftests + handoff notes to `TASK-0055C` and `TASK-0251`

## Closure proof (2026-04-29)

Closure-hardening update completed: marker semantics and scanout source corrections were re-proven with full closure gates green.

- `cargo test -p windowd -p ui_windowd_host -- --nocapture` — green.
- `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite` — green.
- `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo check -p init-lite --target riscv64imac-unknown-none-elf --release` — green.
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` — green; `verify-uart` accepted profile `visible-bootstrap`.

Visible marker ladder observed:

- `display: bootstrap on`
- `display: mode 1280x800 argb8888`
- `windowd: present ok (seq=1 dmg=1)`
- `display: first scanout ok`
- `SELFTEST: display bootstrap guest ok`

Claim boundary:

- This closes one deterministic QEMU `ramfb` first-frame bootstrap path.
- This does not close visible SystemUI/launcher profile selection, real input, cursor, multi-display, virtio-gpu, dirty-rect display service behavior, latency/perf budgets, or kernel/core production-grade display closure.
