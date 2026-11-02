# Milestones

One file per fast lane (RFC-0115): its goal, its sections and its steps in the order they are
built, each step with its tasks and gate. The order of the steps is the plan; a task belongs to a
milestone only through a step's `tasks`. Decisions that hold for the whole lane go in the body.

```bash
nx ledger show hardware-network     # the steps with ✓ ▶ · and their tasks' states
nx ledger table                     # the counted progress table of the active milestone
nx ledger step hardware-network L8.1 --state active --tasks TASK-0349
```
