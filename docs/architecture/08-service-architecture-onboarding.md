# Service architecture (onboarding)

This page explains how “services” are structured in Open Nexus OS and how that connects to the host-first workflow.

Canonical decision record:

- `docs/adr/0017-service-architecture.md` (service architecture direction)

## What is a “service” here?

A service is a **process** (daemon) under `source/services/<name>d` that:

- registers with the service manager (`samgrd`),
- exposes Cap’n Proto IDL interfaces,
- validates inputs and propagates rich errors,
- and forwards into a corresponding **userspace domain library** compiled for `nexus_env="os"`.

The goal is that business rules remain host-testable and safe.

## Host-first structure (domain vs adapter)

- **Domain libraries** live in `userspace/` and are designed to run on the host:
  - unit/property tests
  - contract tests
  - Miri (where applicable)
- **Daemons** in `source/services/*d` are thin adapters:
  - wiring and lifecycle
  - IPC/IDL translation
  - deterministic readiness markers

If service code starts accumulating “real logic”, that’s usually a sign the boundary is leaking.

## How services communicate

The control plane uses Cap’n Proto IDL:

- Schemas: `tools/nexus-idl/` (`*.capnp`)
- Generated runtime: `userspace/nexus-idl-runtime`

Transport and capability semantics are kernel-defined and specified in:

- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`

## Readiness + proof (no fake green)

Services must not claim readiness unless they truly are ready.

Typical marker responsibility split:

- init orchestrator prints `init: start <svc>` / `init: up <svc>`
- each service prints `<svc>: ready` once it can accept requests
- `scripts/qemu-test.sh` enforces marker ordering/presence

## Where to add tests

- **Most behavior**: add tests in the userspace crate.
- **Integration flows**: add host E2E tests under `tests/` (fast, deterministic).
- **Bare-metal smoke**: rely on `scripts/qemu-test.sh` and keep proofs bounded.

## Useful entry points

- Testing methodology: `docs/testing/index.md`
- Tasks workflow: `tasks/README.md`
- Layering and quick reference: `docs/ARCHITECTURE.md`
