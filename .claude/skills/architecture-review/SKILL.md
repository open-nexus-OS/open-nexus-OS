---
name: architecture-review
description: A three-lens design review (system architect · security researcher · pragmatist) to run before any non-trivial design — new service, API, syscall, wire format, protocol, or cross-cutting change — so a task ships the smallest correct change instead of turning into scope-creep with side quests. Use before writing code for anything structural.
---

# Architecture review (three-lens deliberation)

Before structural code, deliberate from three fixed lenses and emit a 3-line
verdict — then build. This is the firewall against scope-creep and parallel
patterns, the two things that make a single-vision codebase drift.

## When to run

New service / API / syscall / wire format / protocol · crossing an architecture
boundary (kernel↔userspace ABI, service↔service IPC, host↔OS gate, policy authority)
· adding a dependency, a cfg, or a new pattern. **Skip** for a localized bugfix or a
within-pattern edit — this is for design, not every line.

## The three lenses (each MUST produce its artifact)

- **System architect** — Does this fit an existing pattern/SSOT, or fork one? Name the SSOT it extends
  (`nexus-sdk-routes`, `service_topology`, the ABI). A boundary crossing needs an ADR; a new
  syscall/API/wire format needs an RFC seed first (CLAUDE.md workflow).
  → *Artifact:* the SSOT row/pattern extended, **or** the ADR/RFC number.
- **Security researcher** — What is the trust boundary? Identity = `sender_service_id` from kernel IPC,
  never a payload string. What invariant must hold, what is the attack surface, what input bounds apply?
  → *Artifact:* the invariant + the `test_reject_*` that proves it.
- **Pragmatist (scope veto)** — What is the smallest honest change? Host-first proof? Explicitly list what
  you are **not** doing this task.
  → *Artifact:* the touched-paths boundary + the parked side-quests (as follow-up tasks, not now).

## Output (before coding)

Three lines:
- **Scope** — files/paths in, and what is explicitly out.
- **Invariant** — what must hold + the negative test that proves it.
- **Contract** — the ADR / RFC / SSOT row this extends.

If the lenses disagree, the pragmatist's scope wins; everything else becomes follow-up tasks.

## Rules

- No new subsystem / ABI / cfg without the architect lens naming its ADR or RFC.
- No security-relevant surface without the security lens' `test_reject_*`.
- No task without the pragmatist's explicit "not doing" list — that list is the scope-creep firewall.
- Default to extend-SSOT / declarative / factory-mint over hardcoding (see the `code-quality` skill).
