# Next Task Preparation (Drift-Free)

## Active execution

- **task**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `In Progress`.
- **contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `In Progress`.
- **gate**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` Gate E (`Windowing, UI & Graphics`, `production-floor`).
- **completed predecessor**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `Done`.
- **archived predecessor handoff**: `.cursor/handoff/archive/TASK-0047-policy-as-code-v1-unified-engine.md`.

## TASK-0054 readiness checks

- [x] Follow-up tasks are now in the header: `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0169`, `TASK-0170`.
- [x] RFC-0046 exists and is linked from TASK-0054 plus `docs/rfcs/README.md`.
- [x] TASK-0054 and RFC-0046 are marked `In Progress`; RFC-0046 Phase 0 is active.
- [x] `.cursor/context_bundles.md`, `.cursor/pre_flight.md`, and `.cursor/stop_conditions.md` include TASK-0054-specific entries.
- [x] Current-state note matches repo reality: no existing `userspace/ui/renderer/`; `TASK-0169` / `TASK-0170` are still `Draft`.
- [x] Security section exists with threat model, invariants, and DON'T DO list.
- [x] Red flags are explicit:
  - `TASK-0169` overlap,
  - host font determinism,
  - PNG/golden determinism,
  - protected root `Cargo.toml` workspace update,
  - production-grade claim boundary.
- [x] Production gate mapping is explicit: TASK-0054 contributes only Gate E `production-floor`, not Gate A kernel/core `production-grade`.
- [x] Reject proof requirements are explicit for oversize inputs, invalid stride/dimensions, damage overflow, golden update gating, and fixture traversal.

## Plan-mode prompts for implementation

- Decide whether to execute the narrow TASK-0054 renderer floor or promote `TASK-0169` as the implementation vehicle.
- If TASK-0054 proceeds, keep API narrow: `Frame`, BGRA8888 primitives, deterministic text fixture, and bounded `Damage`.
- Call out root `Cargo.toml` before editing because it is protected.
- Treat `cargo test -p ui_host_snap` as the primary proof and avoid OS/QEMU claims.
- Follow RFC-0046 Rust discipline: checked newtypes for confusing raw quantities, `#[must_use]` validation/errors, safe ownership, no unsafe `Send`/`Sync`, and `#![forbid(unsafe_code)]` for the host renderer crate unless a later RFC explicitly permits an exception.
- If scheduler, memory management, IPC, VMO, or timer behavior is discovered to be too simplistic for a real UI floor, stop and report the gap; route it to `TASK-0054B` / `TASK-0054C` / `TASK-0054D`, `TASK-0288`, `TASK-0290`, or a new RFC/task.

## Carry-forward guardrails

- No kernel, compositor, GPU, input-routing, or OS present marker work in TASK-0054.
- No host font discovery or locale-dependent fallback.
- No golden rewriting unless explicitly gated by `UPDATE_GOLDENS=1`.
- No success marker for placeholder behavior.
- No weakening of RFC-0046 proof requirements to fit an easy implementation.
