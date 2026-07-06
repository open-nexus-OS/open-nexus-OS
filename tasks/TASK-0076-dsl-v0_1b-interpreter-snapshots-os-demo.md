---
title: TASK-0076 DSL v0.1b: interpreter runtime (mount/dispatch/dirty-index/retained tree) + headless goldens + conformance corpus (host)
status: Draft
owner: @ui @runtime
created: 2025-12-23
updated: 2026-07-06
depends-on:
  - tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
follow-up-tasks:
  - tasks/TASK-0076B-dsl-v0_1c-visible-os-mount-first-frame.md
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Language reference: docs/dev/dsl/{state,ir,testing}.md
  - Widget consumer set: userspace/ui/widgets/* (TASK-0073 kit; pure LayoutNode builders)
  - Layout engine: userspace/ui/layout (nexus-layout), types: userspace/ui/layout-types
  - Golden harness to reuse: tests/ui_v10_goldens/ (BGRA painter + a11y lints)
  - Testing contract: scripts/qemu-test.sh
---

## Context (updated 2026-07-06)

v0.1a gives us canonical `.nxir`. This task builds the **interpreter runtime** — the
single semantics carrier that later runs identically in three hosts (host harness,
in-compositor mount for shell/greeter, app-host process; see `docs/dev/dsl/runtime.md`).
This task is **host-only**; the visible OS mount is TASK-0076B.

Consumes what exists: `LayoutNode`/`LayoutEngine` (RFC-0057), the TASK-0073 widget kit
(32 pure-builder crates), theme tokens generated from `.nxtheme.toml`, and the
`tests/ui_v10_goldens` BGRA painter + a11y lints.

**Fixed decisions:** retained instance tree keyed by persisted stable NodeIds; narrow
invalidation via IR field classes (paint-only must not re-layout); arenas at mount +
**zero heap allocation in steady state**; IO only via injected traits.

## Goal

1. `userspace/dsl/runtime` (`nexus-dsl-runtime`, no_std+alloc), modules pinned:
   `mount.rs`, `store.rs`, `reduce.rs` (expression-tree eval), `effects.rs`,
   `deps.rs` (dependency index store.field → binding sites), `instance.rs` (retained
   tree), `diff.rs` (keyed ForEach), `emit/` (view → LayoutNode per node kind),
   `registry/` (widget registry, generated from the same SSOT the frontend validates
   against), `env.rs` (device.*), `nav.rs`/`i18n.rs` (stubs, filled by TASK-0077).
   IO via traits: `EffectHost`, `SurfaceSink`, `Clock`, `LocaleSource`, `DeviceEnv`.
2. **Update path**: `dispatch(event)` → reducer eval (scratch arena) → changed-field
   diff → class-partitioned dirty sets → paint-only patches `VisualStyle` in place;
   layout re-emits the smallest enclosing subtree by NodeId + relayout; semantics
   updates a11y only. Keyed ForEach insert/remove/move preserving instance state.
3. **Two-way bindings**: TextField/Toggle/Slider minimal (`$state.field` write-back
   as dispatched built-in events — no second mutation path).
4. `nx dsl run` / `nx dsl snapshot`: mount + render headless via the golden painter
   into `tests/dsl_goldens/` (BGRA hex + PNG debug out), device-env fixtures
   (profile/sizeClass/dpi variants).
5. **Example apps** `examples/dsl/{counter,todo}`: exercise every v0.1 construct;
   todo maps onto the shared proof-surface targets (text, icon, keyed list, controls).
6. **Conformance corpus** (`tests/dsl_conformance/`): `(state, event) → state'`
   fixtures executed by the interpreter — later re-executed byte-identically by AOT
   (TASK-0079 parity gate). Started here, grown every phase.

## Non-Goals

- OS/QEMU anything (TASK-0076B). Routes/i18n/device-env semantics (TASK-0077).
  Service effects against real IPC (TASK-0078). Kernel changes.

## Constraints / invariants (hard requirements)

- Deterministic rendering (stable rounding/ordering); bounded per-frame work (node/
  event-queue caps from IR budgets).
- **Zero-alloc steady state**: after warmup, a paint-only dispatch performs no heap
  allocation (counting-allocator test) — the bump-allocator-never-frees OS rule
  starts here, not in Phase 6.
- `nexus-dsl-runtime` + the widget-registry closure build for
  `riscv64imac-unknown-none-elf` (fix any std leakage in widget crates — small,
  mechanical).
- No `unwrap/expect`; no godfiles; no company/product names.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_1b_host/` + `tests/dsl_goldens/` + `tests/dsl_conformance/`:

- interpreter goldens match for counter/todo across two device-env fixtures;
- two-way binding fixture: simulated TextField input updates state + deterministic
  re-render;
- **dirty-class proofs**: a paint-only event does not re-run layout (instrumented
  LayoutEngine call count); a semantics-only event repaints nothing;
- **zero-alloc proof**: counting allocator reports 0 allocations for a steady-state
  paint-only dispatch after warmup;
- ForEach: keyed reorder preserves instance-local state; insert/remove stable;
- conformance corpus green under the interpreter;
- riscv build-check of runtime + registry closure.

### Docs — required (reference grade)

- `docs/dev/dsl/state.md` + `testing.md` reflect shipped behavior;
  `docs/dev/dsl/runtime.md` gains the host-harness section;
- example apps documented as the canonical "shape of a program".

## Touched paths (allowlist)

- `userspace/dsl/runtime/` (new), `userspace/dsl/cli/` (run/snapshot verbs)
- `examples/dsl/{counter,todo}/` (new)
- `tests/dsl_v0_1b_host/`, `tests/dsl_goldens/`, `tests/dsl_conformance/` (new)
- `userspace/ui/widgets/*` (no_std fixes only, if needed)
- `docs/dev/dsl/{state,testing,runtime}.md`

## Plan (small PRs)

1. mount + store + reduce eval + deps index (host tests per module)
2. instance tree + emit/ + registry (goldens for static pages)
3. dispatch/dirty path + diff.rs + dirty-class + zero-alloc proofs
4. bindings + run/snapshot verbs + example apps + conformance corpus + docs
