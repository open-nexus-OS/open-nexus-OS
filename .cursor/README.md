# Cursor Workflow (Open Nexus OS)

This folder contains the *session system* used to keep tasks deterministic, drift-free, and low-token.

## What lives here

- **`current_state.md`**: single source of truth for the current system state (compressed "why", invariants, open threads).
- **`next_task_prep.md`**: preparation checklist for the *next* task (drift check + security + follow-ups).
- **`handoff/current.md`**: the live handoff used to start the next chat/session.
- **`handoff/archive/`**: optional history snapshots (one per completed task).
- **`pre_flight.md`**: end-of-task quality gate (automatic + manual checks).
- **`stop_conditions.md`**: hard "done means X" rules (prevents fake completion).
- **`context_bundles.md`**: copy/paste context bundles (`@...`) to avoid `@codebase` scans.
- **`rules/*.mdc`**: Cursor rules that enforce plan-first and quality gates by path triggers.

## Daily usage

### 1) Before starting a new task (prep)

- Update **`next_task_prep.md`**:
  - confirm the next task is drift-free vs `current_state.md`
  - confirm acceptance criteria and non-goals are explicit and testable
  - confirm security considerations are complete (including negative tests where applicable)
  - confirm the task has a **Touched paths** allowlist
- Update **`handoff/current.md`**:
  - what is done (with proof)
  - what is next (concrete steps)
  - constraints/invariants to watch

### 2) Starting a new chat/session

Provide:

- `@.cursor/handoff/current.md`
- `@.cursor/current_state.md`
- the task file `@tasks/TASK-XXXX-*.md` and linked RFC/ADR contracts

Then instruct:

- "Kontext strikt: @core_context @task_context @quality_gates @touched. Kein @codebase Scan."

The Cursor rules will push planning first (plan mode), then contract-first implementation.

### 3) During implementation

- Stay within the task's **Touched paths** allowlist.
- Implement only what the task/RFC specifies; anything extra becomes a follow-up task.
- Prefer tests for the **desired behavior** (Soll-Zustand), not implementation quirks.

### 4) End of task (wrap-up)

- Run **`pre_flight.md`** and ensure **`stop_conditions.md`** are satisfied.
- Overwrite/update:
  - `current_state.md` (compressed new truth)
  - `handoff/current.md` (proof + next steps)
  - `next_task_prep.md` (prep the next task)
- Optional: create a snapshot in `handoff/archive/` named `TASK-XXXX*.md`.
