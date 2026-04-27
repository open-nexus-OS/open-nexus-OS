# Next Task Preparation (Drift-Free)

## Completed execution

- **task**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `Done`.
- **contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `Done`.
- **gate**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` Gate E (`Windowing, UI & Graphics`, `production-floor`).
- **completed predecessor**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `Done`.
- **archived predecessor handoff**: `.cursor/handoff/archive/TASK-0047-policy-as-code-v1-unified-engine.md`.

## TASK-0054 closure checks

- [x] Follow-up tasks are now in the header: `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0169`, `TASK-0170`.
- [x] RFC-0046 exists and is linked from TASK-0054 plus `docs/rfcs/README.md`.
- [x] TASK-0054 is marked `Done`; RFC-0046 is marked `Done`.
- [x] `.cursor/context_bundles.md`, `.cursor/pre_flight.md`, and `.cursor/stop_conditions.md` include TASK-0054-specific entries.
- [x] Current-state note matches repo reality: `userspace/ui/renderer/` exists as the narrow host proof floor; `TASK-0169` / `TASK-0170` remain successor scope.
- [x] Security section exists with threat model, invariants, and DON'T DO list.
- [x] Red flags are explicit:
  - `TASK-0169` overlap,
  - host font determinism,
  - PNG/golden determinism,
  - protected root `Cargo.toml` workspace update,
  - production-grade claim boundary.
- [x] Production gate mapping is explicit: TASK-0054 contributes only Gate E `production-floor`, not Gate A kernel/core `production-grade`.
- [x] Reject proof requirements are explicit for oversize inputs, invalid stride/dimensions, damage overflow, golden update gating, and fixture traversal.
- [x] Proof floor is green:
  - `cargo test -p ui_renderer -- --nocapture`
  - `cargo test -p ui_host_snap -- --nocapture`
  - `cargo test -p ui_host_snap reject -- --nocapture`
  - `just diag-host`
  - `just test-all`
  - `just ci-network`
  - `scripts/fmt-clippy-deny.sh`
  - `make clean`, `make build`, `make test`, `make run`

## Next task prep prompt

- Queue head is `TASK-0055` prep, but the next session must read that task/RFC context before implementation.
- Carry forward TASK-0054 honesty: host renderer goldens prove deterministic pixels/damage only, not OS present, compositor, input, GPU, or kernel/core production-grade behavior.
- If `TASK-0055` or follow-ups need scheduler/MM/IPC/VMO/timer fixes, route to `TASK-0054B` / `TASK-0054C` / `TASK-0054D`, `TASK-0288`, `TASK-0290`, or a new RFC/task rather than retrofitting TASK-0054.

## Carry-forward guardrails

- No kernel, compositor, GPU, input-routing, or OS present marker work in TASK-0054.
- No host font discovery or locale-dependent fallback.
- No golden rewriting unless explicitly gated by `UPDATE_GOLDENS=1`.
- No success marker for placeholder behavior.
- No weakening of RFC-0046 proof requirements to fit an easy implementation.
