# Retrospective Template (for .cursor/current_state.md)

<!--
Use this when you've learned something painful/valuable during development.
Add entries to `.cursor/current_state.md` in the relevant sections.
-->

## Known risks / hazards (add when you hit a multi-day debug)

```markdown
- **[Component]: [Short problem description]**
  - Root cause: [Why it happened]
  - Symptom: [What you saw / how long it took to find]
  - Mitigation: [What to check/do differently next time]
```

**Example:**
```markdown
- **virtio-blk DMA**: Rust ownership model conflicts with virtio shared-ring semantics
  - Root cause: virtio expects mutable aliasing of ring buffers; Rust forbids without unsafe
  - Symptom: Weeks debugging "mysterious corruption" that was actually borrow-checker workaround gone wrong
  - Mitigation: Use explicit `unsafe` + documentation for shared virtio rings; don't try to "trick" the borrow checker

- **SMP trap stack bring-up**: single-hart trap assumptions leaked into multi-hart planning
  - Root cause: trap entry path relied on global `__stack_top` semantics
  - Symptom: TASK looked "ready" but trap hardening remained an unresolved carry-over risk
  - Mitigation: record trap-stack migration as an explicit stop condition before claiming SMP baseline complete

- **QEMU smoke contention**: parallel runs produced false-negative failures
  - Root cause: multiple smoke jobs contended on shared QEMU artifacts
  - Symptom: timeout/lock errors looked like runtime regressions
  - Mitigation: enforce sequential QEMU proofs and capture command lines in handoff/current_state
```

---

## DON'T DO (session-local) (add when you've tried something that failed badly)

```markdown
- DON'T [specific action] because [reason] (leads to [consequence])
```

**Example:**
```markdown
- DON'T use Arc<Mutex<VirtQueue>> for device rings (leads to double-borrow panic at runtime)
- DON'T skip `docs/testing/index.md → Troubleshooting` (wastes hours on known issues)
- DON'T debug QEMU tests without RUN_UNTIL_MARKER=1 (you'll miss early failures)
- DON'T run multiple QEMU smoke commands in parallel (lock contention hides real signals)
- DON'T close RED decision points "later" in SMP tasks (resolve them in the task contract first)
```

---

## Open threads / follow-ups (add when you defer something intentionally)

```markdown
- [Short description] — [status: blocked on X / needs tooling Y / deferred until Z]
```

**Example:**
```markdown
- Better virtio error reporting (status codes instead of "failed") — needs RFC for error ABI
- Automated marker regression check — deferred until CI pipeline exists
- SMP marker-gating helper flag in harness — deferred until TASK-0012 phase implementing qemu-test wiring
- Per-hart trap stack proof marker strategy — blocked on TASK-0012 trap.S implementation slice
```

---

## Usage
1. After painful debug session: add 1–2 lines to `current_state.md` immediately.
2. Agent will see it in next chat → avoids repeating mistakes.
3. Review quarterly: move stale entries to `docs/architecture/lessons-learned.md` or delete.
