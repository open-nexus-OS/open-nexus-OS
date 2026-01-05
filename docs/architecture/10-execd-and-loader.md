# `execd` + loader path — onboarding

`execd` is the **spawner/execution authority**: it turns a “run this payload” request into a real process.
This is the core of “the OS can run something real” and is therefore heavily proof-driven.

Canonical sources:

- Roles/boundaries: `docs/adr/0001-runtime-roles-and-boundaries.md`
- Loader safety/guards: `docs/rfcs/RFC-0004-safe-loader-guards.md`
- Packaging contract: `docs/packaging/nxb.md`
- QEMU proof harness: `scripts/qemu-test.sh` (marker contract)
- Testing guide: `docs/testing/index.md`

## Responsibilities

- **Spawn services and tasks** as real processes (process-per-service architecture).
- **Delegate loading** to the shared loader library (`userspace/nexus-loader`) rather than duplicating ELF parsing/mapping.
- **Work with bundle packaging** (`bundlemgrd` ↔ `execd`) so installed bundles can be executed.

## Non-goals

- Kernel-space ELF parsing/policy/crypto. The kernel provides minimal primitives; the loader and policy live in userspace.
- “In-proc” fake execution paths (host harnesses can emulate, OS path must be real).

## How the execution pipeline fits together

High-level flow (OS):

1. A caller requests execution (often via init bring-up or a service request).
2. `execd` obtains the payload bytes:
   - for packaged apps: via `bundlemgrd.getPayload(...)` (see `docs/packaging/nxb.md`)
   - for fixtures: via `userspace/exec-payloads`
3. `execd` stages payload bytes into a VMO and calls into `userspace/nexus-loader` to validate and map segments.
4. The kernel enforces W^X and address-space rules at mapping syscalls.
5. `spawn` launches the child task with a guarded stack and bootstrap message.

## Proof and markers (no fake green)

Execution is proven via:

- **Host tests**: loader unit tests and host E2E harnesses (fast feedback).
- **QEMU smoke**: marker contract enforced by `scripts/qemu-test.sh`.

When you change anything about the exec pipeline:

- Update the task(s) that own the proof signals.
- Keep marker semantics honest (no “ok” logs unless the behavior happened).

## Debugging tips

- If you see `init: up execd` but never see `execd: ready`, the daemon likely failed early.
- If `execd: ready` appears but payload execution markers are missing, focus on:
  - packaging handshake (`bundlemgrd.getPayload`)
  - loader safety guards (`RFC-0004`)
  - kernel mapping syscalls / W^X enforcement

Always treat `scripts/qemu-test.sh` as the truth for “what must appear”.
