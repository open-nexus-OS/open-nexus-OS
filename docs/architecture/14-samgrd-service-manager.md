# `samgrd` (service manager) — onboarding

`samgrd` is the **service registry authority** on the OS: services register and clients resolve named targets through it.

Related docs:

- Service architecture (direction): `docs/adr/0017-service-architecture.md`
- Host-first domain library: `docs/architecture/03-samgr.md` (`userspace/samgr`)
- IPC/capability transport contract: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- Testing + marker discipline: `docs/testing/index.md` and `scripts/qemu-test.sh`

**Scope note:** `docs/architecture/03-samgr.md` is the host-first library; this page is the OS daemon authority.

## Responsibilities

- Maintain the OS service registry (name → endpoint/capability routing).
- Provide deterministic, testable behavior for:
  - register / lookup / restart / heartbeat (as the OS contract evolves),
  - error reporting for unknown/malformed requests.

## Relationship to `userspace/samgr`

`userspace/samgr` exists for **host-first testing** (in-memory registry).
`samgrd` is the **OS daemon** that exposes the service manager over IPC/IDL.

To avoid “in-proc registry drift”:

- host harnesses should use `userspace/samgr` directly,
- OS code should talk to `samgrd` via `nexus-ipc` + IDL bindings.

## Proof expectations

`samgrd` behavior must be proven via:

- host-first integration tests (where possible), and
- QEMU smoke markers enforced by `scripts/qemu-test.sh` (canonical truth for ordering/presence).

If marker semantics change, update the task that owns the DoD and then update the harness.
