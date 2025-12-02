# Cursor Agent Playbook (Open Nexus OS)

This playbook keeps agent sessions focused, low-token, and production-minded.

## Principles
- Work in scoped tasks (see `tasks/` files). Do not roam repo-wide.
- Preserve invariants: UART markers, Host path byte-compatibility, `os-lite` feature gating.
- Do not introduce cross-layer dependencies. Kernel ↔ Services boundaries are strict.
- Prefer edits over refactors. If refactor spans modules, require an ADR.
- Keep every touched file’s CONTEXT header in sync with `docs/standards/DOCUMENTATION_STANDARDS.md`. Adjust fields (CONTEXT/OWNERS/STATUS/API_STABILITY/TEST_COVERAGE/ADR) instead of deleting them.

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
- Should depend on `userspace/nexus-loader` for ELF/load routines.

### Loader (`userspace/nexus-loader`)
- Library providing load/ELF/ABI routines for user programs.
- Used by `execd` and test harnesses. Kernel bridge delegates here.

### Kernel bridge (`source/kernel/neuron/src/user_loader.rs`)
- Thin bridge for user-space spawn ABI. No business logic duplication.

### Deprecated target (`source/apps/init-lite`)
- Marked deprecated; keep as wrapper or pointer to `nexus-init`.

### Header & ADR discipline
- Before editing any module, verify it carries the standard header block defined in `docs/standards/DOCUMENTATION_STANDARDS.md`.
- If the header is missing or outdated, fix it in the same prompt. Missing headers block review.
- Reference a relevant ADR (or add one) whenever architectural boundaries change.

## Dont Touch List (without ADR)
- Kernel memory manager internals
- Syscall ABI surface
- UART marker contract and self-test probes

## Testing
- Host: `cargo test --workspace`
- OS-lite: `just test-os`

## Links
- ADR-0001 (Runtime Roles & Boundaries): `docs/adr/0001-runtime-roles-and-boundaries.md`
- Task index: `tasks/`
