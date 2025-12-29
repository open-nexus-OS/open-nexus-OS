---
title: TASK-0164 SDK v1 Part 1b (host-first): typed client stubs + app templates + nx sdk (nx-sdk wrapper) + doctor/build helpers
status: Draft
owner: @devx
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - SDK v1 IDL/codegen/gates: tasks/TASK-0163-sdk-v1-part1a-idl-freeze-codegen-wire-gates.md
  - DevX nx CLI v1: tasks/TASK-0045-devx-nx-cli-v1.md
  - Existing schema runtime: userspace/nexus-idl-runtime/
---

## Context

Once SDK v1 IDLs and gates exist (`TASK-0163`), developers need:

- ergonomic typed client wrappers over generated Cap’n Proto bindings,
- a known-good app template,
- a simple CLI UX to scaffold and validate the local toolchain.

Repo reality: there is already a plan for a unified `tools/nx` CLI (`TASK-0045`).
To avoid tool sprawl, we should implement SDK commands as `nx sdk ...` and optionally provide a thin `nx-sdk`
shim that forwards to `nx sdk` for compatibility with older prompts.

## Goal

Deliver:

1. Typed SDK client wrapper crate:
   - `userspace/libs/nx-sdk/` (or equivalent path)
   - wraps generated bindings from `sdk/idl/v1`
   - provides:
     - connector helpers (host-first)
     - typed errors and bounded timeouts
     - `with_deadline(ns)` convenience
   - no global mutable state; small surface; `no_std`-ready behind feature gate (optional)
2. App template:
   - `sdk/templates/app-hello/`:
     - minimal “Hello” app
     - example permissions manifest
     - uses `nx-sdk` typed client wrappers where possible
     - host tests compile the template deterministically
3. CLI UX:
   - `nx sdk doctor`:
     - checks `capnp`, Rust toolchain, and SDK gates readiness
   - `nx sdk new app <appId> --name ... --out ...`:
     - scaffolds template
   - `nx sdk build/run`:
     - host-first build helpers (no QEMU required)
   - optional `tools/nx-sdk` wrapper:
     - forwards to `tools/nx sdk ...` (no separate implementation logic)
4. Host tests:
   - scaffolder creates a compilable app in a temp dir
   - `doctor` output is stable and correctly detects missing tools (PATH override fixture)
5. Docs:
   - `docs/sdk/overview.md`
   - `docs/sdk/app-template.md`
   - `docs/sdk/codegen.md` (links to `TASK-0163`)
   - `docs/sdk/gates.md`

## Non-Goals

- OS/QEMU proofs.
- A full app packaging/install flow (that’s packages tasks).
- Shipping a “real” SDK stable ABI beyond the IDL surface (this is SDK v1 foundation only).

## Constraints / invariants (hard requirements)

- Determinism: scaffolding produces stable output; doctor output stable; no random IDs in generated project files.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No duplicate CLIs: `nx sdk` is canonical; `nx-sdk` (if present) is a thin shim only.

## Red flags / decision points (track explicitly)

- **YELLOW (client transport contracts)**:
  - typed clients need a stable host connector story for each service. If a service is OS-only today, wrappers must be explicit `Unsupported` on host.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p sdk_v1_part1_host -- --nocapture`
    - `./ci/sdk_gates.sh` (from `TASK-0163`)
  - Required proofs:
    - template scaffolds and `cargo check` passes in CI fixture
    - doctor detects missing `capnp` deterministically (fixture)

## Touched paths (allowlist)

- `userspace/libs/nx-sdk/` (new)
- `sdk/templates/` (new)
- `tools/nx/` (add `sdk` subcommands)
- `tools/nx-sdk/` (optional thin wrapper)
- `docs/sdk/` (new)

## Plan (small PRs)

1. Add `nx-sdk` wrapper crate + minimal typed clients for 1–2 services as a proof path
2. Add template + scaffolder command (`nx sdk new app ...`)
3. Add `doctor` + build helpers + host tests
4. Docs

## Acceptance criteria (behavioral)

- Developers can run `nx sdk doctor` and `nx sdk new app ...` and get a compiling project deterministically.
