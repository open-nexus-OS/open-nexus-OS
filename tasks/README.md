# Tasks: Execution Truth + “100% Before Next” Workflow

This folder is the **execution truth** for the repo:

- **Tasks (`tasks/TASK-*.md`) are authoritative** for scope, stop conditions (DoD), and proof commands.
- **RFCs are design seeds/contracts**, not backlogs. They must link to tasks for execution and proof.
- **Implementation order is tracked separately**: see `tasks/IMPLEMENTATION-ORDER.md` (dynamic ordering; tasks stay authoritative).
- **Status/board view is tracked separately**: see `tasks/STATUS-BOARD.md` (Draft/In Progress/Done + blockers).

## Core strategy

### 1) “One task at a time, 100% complete”

We intentionally work **task-by-task**:

- A task is only “Done” when its **Stop conditions** are met and its **Proof** is green.
- We avoid starting the next task while the current one is incomplete.

This is how we keep progress measurable, reviewable, and handoff-friendly.

### 2) No fake green

- Never add markers like `*: ready` / `SELFTEST: * ok` unless the behavior actually happened.
- Postflight scripts are not proof unless they only delegate to the canonical harness/tests.
- If something is still stubbed, it must say **stub/placeholder/unsupported** (deterministically).

### 3) Proof is part of the task

Each task must include:

- **Proof command(s)** (e.g. `cargo test ...`, `./scripts/qemu-test.sh`),
- **Expected outputs/markers** (when proof is marker-based),
- **What “green” means** (e.g. real tests executed; no “0 tests” success).

## If “100%” isn’t possible (missing prerequisites)

When a task can’t reach 100% because something is missing, do **not** silently implement “later-task architecture”
without updating documents. Pick one of these explicit moves:

### Option A (preferred): Split the task

If the current task is too big or mixes independent concerns:

- Split it into smaller tasks with their own DoD/proofs, or
- Extract a prerequisite slice into its own task.

Then mark the original task’s plan/links to reflect the split.  
**Naming convention for follow-ups:** append a suffix to the parent task ID, e.g. `TASK-0003B-<suffix>.md` for the first follow-up to `TASK-0003`. Update RFCs/tasks to point to the suffixed follow-up to avoid ID collisions.

### Option B: Pull the minimal prerequisite forward (document extraction)

If a “later-task feature” is a **hard prerequisite** for the current task’s Stop conditions:

- Pull the minimal slice forward into the current task (keep it minimal),
- Update the later task to say “**extracted into TASK-XXXX**” to avoid duplicate authority.

This prevents “shadow prerequisites” and keeps the 100% rule feasible.

### Option C: Reduce the current task’s Stop conditions (only if the milestone meaning stays intact)

Sometimes the task definition is too ambitious for the step:

- Narrow the Stop conditions so they match the intended milestone,
- Move removed requirements into a new task with explicit proof.

Never reduce DoD just to get a green checkmark; the milestone must still be meaningful.

## Anti-drift rules (practical)

- **Track docs are not tasks**: `tasks/TRACK-*.md` describe direction, gates, and contracts; they must not become hidden DoD.
- **If you add a dependency, add the gate**:
  - update the current task’s “Gates/Blocked-by” section,
  - update the proof section (new required marker/test),
  - update RFC status/checklists if they reference the milestone.
- **If you introduce ownership boundaries (drivers/services)**, make it explicit:
  - who owns the device/MMIO,
  - who exports which contract,
  - which IPC boundary is used,
  - and how it is proven.

## Suggested task template checklist (minimum)

Every task should have:

- **Context + goal + non-goals**
- **Constraints/invariants**
- **Stop conditions (DoD)** with concrete proofs
- **Touched paths allowlist**
- **Notes about gates** (e.g. “gated on TASK-0010”)
- **Evidence snippet** section for PRs (commands + expected markers)
