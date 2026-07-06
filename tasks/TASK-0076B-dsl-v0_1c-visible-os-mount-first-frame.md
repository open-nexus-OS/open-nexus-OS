---
title: TASK-0076B DSL v0.1c (OS-gated): visible in-compositor mount + first DSL frame + execd isolation probe
status: Draft
owner: @ui @runtime
created: 2026-03-28
updated: 2026-07-06
depends-on:
  - tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
follow-up-tasks:
  - tasks/TASK-0080B-systemui-dsl-bootstrap-shell-launcher-host.md
  - tasks/TASK-0080D-dsl-app-runtime-lifecycle-surface-contract.md
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Runtime contract: docs/dev/dsl/runtime.md (host #2 = in-compositor mount)
  - Shell-config SSOT feeding the mount: source/services/systemui/manifests/shells/*/shell.toml
    (dsl_root + [first_frame]), products/profiles (ADR-0035)
  - One reactive path: docs/rfcs/RFC-0070-ui-design-system-ssot-convergence.md
  - Visible present baseline: tasks/TASK-0055C; input baseline: tasks/TASK-0056B
  - Spawn path this task probes for Phase 6: source/services/execd (nexus-loader, as_create)
  - Testing contract: scripts/qemu-test.sh
---

## Context (updated 2026-07-06)

TASK-0076 proves the runtime host-side. This task mounts it **in the live compositor
path** — the same embedding that will later host the SystemUI shell and the login
greeter (masterplan decision: shell + greeter are DSL-authored; authority stays in
sessiond). The `.nxir` to mount is resolved from the **existing** shell-config registry:
`shell.toml` `dsl_root` names the program, `[first_frame]` gives the initial dims —
no new config mechanism.

The scene flows `LayoutNode → LayoutEngine → windowd SceneGraph → nexus-gfx`
(RFC-0070's one path); no separate DSL renderer.

**Also in this task (cheap, de-risks Phase 6 early): the execd isolation probe.**
execd's ELF-spawn path exists (nexus-loader, `as_create` Sv39, W^X) but is not the live
boot flow, and comments contradict each other about child address-space isolation. A
selftest spawns a trivial payload and proves isolation before TASK-0080D bets on it.

## Goal

1. **Visible DSL mount**: embed `nexus-dsl-runtime` in the windowd/systemui path;
   compile the proof-surface fixture app to `.nxir` at build time; resolve via
   `dsl_root`; first frame through the real interpreter path.
2. **Visible bounded interaction**: one live pointer interaction (button tap toggling
   state) visibly updates the DSL surface via the narrow-invalidation path
   (paint-only dispatch — no full re-render).
3. **execd isolation probe**: selftest spawns a minimal ELF that (a) writes a marker,
   (b) proves address-space isolation (a write to a parent-known VA has no effect in
   the parent), (c) exits and is reaped. Markers below.
4. Handoff notes for 0080B (shell/greeter mount) + 0080D (app-host spawn).

## Non-Goals

- Launcher/shell migration (0080B/C), app-host process (0080D), routes/i18n (0077),
  AOT (0079). Kernel changes (probe uses existing syscalls only).

## Constraints / invariants (hard requirements)

- No separate preview renderer — the live interpreter/runtime path only.
- Interpreter work per frame bounded by IR budgets; no per-frame heap allocation in
  the mounted path (OS bump-allocator rule).
- Existing boot markers unaffected; new markers additive + deterministic.
- No `unwrap/expect`; no godfiles (mount plumbing = own module, not woven into
  existing windowd runtime files).

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required (user boot-verify)

UART markers:

- `DSL: program loaded hash=<h>`
- `DSL: first frame presented`
- `DSL: interaction visible ok`
- `SELFTEST: dsl visible mount ok`
- `EXECD: isolation probe ok (as=isolated)` + `SELFTEST: execd spawn isolation ok`

Visual proof:

- the QEMU window shows the DSL-rendered proof page; a live pointer tap visibly
  changes it; the page reuses the shared proof-surface targets;
- boot remains 0-fault; reveal/present chain markers unchanged.

### Proof (Host) — required

- mount plumbing unit-tested against a stub `SurfaceSink`; the same fixture app renders
  identical goldens host-side and (structurally) in the OS path.

## Touched paths (allowlist)

- `userspace/dsl/runtime/` (SurfaceSink impl for the compositor path)
- `source/services/windowd/` + `source/services/systemui/` (mount module — new file(s))
- `examples/dsl/` fixture app; build wiring to compile `.nxir` into the image
- `source/apps/selftest-client/` (markers + isolation probe)
- `source/services/execd/` (probe support only — no behavior change)
- `docs/dev/dsl/{runtime,testing}.md`, `docs/dev/ui/` mount notes

## Plan (small PRs)

1. build wiring: fixture `.nxir` into the image + registry resolution (host-tested)
2. mount module + first visible frame [boot-verify]
3. live interaction + selftest markers [boot-verify]
4. execd isolation probe + docs/handoff notes [boot-verify, can ride with 3]
