# Handoff (Current)

<!--
CONTEXT
This is the entry-point for a new chat/session.
Keep it short, factual, and proof-oriented.
Update it at the end of each task.
-->

## What was just completed
- **task**: (fill) `tasks/TASK-XXXX-*.md`
- **contracts**:
  - (fill) `docs/rfcs/RFC-XXXX-*.md`
  - (fill) `docs/adr/XXXX-*.md`
- **touched_paths**:
  - (fill) allowlist from task

## Proof (links / commands)
- **tests**:
  - (fill) `cargo test -p <crate>` (or `cargo test --workspace`)
- **os gates** (when OS code touched):
  - (fill) `just dep-gate`
  - (fill) `just diag-os`
- **qemu** (when runtime behavior changed):
  - (fill) `RUN_UNTIL_MARKER=1 just test-os` (marker / expected output)

## Current state summary (compressed)
- **why**:
  - (fill) 1-3 bullets
- **new invariants / constraints**:
  - (fill)
- **known risks**:
  - (fill)

## Next steps (drift-free)
- (fill) 3-7 bullets, each actionable

## Blockers / Open threads
- (fill) with pointers to tasks/files
