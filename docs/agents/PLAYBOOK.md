# Cursor Agent Playbook (Open Nexus OS)

This playbook keeps agent sessions focused, low-token, and production-minded.

## Principles

- Work in scoped tasks (see `tasks/` files). Do not roam repo-wide.
- Preserve invariants: UART markers, Host path byte-compatibility, `os-lite` feature gating.
- Do not introduce cross-layer dependencies. Kernel ↔ Services boundaries are strict.
- Prefer edits over refactors. If refactor spans modules, require an ADR.
- Keep every touched file’s CONTEXT header in sync with `docs/standards/DOCUMENTATION_STANDARDS.md`. Adjust fields (CONTEXT/OWNERS/STATUS/API_STABILITY/TEST_COVERAGE/ADR) instead of deleting them.
- Implementations must be **real**: avoid “fake success” logs/markers and avoid returning “Ok” from stub paths unless explicitly documented as a stub.
- Logging/markers must be **unified + deterministic**: prefer the shared logging facade (`nexus-log` / `log_*` macros) and centralized marker helpers; avoid new ad-hoc UART prints except in panic/trap floor paths.
- Default vision lens: see `docs/agents/VISION.md`. Use it when evaluating tradeoffs and suggest improvements aligned with the vision.

## Proof of implementation (default)

Unless the user explicitly asks for a design-only response, every “please implement X” task MUST include:

- A **code change** that actually wires the behavior (not just logging).
- At least one **proof artifact**:
  - a test (`cargo test -p …`), or
  - a QEMU marker run (`RUN_UNTIL_MARKER=1 …`), or
  - an ABI/contract test demonstrating the behavior is enforced.

If proof cannot be produced (e.g. missing tooling), the agent MUST state the blocker explicitly.

## Stubs and placeholders (default policy)

Stubs are allowed during bring-up, but MUST be obvious and non-deceptive:

- **Label clearly**: return `Unsupported`/`Placeholder` errors or emit a marker containing `stub`/`placeholder`.
- **Never fake success**: do not emit “ready/ok” markers or “success” logs for unimplemented behavior.
- **Document**: update ADR/RFC “Current state” notes when a subsystem is intentionally stubbed.

## Session Checklist

- Read the current `tasks/TASK-*.md` brief. Only touch listed files.
- Use semantic search inside the declared directories before expanding scope.
- After each edit, run lints/tests specified in the task brief.
- Stop when the task's stop conditions are satisfied.

## Subsystem Briefs

### Init (`source/init/nexus-init`)

- Two backends: `std_server` (Host) and `os_lite` (feature-gated). Keep host path byte-compatible.
- Entry: `src/lib.rs` selects backend; `src/os_lite.rs` wires cooperative bootstrap.
- Invariants: preserve UART markers `init: start`, `init: up <svc>`, `init: ready`.

### Spawner (`source/services/execd`)

- Single spawner for services/tasks. Uses lite IPC where applicable.
- Delegates to kernel `exec` loader; no userspace ELF mapping remains.

### Kernel exec path

- `exec` syscall owns ELF parse/map/guard/W^X; treat it as the single loader.
- `source/kernel/neuron/src/user_loader.rs` stays thin; no business logic duplication.

### Deprecated target (`source/apps/init-lite`)

- Marked deprecated; keep as wrapper or pointer to `nexus-init`.

### Header & ADR discipline

- Before editing any module, verify it carries the standard header block defined in `docs/standards/DOCUMENTATION_STANDARDS.md`.
- If the header is missing or outdated, fix it in the same prompt. Missing headers block review.
- Reference a relevant ADR (or add one) whenever architectural boundaries change.

## Don't Touch List (without ADR)

- Kernel memory manager internals
- Syscall ABI surface
- UART marker contract and self-test probes

## Testing

- Host: `cargo test --workspace`
- OS-lite: `just test-os`

## Links

- ADR-0001 (Runtime Roles & Boundaries): `docs/adr/0001-runtime-roles-and-boundaries.md`
- Task index: `tasks/`
- Agent vision: `docs/agents/VISION.md`
