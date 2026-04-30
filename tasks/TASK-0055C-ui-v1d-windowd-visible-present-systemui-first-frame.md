---
title: TASK-0055C UI v1d: windowd visible present + SystemUI first frame in QEMU
status: Done
owner: @ui
created: 2026-03-28
depends-on:
  - TASK-0055
  - TASK-0055B
follow-up-tasks:
  - TASK-0055D
  - TASK-0056
  - TASK-0056B
  - TASK-0056C
  - TASK-0251
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC seed contract: docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md
  - Visible scanout bootstrap: tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md
  - UI v1b compositor baseline: tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - Renderer wiring: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Once the system can show a guest-visible framebuffer, the next missing step is to make the **real `windowd` output**
land on that surface. This task converts the invisible/headless present path into the first visible UI frame.

It is the bridge from:

- "display exists, but only shows a pattern"

to

- "`windowd`/SystemUI draw something real and visible."

## Goal

Deliver:

1. Visible `windowd` present path:
   - the same frame built by `windowd` for headless present is written to the visible display target
   - full-frame present is acceptable in v1d; dirty-rect optimization is a follow-up
2. Minimal SystemUI visible shell frame:
   - draw a deterministic desktop background and a minimal shell surface
   - seed the TOML-backed SystemUI profile/shell pipeline with a minimal `desktop` profile and shell
   - no launcher interaction required yet; this task only proves that the shell frame is visible
3. Marker and visual parity:
   - the visible present reuses the same present lifecycle as the headless path
   - UART markers remain stable and bounded

## Non-Goals

- Rich shell UI.
- Cursor or pointer rendering.
- Input routing.
- Window management beyond a minimal first frame.
- Quick Settings, Notifications, or app launching.

## Constraints / invariants (hard requirements)

- No parallel "debug renderer"; use the same `windowd` composition path.
- The visible path must not bypass `windowd` scene composition; renderer-abstraction backend wiring remains `TASK-0170` scope.
- Markers must correspond to real visible present, not just a headless compose.
- Deterministic frame contents and deterministic shell background.
- SystemUI profile/shell selection starts from small TOML manifests, but the dev preset/start-profile matrix remains follow-up scope.

## Security / authority invariants

- `windowd` remains the single present authority; no parallel compositor or ad-hoc scanout path.
- Marker honesty is mandatory: no success marker before visible present preconditions are satisfied.
- MMIO/display authority stays policy-gated and bounded by prior contracts (`TASK-0010`, `TASK-0055B`).
- Fail closed on invalid visible mode/capability/present sequencing and emit stable reject classes.
- No sensitive data or framebuffer dumps in logs/markers; only bounded metadata (mode, sequence, profile).

## Red flags / decision points

- **Authority drift risk:** introducing a second "visible-only" rendering path would invalidate 55B/55 carry-in assumptions.
- **Fake-visible risk:** marker emission without real visible present evidence would produce false closure.
- **Profile drift risk:** mixing harness profile semantics with future launcher/SystemUI start profiles.
- **SystemUI monolith risk:** adding first-frame logic as one flat helper would make later profile/shell growth brittle.
- **Scope drift risk:** absorbing cursor/input/perf/kernel-grade work that belongs to `TASK-0056*`/`TASK-0251`/kernel lanes.

Red-flag mitigation now:

- Route visible frame bytes through the existing `windowd` lifecycle; do not add sidecar paths.
- Feed QEMU `ramfb` from the `windowd`-composed 1280x800 frame, not from a raw SystemUI source surface.
- Gate visible markers on real present state transitions and keep reject tests explicit (`test_reject_*`).
- Keep profile boundaries explicit in docs/tests (`visible-bootstrap` remains harness/marker profile).
- Keep SystemUI split into small profile/shell/frame modules and seed only the minimal TOML pipeline needed for this slice.
- Enforce strict non-goals for input/cursor/perf/kernel claims in this slice.

## Gate E quality mapping (TRACK alignment)

`TASK-0055C` contributes to Gate E (`Windowing, UI & Graphics`, `production-floor`) in
`tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` by proving a real visible
`windowd` + SystemUI first-frame path.

- **first-frame/present:** visible present works end-to-end with deterministic markers.
- **surface ownership/reuse:** still follows `windowd` ownership boundaries from `TASK-0055`/`TASK-0055B`.
- **input paths:** out of scope here, remains `TASK-0056B`.
- **perf closure:** out of scope here; no production-grade or smoothness claim without dedicated budgets/scenes.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p windowd -p ui_windowd_host -p systemui -- --nocapture`
- `cargo test -p ui_windowd_host reject -- --nocapture`

### Proof (OS/QEMU) — required

UART markers:

- `windowd: backend=visible`
- `windowd: present visible ok`
- `systemui: first frame visible`
- `SELFTEST: ui visible present ok`

### Current implementation evidence (2026-04-30)

Green so far:

- `cargo test -p systemui -- --nocapture`
- `cargo test -p windowd -p ui_windowd_host -- --nocapture`
- `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite`
- `cargo test -p selftest-client -- --nocapture`
- `cargo test -p windowd -p ui_windowd_host -p systemui -- --nocapture`
- `cargo test -p ui_windowd_host reject -- --nocapture`
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
- `scripts/fmt-clippy-deny.sh`
- `just test-all`
- `just ci-network`
- `make clean`
- `make build`
- `make test`
- `make run`

Closure delta after critical review:

- `windowd::present_bootstrap_scanout_tick()` now composes and stores the visible frame before success evidence.
- Host evidence stores the full `windowd`-composed 1280x800 frame; the OS path writes `windowd`-composed rows to QEMU `ramfb` to avoid a 4 MiB selftest heap allocation.
- `selftest-client` asks `windowd` for composed rows and writes those rows to QEMU `ramfb`; it does not own a parallel framebuffer composer.
- Host tests assert composed-frame dimensions, stride, representative SystemUI pixels, and row-copy behavior before accepting 55C marker evidence.

Observed visible marker ladder:

- `display: bootstrap on`
- `display: mode 1280x800 argb8888`
- `windowd: backend=visible`
- `windowd: present visible ok`
- `display: first scanout ok`
- `systemui: first frame visible`
- `SELFTEST: ui visible present ok`

Closure gates now green:

- `just test-all`
- `just ci-network`
- `make clean`, `make build`, `make test`, `make run` (in order)

Quality gates (must be green for closure):

- `scripts/fmt-clippy-deny.sh`
- `just test-all`
- `just ci-network`
- `make clean`, `make build`, `make test`, `make run` (in order)
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`

Visual proof:

- QEMU graphics window shows a deterministic shell/background frame sourced from the composed `windowd` frame.

## Touched paths (allowlist)

- `source/services/windowd/`
- `source/services/systemui/`
- `source/apps/selftest-client/proof-manifest/`
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `tests/ui_windowd_host/` (approved narrow addition for required reject proof)
- `docs/dev/ui/overview.md`
- `docs/dev/ui/foundations/quality/testing.md`
- `docs/testing/index.md`
- `docs/architecture/README.md`
- `docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`
- `tasks/STATUS-BOARD.md`

## Plan (small PRs)

1. `windowd` visible backend handoff
2. minimal SystemUI first frame
3. markers + selftests + docs
