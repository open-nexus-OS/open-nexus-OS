# Current Handoff: TASK-0054 in progress

**Date**: 2026-04-27  
**Active execution task**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `In Progress`  
**Active contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `In Progress`  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  
**Archived predecessor handoff**: `.cursor/handoff/archive/TASK-0047-policy-as-code-v1-unified-engine.md`

## Prep summary

- TASK-0047 handoff was archived before moving focus.
- RFC-0046 was created as the TASK-0054 contract seed and linked from the task/RFC index.
- TASK-0054 and RFC-0046 are now `In Progress`; Phase 0 renderer core is the active RFC phase.
- TASK-0054 header now carries explicit follow-ups: `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0169`, `TASK-0170`.
- TASK-0054 now states current repo reality: `userspace/ui/renderer/` does not exist; `TASK-0169` / `TASK-0170` remain `Draft`.
- TASK-0054 is explicitly bounded as host-first and kernel-free. It must not claim Gate A kernel/core `production-grade` closure.
- Production-grade kernel/UI gaps remain delegated to `TASK-0054B` / `TASK-0054C` / `TASK-0054D`, then `TASK-0288` / `TASK-0290`.
- RFC-0046 raises local TASK-0054 implementation quality to production-grade where needed: checked bounds, newtypes, `#[must_use]`, explicit ownership, no unsafe `Send`/`Sync`, safe Rust, and behavior-first reject proofs.
- Build target hygiene carry-in is current: `justfile`, `Makefile`, and `scripts/fmt-clippy-deny.sh` keep Cargo artifacts in repo-local `target/` unless `NEXUS_CARGO_TARGET_DIR` overrides.
- `context_bundles`, `pre_flight`, and `stop_conditions` now carry TASK-0054-specific entries.

## TASK-0054 decisions to resolve before implementation

- Decide whether TASK-0054 proceeds as the narrow `Frame` / primitive / `Damage` host proof floor or whether `TASK-0169` should be promoted as the actual implementation vehicle.
- Root `Cargo.toml` must be touched for new workspace crates/tests; it is protected by `.cursorrules`, so call this out explicitly in the implementation plan before editing.
- Text proof must avoid host font discovery. Use repo-owned deterministic font fixture or tiny deterministic test font.
- PNG/golden comparison must ignore metadata/gamma/iCCP and compare deterministic decoded pixels or raw buffers.
- If implementation uncovers simplistic scheduler, MM, IPC, VMO, or timer assumptions, report it immediately and route it to the owning follow-up; do not solve it inside TASK-0054 by adding kernel scope.

## Proof target

- Primary host proof remains `cargo test -p ui_host_snap`.
- Required proof includes positive render/damage/snapshot cases plus reject tests for oversize inputs, invalid stride/dimensions, damage overflow, golden update gating, and fixture path traversal.

## Carry-forward guardrails

- No kernel, compositor, GPU, input-routing, or OS present marker work in TASK-0054.
- No fake `ok`/`ready` marker for host-only behavior.
- No direct MMIO/GPU/device authority in the renderer.
- No parallel long-term renderer architecture if `TASK-0169` is selected.
- No weakening of RFC-0046 bounds/proof requirements to make the first renderer easier.
