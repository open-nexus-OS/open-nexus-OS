# Agents

**Read `CLAUDE.md`. It is the single source of truth for AI agents in this
repo** — repo map, build/verify commands, protection zones, and invariants.

Three rules that must survive any context compression:

1. **No fake green.** UART markers (`*: ready`, `SELFTEST: * ok`) only after
   real behavior; stubs say `stub`/`placeholder`.
2. **Never commit without explicit user approval** in the current session.
3. **Kernel (`source/kernel/**`) and core libs (`source/libs/**`) are
   read-only** unless the user explicitly asks for changes there.

Crate-local notes live next to the code (e.g.
`source/init/nexus-init/AGENTS.md`).
